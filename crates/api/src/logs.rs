//! Daily log files, retention cleanup, and the log viewer API
//! (docs/13-deployment.md "Logging", docs/10-configuration.md).
//!
//! Each tracing event is written as a single JSON line to
//! `<data_dir>/logs/unseen-YYYY-MM-DD.log` so that lines logged before a crash
//! or restart remain available through `GET /api/v1/logs` and file download.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use axum::Json;
use axum::body::Body;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, NaiveDate, SecondsFormat, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::io::ReaderStream;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use utoipa::{IntoParams, ToSchema};

use crate::AppState;
use crate::access::{Require, SystemLogs};
use crate::error::Problem;

/// Database setting key for daily log file retention.
pub const RETENTION_DAYS_KEY: &str = "logs.retention_days";
/// Default number of daily log files retained (`docs/10-configuration.md`).
pub const DEFAULT_RETENTION_DAYS: u32 = 14;

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 500;
const LOG_READ_CHUNK_SIZE: usize = 8192;
const FILE_PREFIX: &str = "unseen-";
const FILE_SUFFIX: &str = ".log";

/// Output format for stdout logging (`UNSEEN_LOG_FORMAT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Text,
    Json,
}

/// Parses `UNSEEN_LOG_LEVEL`, defaulting to `info`.
#[must_use]
pub fn parse_level_filter(raw: Option<&str>) -> LevelFilter {
    match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("error") => LevelFilter::ERROR,
        Some("warn") | Some("warning") => LevelFilter::WARN,
        Some("info") | Some("") | None => LevelFilter::INFO,
        Some("debug") => LevelFilter::DEBUG,
        Some("trace") => LevelFilter::TRACE,
        Some(_) => LevelFilter::INFO,
    }
}

/// Parses `UNSEEN_LOG_FORMAT`, defaulting to `text`.
#[must_use]
pub fn parse_log_format(raw: Option<&str>) -> LogFormat {
    match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("json") => LogFormat::Json,
        _ => LogFormat::Text,
    }
}

/// Returns the daily log filename for `date`, e.g. `unseen-2026-11-26.log`.
#[must_use]
pub fn log_file_name(date: NaiveDate) -> String {
    format!("{FILE_PREFIX}{}{FILE_SUFFIX}", date.format("%Y-%m-%d"))
}

/// Parses a strictly valid daily log filename (`unseen-YYYY-MM-DD.log`).
///
/// Rejects path separators, traversal sequences, extra characters, and invalid
/// calendar dates.
#[must_use]
pub fn parse_log_file_date(name: &str) -> Option<NaiveDate> {
    let date_part = name.strip_prefix(FILE_PREFIX)?.strip_suffix(FILE_SUFFIX)?;
    if date_part.len() != 10 {
        return None;
    }
    let bytes = date_part.as_bytes();
    if bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    if !bytes
        .iter()
        .enumerate()
        .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
    {
        return None;
    }
    let date = NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()?;
    (log_file_name(date) == name).then_some(date)
}

/// Severity rank for level filtering (`error` highest, `trace` lowest).
fn parse_level_severity(raw: &str) -> Option<u8> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "error" => Some(4),
        "warn" => Some(3),
        "info" => Some(2),
        "debug" => Some(1),
        "trace" => Some(0),
        _ => None,
    }
}

/// One structured log entry written to a daily file and returned by the API.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// UTC ISO 8601 timestamp with milliseconds.
    pub timestamp: String,
    /// Severity level: `error`, `warn`, `info`, `debug`, or `trace`.
    pub level: String,
    /// Tracing target (module path).
    pub target: String,
    /// Log message.
    pub message: String,
    /// Structured fields attached to the event.
    #[serde(default)]
    pub fields: BTreeMap<String, Value>,
}

/// Metadata for one daily log file in the logs directory.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LogFileInfo {
    /// File name (`unseen-YYYY-MM-DD.log`).
    pub name: String,
    /// File size in bytes.
    pub size_bytes: u64,
    /// UTC date covered by this file (`YYYY-MM-DD`).
    pub date: String,
}

