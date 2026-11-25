//! Radarr v3 resources, field for field as the pinned OpenAPI document
//! (`fixtures/radarr/openapi-<version>.json`) lists them. Every field is
//! serialised, with a default when Unseen has no value (docs/14).
//!
//! The exceptions are objects that stand for "nothing here": `movieFile` of a
//! movie without a file, `collection`, `movie` of a queue record that has no
//! movie, and `quality` of a quality group. Radarr leaves them out; Unseen
//! sends `null` so that the key is present. The schema test lists them.

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

wire_enum!(MovieStatusType {
    Tba = "tba",
    Announced = "announced",
    InCinemas = "inCinemas",
    Released = "released",
    Deleted = "deleted",
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
wire_enum!(AddMovieMethod {
    Manual = "manual",
    List = "list",
    Collection = "collection",
});
wire_enum!(MonitorTypes {
    MovieOnly = "movieOnly",
    MovieAndCollection = "movieAndCollection",
    None = "none",
});
wire_enum!(RatingType {
    User = "user",
    Critic = "critic",
});
wire_enum!(SourceType {
    Tmdb = "tmdb",
    Mappings = "mappings",
    User = "user",
    Indexer = "indexer",
});
wire_enum!(QualitySource {
    Unknown = "unknown",
    Cam = "cam",
    Telesync = "telesync",
    Telecine = "telecine",
    Workprint = "workprint",
    Dvd = "dvd",
    Tv = "tv",
    WebDl = "webdl",
    WebRip = "webrip",
    Bluray = "bluray",
});
wire_enum!(Modifier {
    None = "none",
    Regional = "regional",
    Screener = "screener",
    RawHd = "rawhd",
    BrDisk = "brdisk",
    Remux = "remux",
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

/// An ISO 8601 date-time. Radarr's value for "no date" in a field that
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
    pub modifier: Modifier,
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
pub struct AlternativeTitleResource {
    pub id: i32,
    pub source_type: SourceType,
    pub movie_metadata_id: i32,
    pub title: String,
    pub clean_title: String,
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
pub struct AddMovieOptions {
    pub ignore_episodes_with_files: bool,
    pub ignore_episodes_without_files: bool,
    pub monitor: MonitorTypes,
    pub search_for_movie: bool,
    pub add_method: AddMovieMethod,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RatingChild {
    pub votes: i32,
    pub value: f64,
    #[serde(rename = "type")]
    pub kind: RatingType,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Ratings {
    pub imdb: RatingChild,
    pub tmdb: RatingChild,
    pub metacritic: RatingChild,
    pub rotten_tomatoes: RatingChild,
    pub trakt: RatingChild,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MovieCollectionResource {
    pub title: String,
    pub tmdb_id: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MovieStatisticsResource {
    pub movie_file_count: i32,
    pub size_on_disk: i64,
    pub release_groups: Vec<String>,
    pub movie_file_qualities: Vec<Quality>,
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
pub struct MovieFileResource {
    pub id: i32,
    pub movie_id: i32,
    pub relative_path: String,
    pub path: String,
    pub size: i64,
    pub date_added: Timestamp,
    pub scene_name: String,
    pub release_group: String,
    pub edition: String,
    pub languages: Vec<Language>,
    pub quality: QualityModel,
    pub custom_formats: Vec<CustomFormatResource>,
    pub custom_format_score: Option<i32>,
    pub indexer_flags: Option<i32>,
    pub media_info: MediaInfoResource,
    pub original_file_path: String,
    pub quality_cutoff_not_met: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MovieResource {
    pub id: i32,
    pub title: String,
    pub original_title: String,
    pub original_language: Language,
    pub alternate_titles: Vec<AlternativeTitleResource>,
    pub secondary_year: Option<i32>,
    pub secondary_year_source_id: i32,
    pub sort_title: String,
    pub size_on_disk: Option<i64>,
    pub status: MovieStatusType,
    pub overview: String,
    pub in_cinemas: Option<Timestamp>,
    pub physical_release: Option<Timestamp>,
    pub digital_release: Option<Timestamp>,
    pub release_date: Option<Timestamp>,
    pub physical_release_note: String,
    pub images: Vec<MediaCover>,
    pub website: String,
    pub remote_poster: String,
    pub year: i32,
    pub you_tube_trailer_id: String,
    pub studio: String,
    pub path: String,
    pub quality_profile_id: i32,
    pub has_file: Option<bool>,
    pub movie_file_id: i32,
    pub monitored: bool,
    pub minimum_availability: MovieStatusType,
    pub is_available: bool,
    pub folder_name: String,
    pub runtime: i32,
    pub clean_title: String,
    pub imdb_id: String,
    pub tmdb_id: i32,
    pub title_slug: String,
    pub root_folder_path: String,
    pub folder: String,
    pub certification: String,
    pub genres: Vec<String>,
    pub keywords: Vec<String>,
    pub tags: Vec<i32>,
    pub added: Timestamp,
    pub add_options: AddMovieOptions,
    pub ratings: Ratings,
    pub movie_file: Option<MovieFileResource>,
    pub collection: Option<MovieCollectionResource>,
    pub popularity: f64,
    pub last_search_time: Option<Timestamp>,
    pub statistics: MovieStatisticsResource,
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
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct QualityProfileQualityItemResource {
    pub id: i32,
    pub name: String,
    pub quality: Option<Quality>,
    pub items: Vec<QualityProfileQualityItemResource>,
    pub allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
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
    pub language: Language,
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
    pub movie_id: Option<i32>,
    pub movie: Option<MovieResource>,
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
    pub sizeleft: f64,
    /// A .NET time span, `hh:mm:ss`, absent while unknown.
    pub timeleft: Option<String>,
}

/// A page of records. Radarr has one schema per record type, all with these
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
pub type MovieResourcePagingResource = Paging<MovieResource>;

/// The `body` of a command resource: the fields of Radarr's base `Command`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Command {
    pub send_updates_to_client: bool,
    pub update_scheduled_task: bool,
    pub completion_message: String,
    pub requires_disk_access: bool,
    pub is_exclusive: bool,
    pub is_type_exclusive: bool,
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
    pub database_type: DatabaseType,
    pub database_version: String,
    pub authentication: AuthenticationType,
    pub migration_version: i32,
    pub url_base: String,
    pub runtime_version: String,
    pub runtime_name: String,
    pub start_time: Timestamp,
    pub package_version: String,
    pub package_author: String,
    pub package_update_mechanism: UpdateMechanism,
    pub package_update_mechanism_message: String,
}

impl Default for SystemResource {
    fn default() -> Self {
        Self {
            app_name: "Radarr".to_owned(),
            instance_name: "Radarr".to_owned(),
            version: super::RADARR_VERSION.to_owned(),
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
            branch: "master".to_owned(),
            database_type: DatabaseType::SqLite,
            database_version: String::new(),
            authentication: AuthenticationType::Forms,
            migration_version: 0,
            url_base: super::URL_BASE.to_owned(),
            runtime_version: String::new(),
            runtime_name: ".NET".to_owned(),
            start_time: Timestamp::default(),
            package_version: String::new(),
            package_author: "Unseen".to_owned(),
            package_update_mechanism: UpdateMechanism::Docker,
            package_update_mechanism_message: String::new(),
        }
    }
}
