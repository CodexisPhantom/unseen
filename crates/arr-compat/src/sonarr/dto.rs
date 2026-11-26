//! Sonarr v3 resources, field for field as the pinned OpenAPI document
//! (`fixtures/sonarr/openapi-<version>.json`) lists them. Every field is
//! serialised, with a default when Unseen has no value (docs/14).
//!
//! The exceptions are objects that stand for "nothing here": `episodeFile` of
//! an episode without a file, `series` on an episode listing that does not
//! embed the series, `series` and `episode` of a queue record that has none,
//! and `quality` of a quality group. Sonarr leaves them out; Unseen sends
//! `null` so that the key is present. The schema test lists them.

use serde::{Deserialize, Serialize};

/// A string enum with the wire names of the upstream schema. The first
/// variant is the default.
macro_rules! wire_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $wire:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
        pub enum $name {
            #[default]
            $(#[serde(rename = $wire)] $variant),+
        }

        impl $name {
            /// Every variant, in schema order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The name on the wire.
            pub const fn wire(self) -> &'static str {
                match self { $(Self::$variant => $wire),+ }
            }
        }
    };
}

wire_enum!(SeriesStatusType {
    Continuing = "continuing",
    Ended = "ended",
    Upcoming = "upcoming",
    Deleted = "deleted",
});
wire_enum!(SeriesTypes {
    Standard = "standard",
    Daily = "daily",
    Anime = "anime",
});
wire_enum!(NewItemMonitorTypes {
    All = "all",
    None = "none",
});
wire_enum!(MediaCoverTypes {
    Unknown = "unknown",
    Poster = "poster",
    Banner = "banner",
    Fanart = "fanart",
    Screenshot = "screenshot",
    Headshot = "headshot",
    Clearlogo = "clearlogo",
});
wire_enum!(MonitorTypes {
    Unknown = "unknown",
    All = "all",
    Future = "future",
    Missing = "missing",
    Existing = "existing",
    FirstSeason = "firstSeason",
    LastSeason = "lastSeason",
    LatestSeason = "latestSeason",
    Pilot = "pilot",
    Recent = "recent",
    MonitorSpecials = "monitorSpecials",
    UnmonitorSpecials = "unmonitorSpecials",
    None = "none",
    Skip = "skip",
});
wire_enum!(QualitySource {
    Unknown = "unknown",
    Television = "television",
    TelevisionRaw = "televisionRaw",
    Web = "web",
    WebRip = "webRip",
    Dvd = "dvd",
    Bluray = "bluray",
    BlurayRaw = "blurayRaw",
});
wire_enum!(ReleaseType {
    Unknown = "unknown",
    SingleEpisode = "singleEpisode",
    MultiEpisode = "multiEpisode",
    SeasonPack = "seasonPack",
    MultiSeasonPack = "multiSeasonPack",
});
wire_enum!(QueueStatus {
    Unknown = "unknown",
    Queued = "queued",
    Paused = "paused",
    Downloading = "downloading",
    Completed = "completed",
    Failed = "failed",
    Warning = "warning",
    Delay = "delay",
    DownloadClientUnavailable = "downloadClientUnavailable",
    Fallback = "fallback",
});
wire_enum!(TrackedDownloadStatus {
    Ok = "ok",
    Warning = "warning",
    Error = "error",
});
wire_enum!(TrackedDownloadState {
    Downloading = "downloading",
    ImportBlocked = "importBlocked",
    ImportPending = "importPending",
    Importing = "importing",
    Imported = "imported",
    FailedPending = "failedPending",
    Failed = "failed",
    Ignored = "ignored",
});
wire_enum!(DownloadProtocol {
    Unknown = "unknown",
    Usenet = "usenet",
    Torrent = "torrent",
});
wire_enum!(SortDirection {
    Default = "default",
    Ascending = "ascending",
    Descending = "descending",
});
wire_enum!(CommandPriority {
    Normal = "normal",
    High = "high",
    Low = "low",
});
wire_enum!(CommandStatus {
    Queued = "queued",
    Started = "started",
    Completed = "completed",
    Failed = "failed",
    Aborted = "aborted",
    Cancelled = "cancelled",
    Orphaned = "orphaned",
});
wire_enum!(CommandResult {
    Unknown = "unknown",
    Successful = "successful",
    Unsuccessful = "unsuccessful",
    Indeterminate = "indeterminate",
});
wire_enum!(CommandTrigger {
    Unspecified = "unspecified",
    Manual = "manual",
    Scheduled = "scheduled",
});
wire_enum!(RuntimeMode {
    Console = "console",
    Service = "service",
    Tray = "tray",
});
wire_enum!(DatabaseType {
    SqLite = "sqLite",
    PostgreSql = "postgreSQL",
});
wire_enum!(AuthenticationType {
    None = "none",
    Basic = "basic",
    Forms = "forms",
    External = "external",
    Oidc = "oidc",
});
wire_enum!(UpdateMechanism {
    BuiltIn = "builtIn",
    Script = "script",
    External = "external",
    Apt = "apt",
    Docker = "docker",
});