/// Query parameters for `GET /api/v1/logs`.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub struct LogsParams {
    /// Minimum severity level: `error`, `warn`, `info`, `debug`, or `trace`.
    pub level: Option<String>,
    /// Case-insensitive text filter matched against message, target, and fields.
    pub search: Option<String>,
    /// Opaque pagination cursor returned as `nextBefore` from a previous page.
    pub before: Option<String>,
    /// Number of entries to return (1 to 500, default 100).
    pub limit: Option<usize>,
}

/// Response body of `GET /api/v1/logs`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogsResponse {
    /// Matching log entries, newest first.
    pub items: Vec<LogEntry>,
    /// Cursor to pass as `before` to load older entries; `null` at the oldest entry.
    pub next_before: Option<String>,
    /// Available daily log files, newest first.
    pub files: Vec<LogFileInfo>,
    /// Configured log retention window in days (`logs.retention_days`).
    pub retention_days: u32,
}

/// Destination for formatted JSON log lines.
pub trait LineSink: Send + Sync + 'static {
    fn write_line(&self, now: DateTime<Utc>, line: &[u8]) -> io::Result<()>;
}

struct DailyFileState {
    date: Option<NaiveDate>,
    file: Option<File>,
}

/// Appends JSON log lines to daily files `<dir>/unseen-YYYY-MM-DD.log`.
pub struct DailyFileSink {
    dir: PathBuf,
    state: Mutex<DailyFileState>,
}

impl DailyFileSink {
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            state: Mutex::new(DailyFileState {
                date: None,
                file: None,
            }),
        }
    }
}

impl LineSink for DailyFileSink {
    fn write_line(&self, now: DateTime<Utc>, line: &[u8]) -> io::Result<()> {
        let today = now.date_naive();
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.date != Some(today) || guard.file.is_none() {
            std::fs::create_dir_all(&self.dir)?;
            let path = self.dir.join(log_file_name(today));
            let file = OpenOptions::new().create(true).append(true).open(path)?;
            guard.date = Some(today);
            guard.file = Some(file);
        }
        if let Some(file) = guard.file.as_mut() {
            file.write_all(line)?;
            file.flush()?;
        }
        Ok(())
    }
}

/// Writes JSON log lines to standard output.
pub struct StdoutSink;

impl LineSink for StdoutSink {
    fn write_line(&self, _now: DateTime<Utc>, line: &[u8]) -> io::Result<()> {
        let mut out = io::stdout().lock();
        out.write_all(line)?;
        out.flush()
    }
}

type ClockFn = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

/// Tracing layer that formats events as JSONL and writes them to a [`LineSink`].
pub struct JsonLineLayer<S = DailyFileSink> {
    sink: S,
    clock: ClockFn,
}

impl JsonLineLayer<DailyFileSink> {
    /// Creates a tracing layer writing daily JSONL log files into `logs_dir`.
    #[must_use]
    pub fn daily_file(logs_dir: PathBuf) -> Self {
        Self::with_sink(DailyFileSink::new(logs_dir))
    }
}

impl JsonLineLayer<StdoutSink> {
    /// Creates a tracing layer writing JSONL log lines to stdout.
    #[must_use]
    pub fn stdout() -> Self {
        Self::with_sink(StdoutSink)
    }
}

impl<S: LineSink> JsonLineLayer<S> {
    /// Creates a tracing layer backed by `sink` using the UTC system clock.
    #[must_use]
    pub fn with_sink(sink: S) -> Self {
        Self::with_sink_and_clock(sink, || DateTime::<Utc>::from(SystemTime::now()))
    }

    /// Creates a tracing layer backed by `sink` and a custom clock (useful in tests).
    #[must_use]
    pub fn with_sink_and_clock(
        sink: S,
        clock: impl Fn() -> DateTime<Utc> + Send + Sync + 'static,
    ) -> Self {
        Self {
            sink,
            clock: Arc::new(clock),
        }
    }
}

