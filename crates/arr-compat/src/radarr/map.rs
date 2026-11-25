//! Mapping between Unseen's data and the Radarr resources (docs/14, "Field
//! mapping"). `arr-compat` never touches the database, so the `*View` structs
//! hold what the API layer has read; they follow the `db` models.

use std::fmt;

use chrono::NaiveDate;
use serde::Deserialize;
use unseen_core::profile::{self, Item};
use unseen_core::quality::{self, Quality as CoreQuality};

use super::dto::*;

/// Where TMDB serves the images whose path Unseen stores.
const TMDB_IMAGES: &str = "https://image.tmdb.org/t/p/original";

/// Why a request body cannot become an Unseen operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapError {
    /// A field the operation needs is absent or empty.
    Missing(&'static str),
    /// A field has a value Unseen does not accept.
    Invalid(&'static str, String),
}

impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(field) => write!(f, "{field} is required"),
            Self::Invalid(field, why) => write!(f, "{field}: {why}"),
        }
    }
}

impl std::error::Error for MapError {}

// ---------------------------------------------------------------- qualities

impl From<quality::Source> for QualitySource {
    fn from(source: quality::Source) -> Self {
        match source {
            quality::Source::Unknown => Self::Unknown,
            quality::Source::Cam => Self::Cam,
            quality::Source::Telesync => Self::Telesync,
            quality::Source::Telecine => Self::Telecine,
            quality::Source::Workprint => Self::Workprint,
            quality::Source::Dvd => Self::Dvd,
            quality::Source::Tv => Self::Tv,
            quality::Source::WebDl => Self::WebDl,
            quality::Source::WebRip => Self::WebRip,
            quality::Source::Bluray => Self::Bluray,
        }
    }
}

impl From<quality::Modifier> for Modifier {
    fn from(modifier: quality::Modifier) -> Self {
        match modifier {
            quality::Modifier::None => Self::None,
            quality::Modifier::Regional => Self::Regional,
            quality::Modifier::Screener => Self::Screener,
            quality::Modifier::RawHd => Self::RawHd,
            quality::Modifier::BrDisk => Self::BrDisk,
            quality::Modifier::Remux => Self::Remux,
        }
    }
}

impl From<CoreQuality> for Quality {
    fn from(quality: CoreQuality) -> Self {
        Self {
            id: quality.id(),
            name: quality.name().to_owned(),
            source: quality.source().into(),
            resolution: i32::from(quality.resolution()),
            modifier: quality.modifier().into(),
        }
    }
}

/// The quality with this Radarr id; `Unknown` for an id Unseen lacks.
pub fn quality(id: i32) -> Quality {
    CoreQuality::from_id(id)
        .unwrap_or(CoreQuality::Unknown)
        .into()
}

fn quality_model(id: i32) -> QualityModel {
    QualityModel {
        quality: quality(id),
        revision: Revision::default(),
    }
}

// ---------------------------------------------------------------- languages

/// Radarr's language ids (`Language.cs` of the pinned release) by ISO 639-1
/// code.
const LANGUAGES: &[(&str, i32, &str)] = &[
    ("en", 1, "English"),
    ("fr", 2, "French"),
    ("es", 3, "Spanish"),
    ("de", 4, "German"),
    ("it", 5, "Italian"),
    ("da", 6, "Danish"),
    ("nl", 7, "Dutch"),
    ("ja", 8, "Japanese"),
    ("is", 9, "Icelandic"),
    ("zh", 10, "Chinese"),
    ("ru", 11, "Russian"),
    ("pl", 12, "Polish"),
    ("vi", 13, "Vietnamese"),
    ("sv", 14, "Swedish"),
    ("no", 15, "Norwegian"),
    ("fi", 16, "Finnish"),
    ("tr", 17, "Turkish"),
    ("pt", 18, "Portuguese"),
    ("el", 20, "Greek"),
    ("ko", 21, "Korean"),
    ("hu", 22, "Hungarian"),
    ("he", 23, "Hebrew"),
    ("lt", 24, "Lithuanian"),
    ("cs", 25, "Czech"),
    ("hi", 26, "Hindi"),
    ("ro", 27, "Romanian"),
    ("th", 28, "Thai"),
    ("bg", 29, "Bulgarian"),
    ("ar", 31, "Arabic"),
    ("uk", 32, "Ukrainian"),
    ("fa", 33, "Persian"),
    ("bn", 34, "Bengali"),
    ("sk", 35, "Slovak"),
    ("lv", 36, "Latvian"),
    ("ca", 38, "Catalan"),
    ("hr", 39, "Croatian"),
    ("sr", 40, "Serbian"),
    ("bs", 41, "Bosnian"),
    ("et", 42, "Estonian"),
    ("ta", 43, "Tamil"),
    ("id", 44, "Indonesian"),
    ("te", 45, "Telugu"),
    ("mk", 46, "Macedonian"),
    ("sl", 47, "Slovenian"),
    ("ml", 48, "Malayalam"),
    ("kn", 49, "Kannada"),
    ("sq", 50, "Albanian"),
    ("af", 51, "Afrikaans"),
    ("mr", 52, "Marathi"),
    ("tl", 53, "Tagalog"),
    ("ur", 54, "Urdu"),
    ("rm", 55, "Romansh"),
    ("mn", 56, "Mongolian"),
    ("ka", 57, "Georgian"),
    ("ga", 58, "Irish"),
];