/// An ISO 8601 date-time. Sonarr's value for "no date" in a field that
/// cannot be null is the .NET minimum, which is also the default here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(pub String);

impl Timestamp {
    /// Accepts a date (`2021-09-15`) or a date-time. A date becomes midnight
    /// UTC.
    pub fn parse(text: &str) -> Self {
        let text = text.trim();
        if text.len() == 10 {
            Self(format!("{text}T00:00:00Z"))
        } else {
            Self(text.to_owned())
        }
    }
}

impl Default for Timestamp {
    fn default() -> Self {
        Self("0001-01-01T00:00:00Z".to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Language {
    pub id: i32,
    pub name: String,
}

impl Default for Language {
    fn default() -> Self {
        Self {
            id: 0,
            name: "Unknown".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Quality {
    pub id: i32,
    pub name: String,
    pub source: QualitySource,
    pub resolution: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Revision {
    pub version: i32,
    pub real: i32,
    pub is_repack: bool,
}

impl Default for Revision {
    fn default() -> Self {
        Self {
            version: 1,
            real: 0,
            is_repack: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct QualityModel {
    pub quality: Quality,
    pub revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AlternateTitleResource {
    pub title: String,
    pub season_number: Option<i32>,
    pub scene_season_number: Option<i32>,
    pub scene_origin: String,
    pub comment: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MediaCover {
    pub cover_type: MediaCoverTypes,
    pub url: String,
    pub remote_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SeasonTypeResource {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub season_numbers: Vec<i32>,
    pub episode_count: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SeriesTranslationResource {
    pub language: Language,
    pub title: String,
    pub overview: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SeriesFolderResource {
    pub language: Language,
    pub folder: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AddSeriesOptions {
    pub ignore_episodes_with_files: bool,
    pub ignore_episodes_without_files: bool,
    pub monitor: MonitorTypes,
    pub search_for_missing_episodes: bool,
    pub search_for_cutoff_unmet_episodes: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Ratings {
    pub votes: i32,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SeasonStatisticsResource {
    pub next_airing: Option<Timestamp>,
    pub previous_airing: Option<Timestamp>,
    pub episode_file_count: i32,
    pub episode_count: i32,
    pub total_episode_count: i32,
    pub size_on_disk: i64,
    pub release_groups: Vec<String>,
    pub percent_of_episodes: f64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SeasonResource {
    pub season_number: i32,
    pub monitored: bool,
    pub title: String,
    pub statistics: SeasonStatisticsResource,
    pub images: Vec<MediaCover>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SeriesStatisticsResource {
    pub season_count: i32,
    pub episode_file_count: i32,
    pub episode_count: i32,
    pub total_episode_count: i32,
    pub size_on_disk: i64,
    pub release_groups: Vec<String>,
    pub percent_of_episodes: f64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SeriesResource {
    pub id: i32,
    pub title: String,
    pub alternate_titles: Vec<AlternateTitleResource>,
    pub sort_title: String,
    pub status: SeriesStatusType,
    pub ended: bool,
    pub profile_name: String,
    pub overview: String,
    pub next_airing: Option<Timestamp>,
    pub previous_airing: Option<Timestamp>,
    pub network: String,
    pub air_time: String,
    pub images: Vec<MediaCover>,
    pub language: Language,
    pub original_language: Language,
    pub remote_poster: String,
    pub season_type: String,
    pub seasons: Vec<SeasonResource>,
    pub season_types: Vec<SeasonTypeResource>,
    pub translations: Vec<SeriesTranslationResource>,
    pub year: i32,
    pub path: String,
    pub pending_path: String,
    pub quality_profile_id: i32,
    pub season_folder: bool,
    pub monitored: bool,
    pub monitor_new_items: NewItemMonitorTypes,
    pub use_scene_numbering: bool,
    pub runtime: i32,
    pub tvdb_id: i32,
    pub tv_rage_id: i32,
    pub tv_maze_id: i32,
    pub tmdb_id: i32,
    pub first_aired: Option<Timestamp>,
    pub last_aired: Option<Timestamp>,
    pub series_type: SeriesTypes,
    pub clean_title: String,
    pub original_title: String,
    pub clean_original_title: String,
    pub imdb_id: String,
    pub title_slug: String,
    pub root_folder_path: String,
    pub folder: String,
    pub folders: Vec<SeriesFolderResource>,
    pub certification: String,
    pub genres: Vec<String>,
    pub tags: Vec<i32>,
    pub added: Timestamp,
    pub add_options: AddSeriesOptions,
    pub ratings: Ratings,
    pub statistics: SeriesStatisticsResource,
    pub episodes_changed: Option<bool>,
    pub language_profile_id: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CustomFormatResource {
    pub id: i32,
    pub name: String,
    pub include_custom_format_when_renaming: Option<bool>,
    pub specifications: Vec<CustomFormatSpecificationSchema>,
}

/// Unseen has no custom formats in the compat API until M11, so this is
/// always an empty list; the shape is only here so the type is complete.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CustomFormatSpecificationSchema {
    pub id: i32,
    pub name: String,
    pub implementation: String,
    pub implementation_name: String,
    pub info_link: String,
    pub negate: bool,
    pub required: bool,
    pub fields: Vec<serde_json::Value>,
    pub presets: Vec<CustomFormatSpecificationSchema>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MediaInfoResource {
    pub id: i32,
    pub audio_bitrate: i64,
    pub audio_channels: f64,
    pub audio_codec: String,
    pub audio_languages: String,
    pub audio_stream_count: i32,
    pub video_bit_depth: i32,
    pub video_bitrate: i64,
    pub video_codec: String,
    pub video_fps: f64,
    pub video_dynamic_range: String,
    pub video_dynamic_range_type: String,
    pub resolution: String,
    pub run_time: String,
    pub scan_type: String,
    pub subtitles: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EpisodeFileResource {
    pub id: i32,
    pub series_id: i32,
    pub season_number: i32,
    pub relative_path: String,
    pub path: String,
    pub size: i64,
    pub date_added: Timestamp,
    pub scene_name: String,
    pub release_group: String,
    pub languages: Vec<Language>,
    pub quality: QualityModel,
    pub custom_formats: Vec<CustomFormatResource>,
    pub custom_format_score: i32,
    pub indexer_flags: Option<i32>,
    pub release_type: ReleaseType,
    pub media_info: MediaInfoResource,
    pub quality_cutoff_not_met: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EpisodeResource {
    pub id: i32,
    pub series_id: i32,
    pub tvdb_id: i32,
    pub episode_file_id: i32,
    pub season_number: i32,
    pub episode_number: i32,
    pub title: String,
    pub air_date: Option<String>,
    pub air_date_utc: Option<Timestamp>,
    pub last_search_time: Option<Timestamp>,
    pub runtime: i32,
    pub finale_type: String,
    pub overview: String,
    pub episode_file: Option<EpisodeFileResource>,
    pub has_file: bool,
    pub monitored: bool,
    pub absolute_episode_number: Option<i32>,
    pub scene_absolute_episode_number: Option<i32>,
    pub scene_episode_number: Option<i32>,
    pub scene_season_number: Option<i32>,
    pub unverified_scene_numbering: bool,
    pub series: Option<SeriesResource>,
    pub images: Vec<MediaCover>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EpisodesMonitoredResource {
    pub episode_ids: Vec<i32>,
    pub monitored: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProfileFormatItemResource {
    pub id: i32,
    pub format: i32,
    pub name: String,
    pub score: i32,
}

/// One entry of a profile: a quality, or a group of qualities (then `quality`
/// is `null`, `id` is the group id and `items` holds the members).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct QualityProfileQualityItemResource {
    pub id: i32,
    pub name: String,
    pub quality: Option<Quality>,
    pub items: Vec<QualityProfileQualityItemResource>,
    pub allowed: bool,
    pub min_size: Option<f64>,
    pub max_size: Option<f64>,
    pub preferred_size: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct QualityProfileResource {
    pub id: i32,
    pub name: String,
    pub upgrade_allowed: bool,
    pub cutoff: i32,
    pub items: Vec<QualityProfileQualityItemResource>,
    pub min_format_score: i32,
    pub cutoff_format_score: i32,
    pub min_upgrade_format_score: i32,
    pub format_items: Vec<ProfileFormatItemResource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LanguageProfileItemResource {
    pub id: i32,
    pub language: Language,
    pub allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LanguageProfileResource {
    pub id: i32,
    pub name: String,
    pub upgrade_allowed: bool,
    pub cutoff: Language,
    pub languages: Vec<LanguageProfileItemResource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UnmappedFolder {
    pub name: String,
    pub path: String,
    pub relative_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RootFolderResource {
    pub id: i32,
    pub path: String,
    pub accessible: bool,
    pub free_space: Option<i64>,
    pub total_space: Option<i64>,
    pub unmapped_folders: Vec<UnmappedFolder>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TagResource {
    pub id: i32,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TrackedDownloadStatusMessage {
    pub title: String,
    pub messages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct QueueResource {
    pub id: i32,
    pub series_id: Option<i32>,
    pub episode_id: Option<i32>,
    pub season_number: Option<i32>,
    pub series: Option<SeriesResource>,
    pub episode: Option<EpisodeResource>,
    pub languages: Vec<Language>,
    pub quality: QualityModel,
    pub custom_formats: Vec<CustomFormatResource>,
    pub custom_format_score: i32,
    pub size: f64,
    pub title: String,
    pub estimated_completion_time: Option<Timestamp>,
    pub added: Option<Timestamp>,
    pub status: QueueStatus,
    pub tracked_download_status: TrackedDownloadStatus,
    pub tracked_download_state: TrackedDownloadState,
    pub status_messages: Vec<TrackedDownloadStatusMessage>,
    pub error_message: String,
    pub download_id: String,
    pub protocol: DownloadProtocol,
    pub download_client: String,
    pub download_client_has_post_import_category: bool,
    pub indexer: String,
    pub output_path: String,
    pub episode_has_file: bool,
    pub sizeleft: f64,
    /// A .NET time span, `hh:mm:ss`, absent while unknown.
    pub timeleft: Option<String>,
}

/// A page of records. Sonarr has one schema per record type, all with these
/// fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Paging<T> {
    pub page: i32,
    pub page_size: i32,
    pub sort_key: String,
    pub sort_direction: SortDirection,
    pub total_records: i32,
    pub records: Vec<T>,
}

impl<T> Default for Paging<T> {
    fn default() -> Self {
        Self {
            page: 1,
            page_size: 10,
            sort_key: String::new(),
            sort_direction: SortDirection::default(),
            total_records: 0,
            records: Vec::new(),
        }
    }
}

pub type QueueResourcePagingResource = Paging<QueueResource>;
pub type EpisodeResourcePagingResource = Paging<EpisodeResource>;

/// The `body` of a command resource: the fields of Sonarr's base `Command`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Command {
    pub send_updates_to_client: bool,
    pub update_scheduled_task: bool,
    pub completion_message: String,
    pub requires_disk_access: bool,
    pub is_exclusive: bool,
    pub is_long_running: bool,
    pub name: String,
    pub last_execution_time: Option<Timestamp>,
    pub last_start_time: Option<Timestamp>,
    pub trigger: CommandTrigger,
    pub suppress_messages: bool,
    pub client_user_agent: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CommandResource {
    pub id: i32,
    pub name: String,
    pub command_name: String,
    pub message: String,
    pub body: Command,
    pub priority: CommandPriority,
    pub status: CommandStatus,
    pub result: CommandResult,
    pub queued: Timestamp,
    pub started: Option<Timestamp>,
    pub ended: Option<Timestamp>,
    /// A .NET time span, `hh:mm:ss`, absent until the command ended.
    pub duration: Option<String>,
    pub exception: String,
    pub trigger: CommandTrigger,
    pub client_user_agent: String,
    pub state_change_time: Option<Timestamp>,
    pub send_updates_to_client: bool,
    pub update_scheduled_task: bool,
    pub last_execution_time: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SystemResource {
    pub app_name: String,
    pub instance_name: String,
    pub version: String,
    pub build_time: Timestamp,
    pub is_debug: bool,
    pub is_production: bool,
    pub is_admin: bool,
    pub is_user_interactive: bool,
    pub startup_path: String,
    pub app_data: String,
    pub os_name: String,
    pub os_version: String,
    pub is_net_core: bool,
    pub is_linux: bool,
    pub is_osx: bool,
    pub is_windows: bool,
    pub is_docker: bool,
    pub is_containerized: bool,
    pub mode: RuntimeMode,
    pub branch: String,
    pub authentication: AuthenticationType,
    pub sqlite_version: String,
    pub migration_version: i32,
    pub url_base: String,
    pub runtime_version: String,
    pub runtime_name: String,
    pub start_time: Timestamp,
    pub package_version: String,
    pub package_author: String,
    pub package_update_mechanism: UpdateMechanism,
    pub package_update_mechanism_message: String,
    pub database_version: String,
    pub database_type: DatabaseType,
}

impl Default for SystemResource {
    fn default() -> Self {
        Self {
            app_name: "Sonarr".to_owned(),
            instance_name: "Sonarr".to_owned(),
            version: super::SONARR_VERSION.to_owned(),
            build_time: Timestamp::default(),
            is_debug: false,
            is_production: true,
            is_admin: false,
            is_user_interactive: false,
            startup_path: String::new(),
            app_data: String::new(),
            os_name: String::new(),
            os_version: String::new(),
            is_net_core: true,
            is_linux: true,
            is_osx: false,
            is_windows: false,
            is_docker: true,
            is_containerized: true,
            mode: RuntimeMode::Console,
            branch: "main".to_owned(),
            authentication: AuthenticationType::Forms,
            sqlite_version: String::new(),
            migration_version: 0,
            url_base: super::URL_BASE.to_owned(),
            runtime_version: String::new(),
            runtime_name: ".NET".to_owned(),
            start_time: Timestamp::default(),
            package_version: String::new(),
            package_author: "Unseen".to_owned(),
            package_update_mechanism: UpdateMechanism::Docker,
            package_update_mechanism_message: String::new(),
            database_version: String::new(),
            database_type: DatabaseType::SqLite,
        }
    }
}