struct JsonFieldVisitor {
    message: String,
    fields: BTreeMap<String, Value>,
}

impl JsonFieldVisitor {
    fn new() -> Self {
        Self {
            message: String::new(),
            fields: BTreeMap::new(),
        }
    }

    fn insert(&mut self, name: &str, value: Value) {
        if name == "message" {
            if let Some(text) = value.as_str() {
                self.message = text.to_owned();
            } else {
                self.message = value.to_string();
            }
        } else {
            self.fields.insert(name.to_owned(), value);
        }
    }
}

impl Visit for JsonFieldVisitor {
    fn record_f64(&mut self, field: &Field, value: f64) {
        let json_value = serde_json::Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(value.to_string()));
        self.insert(field.name(), json_value);
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.insert(field.name(), Value::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.insert(field.name(), Value::from(value));
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        self.insert(field.name(), Value::String(value.to_string()));
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        self.insert(field.name(), Value::String(value.to_string()));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.insert(field.name(), Value::Bool(value));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.insert(field.name(), Value::String(value.to_owned()));
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.insert(field.name(), Value::String(value.to_string()));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.insert(field.name(), Value::String(format!("{value:?}")));
    }
}

impl<Sub, S> Layer<Sub> for JsonLineLayer<S>
where
    Sub: Subscriber,
    S: LineSink,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, Sub>) {
        let now = (self.clock)();
        let mut visitor = JsonFieldVisitor::new();
        event.record(&mut visitor);

        let metadata = event.metadata();
        let entry = LogEntry {
            timestamp: now.to_rfc3339_opts(SecondsFormat::Millis, true),
            level: metadata.level().as_str().to_ascii_lowercase(),
            target: metadata.target().to_owned(),
            message: visitor.message,
            fields: visitor.fields,
        };

        if let Ok(mut bytes) = serde_json::to_vec(&entry) {
            bytes.push(b'\n');
            let _ = self.sink.write_line(now, &bytes);
        }
    }
}

/// Creates the daily file tracing layer writing JSONL files into `logs_dir`.
#[must_use]
pub fn file_layer(logs_dir: PathBuf) -> JsonLineLayer<DailyFileSink> {
    JsonLineLayer::daily_file(logs_dir)
}

/// Initializes process-wide tracing: stdout in `format` (`text` or `json`)
/// plus daily JSONL files in `logs_dir`, filtered by `level`.
pub fn init_tracing(logs_dir: PathBuf, level: LevelFilter, format: LogFormat) {
    let registry = tracing_subscriber::registry()
        .with(level)
        .with(file_layer(logs_dir));
    match format {
        LogFormat::Text => {
            registry.with(tracing_subscriber::fmt::layer()).init();
        }
        LogFormat::Json => {
            registry.with(JsonLineLayer::stdout()).init();
        }
    }
}

/// Resolves `logs.retention_days` from `UNSEEN_SETTING_LOGS_RETENTION_DAYS`,
/// the `settings` table, or [`DEFAULT_RETENTION_DAYS`].
pub async fn retention_days(state: &AppState) -> Result<u32, Problem> {
    if let Some(raw) = state.settings_overrides.get(RETENTION_DAYS_KEY) {
        return Ok(parse_retention_override(raw.expose()));
    }
    let stored = db::settings::get_i64(&state.db, RETENTION_DAYS_KEY)
        .await
        .map_err(|error| {
            tracing::error!(%error, "read logs.retention_days");
            Problem::internal()
        })?;
    Ok(stored
        .and_then(|value| u32::try_from(value).ok())
        .filter(|&days| days >= 1)
        .unwrap_or(DEFAULT_RETENTION_DAYS))
}

fn parse_retention_override(raw: &str) -> u32 {
    match raw.trim().parse::<u32>() {
        Ok(days) if days >= 1 => days,
        _ => {
            tracing::warn!(
                key = RETENTION_DAYS_KEY,
                "invalid UNSEEN_SETTING_LOGS_RETENTION_DAYS, using default"
            );
            DEFAULT_RETENTION_DAYS
        }
    }
}