/// The Radarr language of an Unseen profile or file language: `any`,
/// `original` or an ISO 639-1 code. Anything else is `Unknown`.
pub fn language(code: &str) -> Language {
    let code = code.trim().to_ascii_lowercase();
    let (id, name) = match code.as_str() {
        profile::LANGUAGE_ANY => (-1, "Any"),
        profile::LANGUAGE_ORIGINAL => (-2, "Original"),
        _ => LANGUAGES
            .iter()
            .find(|(known, ..)| *known == code)
            .map_or((0, "Unknown"), |(_, id, name)| (*id, *name)),
    };
    Language {
        id,
        name: name.to_owned(),
    }
}

// ------------------------------------------------------------------- movies

/// The release window a movie must reach before Radarr searches for it.
/// Radarr's `tba` is the old name of `announced` and maps to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinimumAvailability {
    Announced,
    InCinemas,
    Released,
}

impl MinimumAvailability {
    /// The value stored in `movies.minimum_availability`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Announced => "announced",
            Self::InCinemas => "inCinemas",
            Self::Released => "released",
        }
    }

    /// `None` for a value Unseen does not know.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "announced" => Some(Self::Announced),
            "inCinemas" => Some(Self::InCinemas),
            "released" => Some(Self::Released),
            _ => None,
        }
    }
}

impl From<MinimumAvailability> for MovieStatusType {
    fn from(value: MinimumAvailability) -> Self {
        match value {
            MinimumAvailability::Announced => Self::Announced,
            MinimumAvailability::InCinemas => Self::InCinemas,
            MinimumAvailability::Released => Self::Released,
        }
    }
}

impl TryFrom<MovieStatusType> for MinimumAvailability {
    type Error = MapError;

    fn try_from(value: MovieStatusType) -> Result<Self, MapError> {
        match value {
            MovieStatusType::Tba | MovieStatusType::Announced => Ok(Self::Announced),
            MovieStatusType::InCinemas => Ok(Self::InCinemas),
            MovieStatusType::Released => Ok(Self::Released),
            MovieStatusType::Deleted => Err(MapError::Invalid(
                "minimumAvailability",
                "`deleted` is not a minimum availability".to_owned(),
            )),
        }
    }
}

/// The file of a movie, as `movie_files` stores it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FileView {
    pub id: i32,
    /// Relative to the movie folder.
    pub relative_path: String,
    pub size: i64,
    pub quality_id: i32,
    pub release_group: Option<String>,
    pub edition: Option<String>,
    pub scene_name: Option<String>,
    /// Language codes, see [`language`].
    pub languages: Vec<String>,
    /// `None` while the file is not probed.
    pub media_info: Option<MediaInfoResource>,
    pub added_at: String,
}

/// A movie as `movies` stores it, with its root folder path, tags and the
/// primary profile and its file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MovieView {
    pub id: i32,
    pub tmdb_id: i32,
    pub imdb_id: Option<String>,
    pub title: String,
    pub original_title: Option<String>,
    pub sort_title: String,
    pub year: i32,
    pub overview: Option<String>,
    pub runtime: Option<i32>,
    /// `tba`, `announced`, `inCinemas`, `released` or `deleted`.
    pub status: String,
    /// A date (`2021-09-15`) or a date-time.
    pub in_cinemas: Option<String>,
    pub digital_release: Option<String>,
    pub physical_release: Option<String>,
    /// TMDB path such as `/abc.jpg`.
    pub poster_path: Option<String>,
    pub root_folder_path: String,
    pub folder_name: String,
    pub monitored: bool,
    /// `announced`, `inCinemas` or `released`.
    pub minimum_availability: String,
    pub added_at: String,
    pub tags: Vec<i32>,
    /// The primary movie profile's quality profile.
    pub quality_profile_id: i32,
    /// The file of the primary movie profile.
    pub file: Option<FileView>,
}

