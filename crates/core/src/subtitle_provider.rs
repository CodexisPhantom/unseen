//! The subtitle provider trait (docs/08-integrations.md). The rest of the
//! code depends on this trait only; OpenSubtitles is the one implementation
//! in V1. Scoring of candidates is not here: it lives in `core` so that a
//! second provider needs no new scoring code.

use std::fmt;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// What a subtitle is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubtitleKind {
    Normal,
    /// Hearing impaired (SDH).
    HearingImpaired,
    /// Foreign parts only.
    Forced,
}

/// What to look for, for one movie.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubtitleQuery {
    pub tmdb_id: Option<i32>,
    /// With or without the `tt` prefix.
    pub imdb_id: Option<String>,
    pub title: String,
    pub year: Option<i32>,
    /// Language codes as stored: `en`, `fr`, `pt-BR`. Empty means any.
    pub languages: Vec<String>,
    /// Empty means any kind.
    pub kinds: Vec<SubtitleKind>,
    /// The 64-bit OpenSubtitles hash of the video file: 16 hex digits.
    pub hash: Option<String>,
    pub file_name: Option<String>,
}

/// One subtitle that a provider offers.
#[derive(Debug, Clone, PartialEq)]
pub struct SubtitleCandidate {
    /// The provider's id, to give to [`SubtitleProvider::download`].
    pub id: String,
    pub language: String,
    pub kind: SubtitleKind,
    pub release: String,
    pub hash_match: bool,
    pub trusted: bool,
    pub download_count: u64,
    pub rating: f64,
    /// Machine or AI translated.
    pub machine_translated: bool,
}

/// The downloads a provider still allows, as its last answer said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quota {
    pub remaining: u32,
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadedSubtitle {
    pub bytes: Vec<u8>,
    /// Without the dot: `srt`, `ass`.
    pub extension: String,
    pub quota: Option<Quota>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubtitleError {
    NotFound,
    /// The provider refused the API key or the login.
    Unauthorized,
    /// No download is left. The provider is not called again before
    /// `resets_at`.
    QuotaExhausted {
        resets_at: DateTime<Utc>,
    },
    /// Still rate limited after the retries.
    RateLimited,
    /// No answer, or an answer that is not from the provider.
    Unavailable(String),
}

impl fmt::Display for SubtitleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("the subtitle provider does not know this subtitle"),
            Self::Unauthorized => {
                f.write_str("the subtitle provider refused the API key or the login")
            }
            Self::QuotaExhausted { resets_at } => write!(
                f,
                "the download quota is used up until {}",
                resets_at.format("%Y-%m-%d %H:%M UTC")
            ),
            Self::RateLimited => f.write_str("the subtitle provider is rate limiting"),
            Self::Unavailable(reason) => {
                write!(f, "the subtitle provider is not available: {reason}")
            }
        }
    }
}

impl std::error::Error for SubtitleError {}

#[async_trait]
pub trait SubtitleProvider: Send + Sync {
    /// Candidates for one movie, best provider order first. No quota used.
    async fn search(&self, query: &SubtitleQuery) -> Result<Vec<SubtitleCandidate>, SubtitleError>;

    /// The subtitle file of one candidate. Uses one unit of quota.
    async fn download(&self, id: &str) -> Result<DownloadedSubtitle, SubtitleError>;
}