/// Removes daily log files in `logs_dir` older than `retention_days` relative
/// to `today` (keeping `retention_days` daily files including `today`).
///
/// Ignores non-matching files and subdirectories. Returns the number of
/// removed files.
pub fn prune_old_logs(logs_dir: &Path, retention_days: u32, today: NaiveDate) -> io::Result<usize> {
    let entries = match std::fs::read_dir(logs_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };

    let keep_days = i64::from(retention_days.max(1));
    let cutoff = today - TimeDelta::days(keep_days - 1);
    let mut removed = 0;

    for entry in entries {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Some(date) = parse_log_file_date(&name) else {
            continue;
        };
        if date < cutoff {
            let file_type = entry.file_type()?;
            if file_type.is_file() {
                std::fs::remove_file(entry.path())?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}

/// Runs log retention cleanup at startup using the effective `logs.retention_days`.
pub async fn prune_expired_on_startup(state: &AppState) {
    let days = match retention_days(state).await {
        Ok(days) => days,
        Err(_) => DEFAULT_RETENTION_DAYS,
    };
    let dir = state.logs_dir.clone();
    let today = DateTime::<Utc>::from(SystemTime::now()).date_naive();
    match tokio::task::spawn_blocking(move || prune_old_logs(&dir, days, today)).await {
        Ok(Ok(removed)) if removed > 0 => {
            tracing::info!(removed, retention_days = days, "pruned expired log files");
        }
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            tracing::warn!(%error, "failed to prune expired log files");
        }
        Err(error) => {
            tracing::warn!(%error, "log pruning task failed");
        }
    }
}

/// Lists valid daily log files in `logs_dir`, sorted newest date first.
pub fn list_log_files(logs_dir: &Path) -> io::Result<Vec<(NaiveDate, LogFileInfo)>> {
    if logs_dir.as_os_str().is_empty() {
        return Ok(Vec::new());
    }
    let entries = match std::fs::read_dir(logs_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };

    let canonical_logs_dir = match std::fs::canonicalize(logs_dir) {
        Ok(path) => path,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };

    let mut files = Vec::new();
    for entry in entries {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Some(date) = parse_log_file_date(&name) else {
            continue;
        };
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if !metadata.is_file() {
            continue;
        }
        let canonical_path = std::fs::canonicalize(&path)?;
        if !canonical_path.starts_with(&canonical_logs_dir) {
            continue;
        }
        files.push((
            date,
            LogFileInfo {
                name,
                size_bytes: metadata.len(),
                date: date.format("%Y-%m-%d").to_string(),
            },
        ));
    }
    files.sort_by(|(a, _), (b, _)| b.cmp(a));
    Ok(files)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cursor {
    date: NaiveDate,
    offset: u64,
}

impl Cursor {
    fn parse(raw: &str) -> Option<Self> {
        let (date_part, line_part) = raw.split_once(':')?;
        if date_part.len() != 10 {
            return None;
        }
        let date = NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()?;
        let offset = line_part.parse::<u64>().ok()?;
        Some(Self { date, offset })
    }

    fn encode(self) -> String {
        format!("{}:{}", self.date.format("%Y-%m-%d"), self.offset)
    }
}

fn parse_log_line(line: &str) -> LogEntry {
    match serde_json::from_str::<LogEntry>(line) {
        Ok(mut entry) => {
            entry.level = entry.level.to_ascii_lowercase();
            entry
        }
        Err(_) => LogEntry {
            timestamp: String::new(),
            level: "info".to_owned(),
            target: String::new(),
            message: line.to_owned(),
            fields: BTreeMap::new(),
        },
    }
}

fn matches_filters(entry: &LogEntry, min_severity: Option<u8>, needle: Option<&str>) -> bool {
    if let Some(min_sev) = min_severity {
        let entry_sev = parse_level_severity(&entry.level).unwrap_or(2);
        if entry_sev < min_sev {
            return false;
        }
    }
    if let Some(needle) = needle {
        let in_message = entry.message.to_ascii_lowercase().contains(needle);
        let in_target = entry.target.to_ascii_lowercase().contains(needle);
        let in_fields = !entry.fields.is_empty()
            && serde_json::to_string(&entry.fields)
                .map(|serialized| serialized.to_ascii_lowercase().contains(needle))
                .unwrap_or(false);
        if !in_message && !in_target && !in_fields {
            return false;
        }
    }
    true
}

struct QueryResult {
    items: Vec<LogEntry>,
    next_before: Option<String>,
    files: Vec<LogFileInfo>,
}

fn read_logs(
    logs_dir: &Path,
    min_severity: Option<u8>,
    search_needle: Option<&str>,
    before: Option<Cursor>,
    limit: usize,
) -> io::Result<QueryResult> {
    let dated_files = list_log_files(logs_dir)?;
    let files: Vec<LogFileInfo> = dated_files.iter().map(|(_, info)| info.clone()).collect();

    let mut items = Vec::new();
    let mut last_cursor: Option<Cursor> = None;
    let mut has_more = false;

    'files: for (file_date, info) in &dated_files {
        if let Some(cursor) = before
            && *file_date > cursor.date
        {
            continue;
        }

        let path = match safe_log_file_path(logs_dir, &info.name) {
            Ok(Some(path)) => path,
            Ok(None) => continue,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let file_len = file.metadata()?.len();
        let upper_bound = match before {
            Some(cursor) if *file_date == cursor.date => cursor.offset.min(file_len),
            _ => file_len,
        };

        let mut page = PageState {
            min_severity,
            search_needle,
            limit,
            items: &mut items,
            last_cursor: &mut last_cursor,
        };
        if scan_file_backwards(&mut file, *file_date, upper_bound, file_len, &mut page)? {
            has_more = true;
            break 'files;
        }
    }

    let next_before = if has_more {
        last_cursor.map(Cursor::encode)
    } else {
        None
    };

    Ok(QueryResult {
        items,
        next_before,
        files,
    })
}

struct PageState<'a> {
    min_severity: Option<u8>,
    search_needle: Option<&'a str>,
    limit: usize,
    items: &'a mut Vec<LogEntry>,
    last_cursor: &'a mut Option<Cursor>,
}

fn safe_log_file_path(logs_dir: &Path, name: &str) -> io::Result<Option<PathBuf>> {
    let path = logs_dir.join(name);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_file() {
        return Ok(None);
    }

    let canonical_logs_dir = std::fs::canonicalize(logs_dir)?;
    let canonical_path = std::fs::canonicalize(path)?;
    if !canonical_path.starts_with(canonical_logs_dir) {
        return Ok(None);
    }
    Ok(Some(canonical_path))
}

#[cfg(unix)]
fn same_file_identity(
    path_metadata: &std::fs::Metadata,
    file_metadata: &std::fs::Metadata,
) -> bool {
    use std::os::unix::fs::MetadataExt;

    path_metadata.dev() == file_metadata.dev() && path_metadata.ino() == file_metadata.ino()
}

#[cfg(windows)]
fn same_file_identity(identity: &same_file::Handle, file: &File) -> io::Result<bool> {
    same_file::Handle::from_file(file.try_clone()?).map(|opened| identity == &opened)
}

fn scan_file_backwards(
    file: &mut File,
    date: NaiveDate,
    upper_bound: u64,
    file_len: u64,
    page: &mut PageState<'_>,
) -> io::Result<bool> {
    let mut position = upper_bound;
    let mut trailing = Vec::new();
    let mut chunk = vec![0; LOG_READ_CHUNK_SIZE];
    let mut first_chunk = true;

    while position > 0 {
        let chunk_start = position.saturating_sub(LOG_READ_CHUNK_SIZE as u64);
        let chunk_len = (position - chunk_start) as usize;
        file.seek(SeekFrom::Start(chunk_start))?;
        file.read_exact(&mut chunk[..chunk_len])?;

        let mut combined = Vec::with_capacity(chunk_len + trailing.len());
        combined.extend_from_slice(&chunk[..chunk_len]);
        combined.extend_from_slice(&trailing);

        let mut line_start = 0;
        let mut lines = Vec::new();
        for (line_end, byte) in combined.iter().enumerate() {
            if *byte != b'\n' {
                continue;
            }
            lines.push((line_start, line_end));
            line_start = line_end + 1;
        }

        if first_chunk
            && upper_bound == file_len
            && line_start < combined.len()
            && (!lines.is_empty() || chunk_start == 0)
            && process_log_line(
                &combined[line_start..],
                chunk_start + line_start as u64,
                date,
                page,
            )
        {
            return Ok(true);
        }

        for &(start, end) in lines.iter().rev().take(lines.len().saturating_sub(1)) {
            if process_log_line(
                &combined[start..end],
                chunk_start + start as u64,
                date,
                page,
            ) {
                return Ok(true);
            }
        }

        let tail_was_processed = first_chunk && upper_bound == file_len && lines.is_empty();
        trailing.clear();
        if let Some(&(_, end)) = lines.first() {
            trailing.extend_from_slice(&combined[..end + 1]);
        } else {
            trailing.extend_from_slice(&combined);
        }

        if chunk_start == 0 {
            if !trailing.is_empty()
                && !tail_was_processed
                && process_log_line(&trailing, 0, date, page)
            {
                return Ok(true);
            }
            break;
        }
        position = chunk_start;
        first_chunk = false;
    }

    Ok(false)
}

fn process_log_line(bytes: &[u8], offset: u64, date: NaiveDate, page: &mut PageState<'_>) -> bool {
    let line = String::from_utf8_lossy(bytes).trim().to_owned();
    if line.is_empty() {
        return false;
    }
    let entry = parse_log_line(&line);
    if !matches_filters(&entry, page.min_severity, page.search_needle) {
        return false;
    }
    if page.items.len() < page.limit {
        page.items.push(entry);
        *page.last_cursor = Some(Cursor { date, offset });
        false
    } else {
        true
    }
}

/// Recent log entries from daily log files, newest first, with level and text
/// filters and backwards pagination.
#[utoipa::path(
    get,
    path = "/api/v1/logs",
    operation_id = "list_logs",
    tag = "system",
    params(LogsParams),
    responses(
        (status = 200, description = "Matching log entries and available files", body = LogsResponse),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn list(
    _: Require<SystemLogs>,
    State(state): State<AppState>,
    Query(params): Query<LogsParams>,
) -> Result<Json<LogsResponse>, Problem> {
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(Problem::validation("limit", "must be 1 to 500"));
    }

    let min_severity = match params.level.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(raw) => Some(parse_level_severity(raw).ok_or_else(|| {
            Problem::validation("level", "must be one of error, warn, info, debug, trace")
        })?),
    };

    let before = match params.before.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(raw) => Some(
            Cursor::parse(raw)
                .ok_or_else(|| Problem::validation("before", "must be a valid log cursor"))?,
        ),
    };

    let search = params
        .search
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);

    let retention_days = retention_days(&state).await?;
    let logs_dir = state.logs_dir.clone();

    let result = tokio::task::spawn_blocking(move || {
        read_logs(&logs_dir, min_severity, search.as_deref(), before, limit)
    })
    .await
    .map_err(|error| {
        tracing::error!(%error, "read logs task failed");
        Problem::internal()
    })?
    .map_err(|error| {
        tracing::error!(%error, "read log files");
        Problem::internal()
    })?;

    Ok(Json(LogsResponse {
        items: result.items,
        next_before: result.next_before,
        files: result.files,
        retention_days,
    }))
}