fn clean_title(title: &str) -> String {
    title
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn join(root: &str, name: &str) -> String {
    let separator = if root.contains('\\') && !root.contains('/') {
        '\\'
    } else {
        '/'
    };
    format!("{}{separator}{name}", root.trim_end_matches(['/', '\\']))
}

fn date_of(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text.get(..10)?, "%Y-%m-%d").ok()
}

/// Radarr's rule: a movie is available when `now` has reached the date of its
/// minimum availability. `released` falls back to 90 days after the cinema
/// release when no home release date is known; without any date, only
/// `announced` is available.
fn is_available(movie: &MovieView, now: &str) -> bool {
    let Some(today) = date_of(now) else {
        return false;
    };
    let reached = |date: &Option<String>| {
        date.as_deref()
            .and_then(date_of)
            .is_some_and(|date| date <= today)
    };
    let home = reached(&movie.digital_release) || reached(&movie.physical_release);
    let cinema = reached(&movie.in_cinemas);
    let late_cinema = movie
        .in_cinemas
        .as_deref()
        .and_then(date_of)
        .and_then(|date| date.checked_add_days(chrono::Days::new(90)))
        .is_some_and(|date| date <= today);
    let any_home_date = movie.digital_release.is_some() || movie.physical_release.is_some();
    match MinimumAvailability::parse(&movie.minimum_availability) {
        Some(MinimumAvailability::InCinemas) => cinema || home,
        Some(MinimumAvailability::Released) => home || (!any_home_date && late_cinema),
        _ => true,
    }
}

fn status_of(text: &str) -> MovieStatusType {
    MovieStatusType::ALL
        .iter()
        .copied()
        .find(|status| status.wire() == text)
        .unwrap_or_default()
}

fn movie_file(movie: &MovieView, file: &FileView) -> MovieFileResource {
    MovieFileResource {
        id: file.id,
        movie_id: movie.id,
        relative_path: file.relative_path.clone(),
        path: join(
            &join(&movie.root_folder_path, &movie.folder_name),
            &file.relative_path,
        ),
        size: file.size,
        date_added: Timestamp::parse(&file.added_at),
        scene_name: file.scene_name.clone().unwrap_or_default(),
        release_group: file.release_group.clone().unwrap_or_default(),
        edition: file.edition.clone().unwrap_or_default(),
        languages: file.languages.iter().map(|code| language(code)).collect(),
        quality: quality_model(file.quality_id),
        media_info: file.media_info.clone().unwrap_or_default(),
        ..MovieFileResource::default()
    }
}

/// A movie as Radarr's `MovieResource`. `now` is an ISO 8601 date or
/// date-time; it decides `isAvailable`.
pub fn movie_resource(movie: &MovieView, now: &str) -> MovieResource {
    let path = join(&movie.root_folder_path, &movie.folder_name);
    let poster = movie
        .poster_path
        .as_deref()
        .filter(|path| !path.is_empty())
        .map(|path| format!("{TMDB_IMAGES}{path}"));
    let file = movie.file.as_ref();
    MovieResource {
        id: movie.id,
        title: movie.title.clone(),
        original_title: movie.original_title.clone().unwrap_or_default(),
        sort_title: movie.sort_title.clone(),
        status: status_of(&movie.status),
        overview: movie.overview.clone().unwrap_or_default(),
        in_cinemas: movie.in_cinemas.as_deref().map(Timestamp::parse),
        digital_release: movie.digital_release.as_deref().map(Timestamp::parse),
        physical_release: movie.physical_release.as_deref().map(Timestamp::parse),
        images: poster
            .iter()
            .map(|url| MediaCover {
                cover_type: MediaCoverTypes::Poster,
                url: url.clone(),
                remote_url: url.clone(),
            })
            .collect(),
        remote_poster: poster.unwrap_or_default(),
        year: movie.year,
        folder: movie.folder_name.clone(),
        path: path.clone(),
        folder_name: path,
        root_folder_path: movie.root_folder_path.clone(),
        quality_profile_id: movie.quality_profile_id,
        has_file: Some(file.is_some()),
        movie_file_id: file.map_or(0, |file| file.id),
        size_on_disk: Some(file.map_or(0, |file| file.size)),
        monitored: movie.monitored,
        minimum_availability: status_of(&movie.minimum_availability),
        is_available: is_available(movie, now),
        runtime: movie.runtime.unwrap_or_default(),
        clean_title: clean_title(&movie.title),
        imdb_id: movie.imdb_id.clone().unwrap_or_default(),
        tmdb_id: movie.tmdb_id,
        title_slug: movie.tmdb_id.to_string(),
        tags: movie.tags.clone(),
        added: Timestamp::parse(&movie.added_at),
        movie_file: file.map(|file| movie_file(movie, file)),
        statistics: MovieStatisticsResource {
            movie_file_count: i32::from(file.is_some()),
            size_on_disk: file.map_or(0, |file| file.size),
            release_groups: file
                .and_then(|file| file.release_group.clone())
                .into_iter()
                .collect(),
            movie_file_qualities: file
                .map(|file| quality(file.quality_id))
                .into_iter()
                .collect(),
        },
        ..MovieResource::default()
    }
}

/// The body of `POST /movie` and `PUT /movie`. A client sends the fields it
/// knows, so everything is optional and anything else is ignored.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MovieRequest {
    pub id: Option<i32>,
    pub tmdb_id: Option<i32>,
    pub quality_profile_id: Option<i32>,
    pub root_folder_path: Option<String>,
    pub monitored: Option<bool>,
    pub minimum_availability: Option<MovieStatusType>,
    pub tags: Option<Vec<i32>>,
    pub add_options: Option<AddMovieOptions>,
}

/// What `POST /movie` asks Unseen to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddMovie {
    pub tmdb_id: i32,
    pub quality_profile_id: i32,
    pub root_folder_path: String,
    pub monitored: bool,
    pub minimum_availability: MinimumAvailability,
    pub tags: Vec<i32>,
    /// Needs the `search:run` permission (docs/14).
    pub search_for_movie: bool,
}

/// What `PUT /movie` asks Unseen to change. `None` keeps the stored value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateMovie {
    pub id: i32,
    pub quality_profile_id: Option<i32>,
    pub monitored: Option<bool>,
    pub minimum_availability: Option<MinimumAvailability>,
    pub tags: Option<Vec<i32>>,
}

impl MovieRequest {
    /// A new movie: it needs a TMDB id, a quality profile and a root folder.
    /// A movie is monitored and `released` by default, as in Unseen's own API.
    pub fn into_add(self) -> Result<AddMovie, MapError> {
        let positive =
            |value: Option<i32>, field| value.filter(|id| *id > 0).ok_or(MapError::Missing(field));
        Ok(AddMovie {
            tmdb_id: positive(self.tmdb_id, "tmdbId")?,
            quality_profile_id: positive(self.quality_profile_id, "qualityProfileId")?,
            root_folder_path: self
                .root_folder_path
                .filter(|path| !path.trim().is_empty())
                .ok_or(MapError::Missing("rootFolderPath"))?,
            monitored: self.monitored.unwrap_or(true),
            minimum_availability: self.minimum_availability.map_or(
                Ok(MinimumAvailability::Released),
                MinimumAvailability::try_from,
            )?,
            tags: self.tags.unwrap_or_default(),
            search_for_movie: self
                .add_options
                .is_some_and(|options| options.search_for_movie),
        })
    }

    /// A change of an existing movie: it needs the Unseen id.
    pub fn into_update(self) -> Result<UpdateMovie, MapError> {
        Ok(UpdateMovie {
            id: self
                .id
                .filter(|id| *id > 0)
                .ok_or(MapError::Missing("id"))?,
            quality_profile_id: self.quality_profile_id.filter(|id| *id > 0),
            monitored: self.monitored,
            minimum_availability: self
                .minimum_availability
                .map(MinimumAvailability::try_from)
                .transpose()?,
            tags: self.tags,
        })
    }
}

// --------------------------------------------------------- quality profiles

/// A quality profile as `quality_profiles` stores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileView {
    pub id: i32,
    pub name: String,
    pub upgrade_allowed: bool,
    pub cutoff_quality_id: i32,
    pub items: Vec<Item>,
    /// `any`, `original` or a language code.
    pub language: String,
    pub min_format_score: i32,
    pub cutoff_format_score: i32,
    pub min_upgrade_format_score: i32,
}

fn quality_item(quality_id: i32, allowed: bool) -> QualityProfileQualityItemResource {
    QualityProfileQualityItemResource {
        quality: Some(quality(quality_id)),
        allowed,
        ..QualityProfileQualityItemResource::default()
    }
}

/// A profile as Radarr's `QualityProfileResource`. Custom formats are not in
/// the compat API before M11, so `formatItems` is empty.
pub fn quality_profile(view: &ProfileView) -> QualityProfileResource {
    QualityProfileResource {
        id: view.id,
        name: view.name.clone(),
        upgrade_allowed: view.upgrade_allowed,
        cutoff: view.cutoff_quality_id,
        items: view
            .items
            .iter()
            .map(|item| match item {
                Item::Quality {
                    quality_id,
                    allowed,
                } => quality_item(*quality_id, *allowed),
                Item::Group {
                    id,
                    name,
                    quality_ids,
                    allowed,
                } => QualityProfileQualityItemResource {
                    id: *id,
                    name: name.clone(),
                    quality: None,
                    items: quality_ids
                        .iter()
                        .map(|quality_id| quality_item(*quality_id, *allowed))
                        .collect(),
                    allowed: *allowed,
                },
            })
            .collect(),
        min_format_score: view.min_format_score,
        cutoff_format_score: view.cutoff_format_score,
        min_upgrade_format_score: view.min_upgrade_format_score,
        format_items: Vec::new(),
        language: language(&view.language),
    }
}