/// Downloads one daily log file (`unseen-YYYY-MM-DD.log`).
#[utoipa::path(
    get,
    path = "/api/v1/logs/files/{name}",
    operation_id = "download_log_file",
    tag = "system",
    params(
        ("name" = String, Path, description = "Daily log file name (`unseen-YYYY-MM-DD.log`)")
    ),
    responses(
        (status = 200, description = "The daily log file contents", content_type = "text/plain; charset=utf-8", body = String),
        (status = 400, description = "Invalid log file name", body = Problem),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "Log file not found", body = Problem),
    )
)]
pub async fn download(
    _: Require<SystemLogs>,
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Response, Problem> {
    if parse_log_file_date(&name).is_none() {
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "invalid_log_file",
            "Invalid log file name",
        ));
    }

    if state.logs_dir.as_os_str().is_empty() {
        return Err(Problem::not_found());
    }

    let path = state.logs_dir.join(&name);
    let file = tokio::task::spawn_blocking(move || -> io::Result<Option<File>> {
        #[cfg(windows)]
        // Snapshot the path identity before checking its type; a later type check catches a pre-existing symlink,
        // and comparing the opened handle catches any replacement between the check and open.
        let path_identity = match same_file::Handle::from_path(&path) {
            Ok(identity) => identity,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        if !meta.file_type().is_file() {
            return Ok(None);
        }

        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let file = match options.open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            #[cfg(unix)]
            Err(error) if error.raw_os_error() == Some(libc::ELOOP) => return Ok(None),
            Err(error) => return Err(error),
        };
        let opened_meta = file.metadata()?;
        // This identity check closes a path-swap race on Windows; Unix also uses O_NOFOLLOW on the final component.
        #[cfg(unix)]
        let same_identity = same_file_identity(&meta, &opened_meta);
        #[cfg(windows)]
        let same_identity = same_file_identity(&path_identity, &file)?;
        if !opened_meta.is_file() || !same_identity {
            return Ok(None);
        }
        Ok(Some(file))
    })
    .await
    .map_err(|error| {
        tracing::error!(%error, "download log task failed");
        Problem::internal()
    })?
    .map_err(|error| {
        tracing::error!(%error, "read log file for download");
        Problem::internal()
    })?
    .ok_or_else(Problem::not_found)?;

    let disposition = HeaderValue::from_str(&format!("attachment; filename=\"{name}\""))
        .map_err(|_| Problem::internal())?;
    let content_type = HeaderValue::from_static("text/plain; charset=utf-8");

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        Body::from_stream(ReaderStream::new(tokio::fs::File::from_std(file))),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tracing_subscriber::layer::SubscriberExt;

    #[test]
    fn parses_log_level_and_format() {
        assert_eq!(parse_level_filter(None), LevelFilter::INFO);
        assert_eq!(parse_level_filter(Some("")), LevelFilter::INFO);
        assert_eq!(parse_level_filter(Some("ERROR")), LevelFilter::ERROR);
        assert_eq!(parse_level_filter(Some("warn")), LevelFilter::WARN);
        assert_eq!(parse_level_filter(Some("warning")), LevelFilter::WARN);
        assert_eq!(parse_level_filter(Some("info")), LevelFilter::INFO);
        assert_eq!(parse_level_filter(Some("debug")), LevelFilter::DEBUG);
        assert_eq!(parse_level_filter(Some("trace")), LevelFilter::TRACE);
        assert_eq!(parse_level_filter(Some("bogus")), LevelFilter::INFO);

        assert_eq!(parse_log_format(None), LogFormat::Text);
        assert_eq!(parse_log_format(Some("text")), LogFormat::Text);
        assert_eq!(parse_log_format(Some("JSON")), LogFormat::Json);
        assert_eq!(parse_log_format(Some("other")), LogFormat::Text);
    }

    #[test]
    fn validates_daily_log_filenames_strictly() {
        assert_eq!(
            parse_log_file_date("unseen-2026-11-26.log"),
            Some(NaiveDate::from_ymd_opt(2026, 11, 26).unwrap())
        );
        for invalid in [
            "",
            "unseen.log",
            "unseen-2026-11-26.txt",
            "unseen-2026-02-31.log",
            "unseen-2026-1-1.log",
            "../unseen-2026-11-26.log",
            "sub/unseen-2026-11-26.log",
            "sub\\unseen-2026-11-26.log",
            "unseen-2026-11-26.log.bak",
            "secret",
        ] {
            assert_eq!(parse_log_file_date(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn rotates_daily_file_when_utc_date_changes() {
        let dir = tempfile::tempdir().unwrap();
        let day_index = Arc::new(AtomicUsize::new(0));
        let dates = [
            DateTime::parse_from_rfc3339("2026-11-25T23:59:59.000Z")
                .unwrap()
                .with_timezone(&Utc),
            DateTime::parse_from_rfc3339("2026-11-26T00:00:01.000Z")
                .unwrap()
                .with_timezone(&Utc),
        ];
        let counter = Arc::clone(&day_index);
        let layer = JsonLineLayer::with_sink_and_clock(
            DailyFileSink::new(dir.path().to_path_buf()),
            move || dates[counter.load(Ordering::SeqCst)],
        );
        let subscriber = tracing_subscriber::registry().with(layer);

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(
                count = 7_u64,
                signed = -3_i64,
                big_u = 9_u128,
                big_i = -9_i128,
                ratio = 1.5_f64,
                ok = true,
                "before midnight"
            );
            day_index.store(1, Ordering::SeqCst);
            let err: &(dyn std::error::Error + 'static) = &io::Error::other("boom");
            tracing::warn!(error = err, "after midnight");
        });

        let first = std::fs::read_to_string(dir.path().join("unseen-2026-11-25.log")).unwrap();
        let second = std::fs::read_to_string(dir.path().join("unseen-2026-11-26.log")).unwrap();
        let first_entry: LogEntry = serde_json::from_str(first.trim()).unwrap();
        let second_entry: LogEntry = serde_json::from_str(second.trim()).unwrap();

        assert_eq!(first_entry.message, "before midnight");
        assert_eq!(first_entry.level, "info");
        assert_eq!(first_entry.fields["count"], serde_json::json!(7));
        assert_eq!(first_entry.fields["signed"], serde_json::json!(-3));
        assert_eq!(first_entry.fields["ok"], serde_json::json!(true));
        assert_eq!(second_entry.message, "after midnight");
        assert_eq!(second_entry.level, "warn");
        assert_eq!(second_entry.fields["error"], serde_json::json!("boom"));
    }

    #[test]
    fn prunes_expired_daily_files_and_keeps_recent_and_unrelated() {
        let dir = tempfile::tempdir().unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();

        // Retention = 14 keeps 2026-11-13..=2026-11-26 (14 days) and deletes 2026-11-12 and older.
        for name in [
            "unseen-2026-11-01.log",
            "unseen-2026-11-12.log",
            "unseen-2026-11-13.log",
            "unseen-2026-11-26.log",
            "unrelated.txt",
        ] {
            std::fs::write(dir.path().join(name), "line\n").unwrap();
        }
        std::fs::create_dir(dir.path().join("unseen-2026-10-01.log")).unwrap();

        let removed = prune_old_logs(dir.path(), 14, today).unwrap();
        assert_eq!(removed, 2);
        assert!(!dir.path().join("unseen-2026-11-01.log").exists());
        assert!(!dir.path().join("unseen-2026-11-12.log").exists());
        assert!(dir.path().join("unseen-2026-11-13.log").exists());
        assert!(dir.path().join("unseen-2026-11-26.log").exists());
        assert!(dir.path().join("unrelated.txt").exists());
        assert!(dir.path().join("unseen-2026-10-01.log").is_dir());

        // Missing directory is not an error.
        let missing = dir.path().join("does-not-exist");
        assert_eq!(prune_old_logs(&missing, 14, today).unwrap(), 0);
    }

    #[test]
    fn fallback_parses_non_json_log_lines() {
        let entry = parse_log_line("plain text crash line");
        assert_eq!(entry.message, "plain text crash line");
        assert_eq!(entry.level, "info");
        assert!(entry.fields.is_empty());
    }
}