// ------------------------------------------------------------ root folders

/// A root folder with what the API layer found on disk.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RootFolderView {
    pub id: i32,
    pub path: String,
    pub accessible: bool,
    pub free_space: Option<i64>,
    /// Names of the folders without a movie, see `core::root_folder::unmapped`.
    pub unmapped: Vec<String>,
}

pub fn root_folder(view: &RootFolderView) -> RootFolderResource {
    RootFolderResource {
        id: view.id,
        path: view.path.clone(),
        accessible: view.accessible,
        free_space: view.free_space,
        unmapped_folders: view
            .unmapped
            .iter()
            .map(|name| UnmappedFolder {
                name: name.clone(),
                path: join(&view.path, name),
                relative_path: name.clone(),
            })
            .collect(),
    }
}

// -------------------------------------------------------------------- tags

pub fn tag(id: i32, label: &str) -> TagResource {
    TagResource {
        id,
        label: label.to_owned(),
    }
}

/// The label of a tag that a client sends.
pub fn tag_label(resource: &TagResource) -> Result<String, MapError> {
    let label = resource.label.trim();
    if label.is_empty() {
        Err(MapError::Missing("label"))
    } else {
        Ok(label.to_owned())
    }
}

// ------------------------------------------------------------------- queue

/// One download of a movie.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct QueueView {
    pub id: i32,
    pub movie: Option<MovieView>,
    pub quality_id: i32,
    pub title: String,
    /// Bytes.
    pub size: f64,
    pub size_left: f64,
    pub time_left_seconds: Option<u64>,
    pub estimated_completion: Option<String>,
    pub added: Option<String>,
    pub status: QueueStatus,
    pub tracked_status: TrackedDownloadStatus,
    pub tracked_state: TrackedDownloadState,
    /// Title and messages.
    pub status_messages: Vec<(String, Vec<String>)>,
    pub error_message: Option<String>,
    pub download_id: Option<String>,
    pub protocol: DownloadProtocol,
    pub download_client: Option<String>,
    pub indexer: Option<String>,
    pub output_path: Option<String>,
}

/// A .NET `TimeSpan`: `[d.]hh:mm:ss`.
fn time_span(seconds: u64) -> String {
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    let clock = format!(
        "{:02}:{:02}:{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    );
    if days > 0 {
        format!("{days}.{clock}")
    } else {
        clock
    }
}

pub fn queue_item(view: &QueueView, now: &str) -> QueueResource {
    let text = |value: &Option<String>| value.clone().unwrap_or_default();
    QueueResource {
        id: view.id,
        movie_id: view.movie.as_ref().map(|movie| movie.id),
        movie: view.movie.as_ref().map(|movie| movie_resource(movie, now)),
        quality: quality_model(view.quality_id),
        size: view.size,
        title: view.title.clone(),
        estimated_completion_time: view.estimated_completion.as_deref().map(Timestamp::parse),
        added: view.added.as_deref().map(Timestamp::parse),
        status: view.status,
        tracked_download_status: view.tracked_status,
        tracked_download_state: view.tracked_state,
        status_messages: view
            .status_messages
            .iter()
            .map(|(title, messages)| TrackedDownloadStatusMessage {
                title: title.clone(),
                messages: messages.clone(),
            })
            .collect(),
        error_message: text(&view.error_message),
        download_id: text(&view.download_id),
        protocol: view.protocol,
        download_client: text(&view.download_client),
        indexer: text(&view.indexer),
        output_path: text(&view.output_path),
        sizeleft: view.size_left,
        timeleft: view.time_left_seconds.map(time_span),
        ..QueueResource::default()
    }
}

/// One page of `records`; `total` counts all records, not only this page.
pub fn page<T>(
    records: Vec<T>,
    page: i32,
    page_size: i32,
    total: i32,
    sort_key: &str,
    sort_direction: SortDirection,
) -> Paging<T> {
    Paging {
        page,
        page_size,
        sort_key: sort_key.to_owned(),
        sort_direction,
        total_records: total,
        records,
    }
}

// ---------------------------------------------------------------- commands

/// The body of `POST /command`. `movieIds` is not in the upstream `Command`
/// schema, so it is read here and never written back.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CommandRequest {
    pub name: String,
    pub movie_ids: Vec<i32>,
}

/// The commands Unseen runs; any other name is answered with 501 (docs/14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandKind {
    MoviesSearch { movie_ids: Vec<i32> },
    RefreshMonitoredDownloads,
    Unsupported(String),
}

impl CommandRequest {
    /// Command names compare without regard to case, as Radarr does.
    pub fn kind(self) -> CommandKind {
        match self.name.to_ascii_lowercase().as_str() {
            "moviessearch" => CommandKind::MoviesSearch {
                movie_ids: self.movie_ids,
            },
            "refreshmonitoreddownloads" => CommandKind::RefreshMonitoredDownloads,
            _ => CommandKind::Unsupported(self.name),
        }
    }
}

/// A command that was queued or has run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommandView {
    pub id: i32,
    /// Radarr's command name, such as `MoviesSearch`.
    pub name: String,
    pub status: CommandStatus,
    pub queued: String,
    pub started: Option<String>,
    pub ended: Option<String>,
}

/// `MoviesSearch` becomes `Movies Search`.
fn humanize(name: &str) -> String {
    let mut text = String::with_capacity(name.len() + 2);
    for (index, c) in name.chars().enumerate() {
        if index > 0 && c.is_uppercase() {
            text.push(' ');
        }
        text.push(c);
    }
    text
}

pub fn command(view: &CommandView) -> CommandResource {
    let result = match view.status {
        CommandStatus::Completed => CommandResult::Successful,
        CommandStatus::Failed | CommandStatus::Aborted | CommandStatus::Cancelled => {
            CommandResult::Unsuccessful
        }
        _ => CommandResult::Unknown,
    };
    let state_change = view
        .ended
        .as_ref()
        .or(view.started.as_ref())
        .unwrap_or(&view.queued);
    CommandResource {
        id: view.id,
        name: view.name.clone(),
        command_name: humanize(&view.name),
        body: Command {
            send_updates_to_client: true,
            name: view.name.clone(),
            trigger: CommandTrigger::Manual,
            ..Command::default()
        },
        status: view.status,
        result,
        queued: Timestamp::parse(&view.queued),
        started: view.started.as_deref().map(Timestamp::parse),
        ended: view.ended.as_deref().map(Timestamp::parse),
        trigger: CommandTrigger::Manual,
        state_change_time: Some(Timestamp::parse(state_change)),
        send_updates_to_client: true,
        ..CommandResource::default()
    }
}

// ------------------------------------------------------------------ system

/// What `GET /system/status` reports about this process.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SystemView {
    pub start_time: String,
    pub startup_path: String,
    pub app_data: String,
    pub os_name: String,
    pub os_version: String,
}

/// The status of an Unseen that poses as the pinned Radarr release.
pub fn system(view: &SystemView) -> SystemResource {
    SystemResource {
        start_time: Timestamp::parse(&view.start_time),
        startup_path: view.startup_path.clone(),
        app_data: view.app_data.clone(),
        os_name: view.os_name.clone(),
        os_version: view.os_version.clone(),
        ..SystemResource::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> MovieView {
        MovieView {
            id: 7,
            tmdb_id: 603,
            imdb_id: Some("tt0133093".to_owned()),
            title: "The Matrix: Reloaded".to_owned(),
            sort_title: "matrix: reloaded".to_owned(),
            year: 1999,
            runtime: Some(136),
            status: "released".to_owned(),
            in_cinemas: Some("1999-03-30".to_owned()),
            digital_release: Some("2004-09-21".to_owned()),
            root_folder_path: "/media/movies".to_owned(),
            folder_name: "The Matrix Reloaded (1999)".to_owned(),
            monitored: true,
            minimum_availability: "inCinemas".to_owned(),
            added_at: "2026-11-16T20:15:13.123Z".to_owned(),
            tags: vec![2, 5],
            quality_profile_id: 4,
            ..MovieView::default()
        }
    }

    /// The movie as a client reads it from us, then sends it back.
    fn sent_back(view: &MovieView) -> MovieRequest {
        let json = serde_json::to_string(&movie_resource(view, "2026-11-25")).unwrap();
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn the_fields_follow_the_mapping_table() {
        let resource = movie_resource(&view(), "2026-11-25");
        assert_eq!(resource.title_slug, "603");
        assert_eq!(resource.path, "/media/movies/The Matrix Reloaded (1999)");
        assert_eq!(resource.folder_name, resource.path);
        assert_eq!(resource.root_folder_path, "/media/movies");
        assert_eq!(resource.clean_title, "thematrixreloaded");
        assert_eq!(resource.has_file, Some(false));
        assert!(resource.movie_file.is_none());
        assert_eq!(resource.status, MovieStatusType::Released);
        assert_eq!(resource.minimum_availability, MovieStatusType::InCinemas);
        assert_eq!(
            resource.in_cinemas,
            Some(Timestamp("1999-03-30T00:00:00Z".to_owned()))
        );
        assert_eq!(resource.release_date, None);
        assert_eq!(resource.tags, [2, 5]);
        assert_eq!(resource.runtime, 136);
    }

    #[test]
    fn a_file_adds_its_resource_and_statistics() {
        let mut with_file = view();
        with_file.file = Some(FileView {
            id: 11,
            relative_path: "movie.mkv".to_owned(),
            size: 5_000,
            quality_id: CoreQuality::Bluray1080p.id(),
            release_group: Some("GRP".to_owned()),
            languages: vec!["en".to_owned()],
            ..FileView::default()
        });
        let resource = movie_resource(&with_file, "2026-11-25");
        let file = resource.movie_file.expect("a file");
        assert_eq!(resource.has_file, Some(true));
        assert_eq!(resource.movie_file_id, 11);
        assert_eq!(resource.size_on_disk, Some(5_000));
        assert_eq!(file.movie_id, 7);
        assert_eq!(
            file.path,
            "/media/movies/The Matrix Reloaded (1999)/movie.mkv"
        );
        assert_eq!(file.quality.quality.name, "Bluray-1080p");
        assert_eq!(file.languages[0].id, 1);
        assert_eq!(resource.statistics.movie_file_count, 1);
        assert_eq!(resource.statistics.release_groups, ["GRP"]);
    }

    #[test]
    fn availability_follows_the_minimum_availability() {
        let at = |availability: &str, digital: Option<&str>, now: &str| {
            let mut movie = view();
            movie.minimum_availability = availability.to_owned();
            movie.in_cinemas = Some("2026-06-01".to_owned());
            movie.digital_release = digital.map(str::to_owned);
            movie_resource(&movie, now).is_available
        };
        assert!(at("announced", None, "2020-01-01"));
        assert!(!at("inCinemas", None, "2026-05-31"));
        assert!(at("inCinemas", None, "2026-06-01"));
        assert!(!at("released", Some("2026-09-01"), "2026-08-31"));
        assert!(at("released", Some("2026-09-01"), "2026-09-01"));
        // No home release date: 90 days after the cinema release.
        assert!(!at("released", None, "2026-08-29"));
        assert!(at("released", None, "2026-08-30"));
    }

    #[test]
    fn adding_a_movie_survives_the_round_trip() {
        let mut movie = view();
        movie.minimum_availability = "released".to_owned();
        let add = sent_back(&movie).into_add().unwrap();
        assert_eq!(
            add,
            AddMovie {
                tmdb_id: 603,
                quality_profile_id: 4,
                root_folder_path: "/media/movies".to_owned(),
                monitored: true,
                minimum_availability: MinimumAvailability::Released,
                tags: vec![2, 5],
                search_for_movie: false,
            }
        );
    }

    #[test]
    fn updating_a_movie_survives_the_round_trip() {
        let mut movie = view();
        movie.monitored = false;
        let update = sent_back(&movie).into_update().unwrap();
        assert_eq!(
            update,
            UpdateMovie {
                id: 7,
                quality_profile_id: Some(4),
                monitored: Some(false),
                minimum_availability: Some(MinimumAvailability::InCinemas),
                tags: Some(vec![2, 5]),
            }
        );
    }

    #[test]
    fn a_seerr_add_body_is_read() {
        let body = r#"{
            "title": "Dune", "qualityProfileId": 4, "profileId": 4,
            "minimumAvailability": "announced", "tags": [1],
            "year": 2021, "rootFolderPath": "/movies", "tmdbId": 438631,
            "monitored": true, "addOptions": {"searchForMovie": true},
            "unknownField": {"a": 1}
        }"#;
        let add = serde_json::from_str::<MovieRequest>(body)
            .unwrap()
            .into_add()
            .unwrap();
        assert_eq!(add.tmdb_id, 438_631);
        assert_eq!(add.minimum_availability, MinimumAvailability::Announced);
        assert!(add.search_for_movie);
        assert_eq!(add.tags, [1]);
    }

    fn parse(body: &str) -> MovieRequest {
        serde_json::from_str(body).unwrap()
    }

    #[test]
    fn an_add_needs_its_ids_and_a_root_folder() {
        assert_eq!(parse("{}").into_add(), Err(MapError::Missing("tmdbId")));
        assert_eq!(
            parse(r#"{"tmdbId": 1}"#).into_add(),
            Err(MapError::Missing("qualityProfileId"))
        );
        assert_eq!(
            parse(r#"{"tmdbId": 1, "qualityProfileId": 2, "rootFolderPath": " "}"#).into_add(),
            Err(MapError::Missing("rootFolderPath"))
        );
        // Defaults: monitored, `released`, no tags.
        let add = parse(r#"{"tmdbId": 1, "qualityProfileId": 2, "rootFolderPath": "/m"}"#)
            .into_add()
            .unwrap();
        assert!(add.monitored && add.tags.is_empty());
        assert_eq!(add.minimum_availability, MinimumAvailability::Released);
    }

    #[test]
    fn tba_is_announced_and_deleted_is_refused() {
        let with = |availability: &str| {
            parse(&format!(
                r#"{{"tmdbId":1,"qualityProfileId":2,"rootFolderPath":"/m","minimumAvailability":"{availability}"}}"#
            ))
            .into_add()
        };
        assert_eq!(
            with("tba").unwrap().minimum_availability,
            MinimumAvailability::Announced
        );
        assert!(matches!(
            with("deleted"),
            Err(MapError::Invalid("minimumAvailability", _))
        ));
    }

    #[test]
    fn an_update_changes_only_what_it_carries() {
        let update = parse(r#"{"id": 3, "monitored": true}"#)
            .into_update()
            .unwrap();
        assert_eq!(
            update,
            UpdateMovie {
                id: 3,
                quality_profile_id: None,
                monitored: Some(true),
                minimum_availability: None,
                tags: None,
            }
        );
        assert_eq!(parse("{}").into_update(), Err(MapError::Missing("id")));
    }

    #[test]
    fn languages_use_radarr_ids() {
        assert_eq!(
            language("any"),
            Language {
                id: -1,
                name: "Any".to_owned()
            }
        );
        assert_eq!(language("original").id, -2);
        assert_eq!(language("EN").name, "English");
        assert_eq!(language("ja").id, 8);
        assert_eq!(language("xyz").id, 0);
    }

    #[test]
    fn a_group_has_no_quality_and_holds_its_members() {
        let profile = quality_profile(&ProfileView {
            id: 1,
            name: "p".to_owned(),
            upgrade_allowed: true,
            cutoff_quality_id: 1000,
            items: vec![Item::Group {
                id: 1000,
                name: "WEB 1080p".to_owned(),
                quality_ids: vec![CoreQuality::WebDl1080p.id(), CoreQuality::WebRip1080p.id()],
                allowed: true,
            }],
            language: "original".to_owned(),
            min_format_score: 0,
            cutoff_format_score: 0,
            min_upgrade_format_score: 1,
        });
        let group = &profile.items[0];
        assert_eq!((group.id, group.quality.is_none()), (1000, true));
        assert_eq!(
            group.items[1].quality.as_ref().unwrap().name,
            "WEBRip-1080p"
        );
        assert_eq!(profile.cutoff, 1000);
        assert_eq!(profile.language.id, -2);
    }

    #[test]
    fn commands_are_told_apart_by_name() {
        let kind = |body: &str| serde_json::from_str::<CommandRequest>(body).unwrap().kind();
        assert_eq!(
            kind(r#"{"name": "MoviesSearch", "movieIds": [3, 4]}"#),
            CommandKind::MoviesSearch {
                movie_ids: vec![3, 4]
            }
        );
        assert_eq!(
            kind(r#"{"name": "refreshmonitoreddownloads"}"#),
            CommandKind::RefreshMonitoredDownloads
        );
        assert_eq!(
            kind(r#"{"name": "RescanMovie"}"#),
            CommandKind::Unsupported("RescanMovie".to_owned())
        );
    }

    #[test]
    fn tags_and_root_folders_map_both_ways() {
        assert_eq!(
            tag(3, "4k"),
            TagResource {
                id: 3,
                label: "4k".to_owned()
            }
        );
        assert_eq!(tag_label(&tag(0, "  4k ")).unwrap(), "4k");
        assert_eq!(tag_label(&tag(0, " ")), Err(MapError::Missing("label")));
        let folder = root_folder(&RootFolderView {
            id: 1,
            path: r"D:\Movies\".to_owned(),
            unmapped: vec!["Stray".to_owned()],
            ..RootFolderView::default()
        });
        assert_eq!(folder.unmapped_folders[0].path, r"D:\Movies\Stray");
        assert_eq!(folder.unmapped_folders[0].relative_path, "Stray");
    }

    #[test]
    fn a_time_span_has_days_when_it_needs_them() {
        assert_eq!(time_span(59), "00:00:59");
        assert_eq!(time_span(3_661), "01:01:01");
        assert_eq!(time_span(93_784), "1.02:03:04");
    }
}
