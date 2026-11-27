//! Contract test: what the Sonarr DTOs serialise to is checked against the
//! pinned upstream OpenAPI document (docs/12-testing.md, "Compat contract
//! tests", step 2).
//!
//! The check is stricter than JSON Schema: the upstream document has no
//! `required` list, but docs/14 promises that no field is omitted, so every
//! property of a schema must be present. Keys outside the schema are errors
//! (`additionalProperties: false` upstream). A `null` is accepted where the
//! schema says `nullable`, and for the few objects that Sonarr itself leaves
//! out when there is nothing to show (`NULL_OBJECTS`).

use std::collections::BTreeSet;
use std::path::PathBuf;

use arr_compat::sonarr::SONARR_VERSION;
use arr_compat::sonarr::dto::*;
use arr_compat::sonarr::map::{self, *};
use serde::Serialize;
use serde_json::{Value, json};
use unseenarr_core::profile::Item;
use unseenarr_core::quality::Quality as CoreQuality;

/// `Schema.property` pairs that may be `null` although the schema says they
/// cannot: an object that stands for "nothing here" (docs/14).
const NULL_OBJECTS: [&str; 5] = [
    "EpisodeResource.episodeFile",
    "EpisodeResource.series",
    "QueueResource.series",
    "QueueResource.episode",
    "QualityProfileQualityItemResource.quality",
];

fn pinned_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/sonarr")
        .join(format!("openapi-{SONARR_VERSION}.json"))
}

fn pinned() -> Value {
    let text = std::fs::read_to_string(pinned_path()).expect("the pinned OpenAPI document");
    serde_json::from_str(&text).expect("valid JSON")
}

struct Checker {
    document: Value,
    /// Also demand every property of every object.
    presence: bool,
}

impl Checker {
    fn new(presence: bool) -> Self {
        Self {
            document: pinned(),
            presence,
        }
    }

    fn schema(&self, name: &str) -> &Value {
        &self.document["components"]["schemas"][name]
    }

    /// The problems of `value` against the named schema; empty when valid.
    fn problems(&self, name: &str, value: &impl Serialize) -> Vec<String> {
        let value = serde_json::to_value(value).expect("serialisable");
        let mut found = Vec::new();
        assert!(
            !self.schema(name).is_null(),
            "the pinned document has no schema {name}"
        );
        self.check(self.schema(name), &value, name, name, &mut found);
        found
    }

    fn check(
        &self,
        schema: &Value,
        value: &Value,
        owner: &str,
        path: &str,
        found: &mut Vec<String>,
    ) {
        if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
            let name = reference.rsplit('/').next().expect("a name");
            return self.check(self.schema(name), value, name, path, found);
        }
        if value.is_null() {
            if schema["nullable"] != true {
                found.push(format!("{path}: null where the schema is not nullable"));
            }
            return;
        }
        if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
            if !allowed.contains(value) {
                found.push(format!("{path}: {value} is not one of {allowed:?}"));
            }
            return;
        }
        let kind = schema.get("type").and_then(Value::as_str);
        let format = schema.get("format").and_then(Value::as_str);
        let fits = match kind {
            None => true,
            Some("string") => value.is_string(),
            Some("boolean") => value.is_boolean(),
            Some("number") => value.is_number(),
            Some("integer") => match format {
                Some("int32") => value
                    .as_i64()
                    .is_some_and(|number| i32::try_from(number).is_ok()),
                _ => value.is_i64() || value.is_u64(),
            },
            Some("array") => value.is_array(),
            Some("object") => value.is_object(),
            Some(other) => panic!("{path}: unexpected schema type {other}"),
        };
        if !fits {
            found.push(format!(
                "{path}: {value} is not of type {kind:?} {format:?}"
            ));
            return;
        }
        match (kind, format, value) {
            (Some("string"), Some("date-time"), Value::String(text))
                if chrono::DateTime::parse_from_rfc3339(text).is_err() =>
            {
                found.push(format!("{path}: {text:?} is not an RFC 3339 date-time"));
            }
            (Some("string"), Some("date-span"), Value::String(text)) if !time_span(text) => {
                found.push(format!("{path}: {text:?} is not a .NET time span"));
            }
            (Some("array"), _, Value::Array(items)) => {
                for (index, item) in items.iter().enumerate() {
                    self.check(
                        &schema["items"],
                        item,
                        owner,
                        &format!("{path}[{index}]"),
                        found,
                    );
                }
            }
            (Some("object"), _, Value::Object(map)) => {
                let properties = schema["properties"]
                    .as_object()
                    .cloned()
                    .unwrap_or_default();
                for (key, sub) in &properties {
                    match map.get(key) {
                        None if self.presence => found.push(format!("{path}.{key}: missing")),
                        None => {}
                        Some(Value::Null)
                            if NULL_OBJECTS.contains(&format!("{owner}.{key}").as_str()) => {}
                        Some(inner) => {
                            self.check(sub, inner, owner, &format!("{path}.{key}"), found)
                        }
                    }
                }
                if schema["additionalProperties"] == false {
                    for key in map.keys().filter(|key| !properties.contains_key(*key)) {
                        found.push(format!("{path}.{key}: not in the schema"));
                    }
                }
            }
            _ => {}
        }
    }
}

/// `[d.]hh:mm:ss[.fraction]`
fn time_span(text: &str) -> bool {
    let clock = match text.split_once('.') {
        Some((days, rest)) if !days.is_empty() && days.bytes().all(|b| b.is_ascii_digit()) => rest,
        _ => text,
    };
    let clock = clock.split('.').next().unwrap_or("");
    let parts: Vec<_> = clock.split(':').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit()))
}

fn assert_valid(name: &str, value: &impl Serialize) {
    let problems = Checker::new(true).problems(name, value);
    assert!(problems.is_empty(), "{name}: {problems:#?}");
}

// ---------------------------------------------------------------- checker

#[test]
fn the_checker_finds_each_kind_of_problem() {
    let checker = Checker::new(true);
    let good = serde_json::to_value(TagResource::default()).unwrap();
    assert!(checker.problems("TagResource", &good).is_empty());

    let mut missing = good.clone();
    missing.as_object_mut().unwrap().remove("label");
    assert_eq!(
        checker.problems("TagResource", &missing),
        ["TagResource.label: missing"]
    );
    assert!(
        Checker::new(false)
            .problems("TagResource", &missing)
            .is_empty()
    );

    let extra = json!({"id": 1, "label": "x", "colour": "red"});
    assert_eq!(
        checker.problems("TagResource", &extra),
        ["TagResource.colour: not in the schema"]
    );
    let wrong = json!({"id": 1.0, "label": 5});
    assert_eq!(checker.problems("TagResource", &wrong).len(), 2);
    let big = json!({"id": 3_000_000_000_i64, "label": "x"});
    assert_eq!(checker.problems("TagResource", &big).len(), 1);

    let mut series = serde_json::to_value(SeriesResource::default()).unwrap();
    series["status"] = json!("cancelled");
    series["added"] = json!("yesterday");
    series["originalLanguage"] = Value::Null;
    assert_eq!(checker.problems("SeriesResource", &series).len(), 3);

    let mut queue = serde_json::to_value(QueueResource::default()).unwrap();
    queue["timeleft"] = json!("PT5M");
    assert_eq!(checker.problems("QueueResource", &queue).len(), 1);
    queue["timeleft"] = json!("1.02:03:04");
    assert!(checker.problems("QueueResource", &queue).is_empty());
}

#[test]
fn the_pinned_document_is_the_pinned_version() {
    let name = pinned_path();
    let name = name.file_name().unwrap().to_string_lossy();
    assert_eq!(name, format!("openapi-{SONARR_VERSION}.json"));
    assert_eq!(pinned()["info"]["title"], "Sonarr");
}

// ------------------------------------------------------------------ enums

fn wire_names<T: Copy>(all: &[T], wire: impl Fn(T) -> &'static str) -> BTreeSet<String> {
    all.iter()
        .map(|variant| wire(*variant).to_owned())
        .collect()
}

fn schema_names(name: &str) -> BTreeSet<String> {
    Checker::new(true).schema(name)["enum"]
        .as_array()
        .unwrap_or_else(|| panic!("{name} is not an enum"))
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect()
}

macro_rules! same_names {
    ($($name:ident),+ $(,)?) => {$(
        assert_eq!(
            wire_names($name::ALL, $name::wire),
            schema_names(stringify!($name)),
            stringify!($name)
        );
        for variant in $name::ALL {
            assert_eq!(serde_json::to_value(variant).unwrap(), variant.wire());
            let back: $name = serde_json::from_value(json!(variant.wire())).unwrap();
            assert_eq!(back, *variant);
        }
    )+};
}

#[test]
fn every_enum_has_the_names_of_the_schema() {
    same_names!(
        SeriesStatusType,
        SeriesTypes,
        NewItemMonitorTypes,
        MediaCoverTypes,
        MonitorTypes,
        QualitySource,
        ReleaseType,
        QueueStatus,
        TrackedDownloadStatus,
        TrackedDownloadState,
        DownloadProtocol,
        SortDirection,
        CommandPriority,
        CommandStatus,
        CommandResult,
        CommandTrigger,
        RuntimeMode,
        DatabaseType,
        AuthenticationType,
        UpdateMechanism,
    );
}

#[test]
fn every_tv_quality_maps_to_a_schema_quality_and_round_trips() {
    let mut tv_count = 0;
    for core in CoreQuality::ALL {
        if let Some(sonarr_id) = to_sonarr_quality_id(core.id()) {
            tv_count += 1;
            let dto = quality_from_core(core).expect("TV quality");
            assert_valid("Quality", &dto);
            assert_eq!(dto.id, sonarr_id);
            assert_eq!(from_sonarr_quality_id(sonarr_id), Some(core.id()));
        }
    }
    assert_eq!(tv_count, 22);
    assert_eq!(quality(CoreQuality::Bluray480p.id()).id, 13);
    assert_eq!(quality(CoreQuality::Bluray576p.id()).id, 22);
    assert_eq!(quality(CoreQuality::Remux1080p.id()).id, 20);
    assert_eq!(
        quality(CoreQuality::Remux1080p.id()).name,
        "Bluray-1080p Remux"
    );
    assert_eq!(quality(CoreQuality::Remux2160p.id()).id, 21);
    assert_eq!(
        quality(CoreQuality::Remux2160p.id()).name,
        "Bluray-2160p Remux"
    );
}

// -------------------------------------------------------------- resources

fn show_view() -> SeriesView {
    SeriesView {
        id: 7,
        tvdb_id: Some(280_619),
        tmdb_id: Some(63_639),
        imdb_id: Some("tt3230854".to_owned()),
        title: "The Expanse".to_owned(),
        original_title: Some("The Expanse".to_owned()),
        sort_title: "expanse".to_owned(),
        year: 2015,
        overview: Some("A detective and a rogue ship's crew.".to_owned()),
        runtime: Some(45),
        status: "ended".to_owned(),
        series_type: "standard".to_owned(),
        network: Some("Prime Video".to_owned()),
        poster_path: Some("/expanse.jpg".to_owned()),
        season_folder: true,
        root_folder_path: "/media/tv".to_owned(),
        folder_name: "The Expanse".to_owned(),
        monitored: true,
        monitor_new_items: "all".to_owned(),
        first_aired: Some("2015-12-14".to_owned()),
        last_aired: Some("2022-01-14T00:00:00Z".to_owned()),
        next_airing: None,
        previous_airing: Some("2022-01-14T00:00:00Z".to_owned()),
        original_language: Some("en".to_owned()),
        added_at: "2026-11-16T20:15:13.123Z".to_owned(),
        aliases: vec!["Expanse".to_owned()],
        tags: vec![2, 5],
        quality_profile_id: 4,
        seasons: vec![SeasonView {
            season_number: 1,
            monitored: true,
            title: Some("Season 1".to_owned()),
            next_airing: None,
            previous_airing: Some("2016-02-02T03:00:00Z".to_owned()),
            episode_file_count: 10,
            episode_count: 10,
            total_episode_count: 10,
            size_on_disk: 40_000_000_000,
            release_groups: vec!["GRP".to_owned()],
        }],
    }
}

fn episode_view() -> EpisodeView {
    EpisodeView {
        id: 101,
        series_id: 7,
        tvdb_id: Some(5_000_001),
        season_number: 1,
        episode_number: 1,
        absolute_number: Some(1),
        title: "Dulcinea".to_owned(),
        overview: Some("In the asteroid belt, Detective Miller searches for Julie Mao.".to_owned()),
        air_date: Some("2015-12-14".to_owned()),
        air_date_utc: Some("2015-12-15T03:00:00Z".to_owned()),
        runtime: Some(46),
        monitored: true,
        file: Some(EpisodeFileView {
            id: 201,
            series_id: 7,
            season_number: 1,
            relative_path: "Season 1/The Expanse - S01E01 - Dulcinea Bluray-1080p.mkv".to_owned(),
            size: 4_000_000_000,
            quality_id: CoreQuality::Remux1080p.id(),
            release_group: Some("GRP".to_owned()),
            scene_name: Some("The.Expanse.S01E01.1080p.BluRay.REMUX-GRP".to_owned()),
            languages: vec!["en".to_owned(), "fr".to_owned()],
            release_type: ReleaseType::SingleEpisode,
            media_info: Some(MediaInfoResource {
                id: 1,
                audio_bitrate: 640_000,
                audio_channels: 5.1,
                audio_codec: "AC3".to_owned(),
                video_codec: "x264".to_owned(),
                resolution: "1920x1080".to_owned(),
                run_time: "00:46:00".to_owned(),
                ..MediaInfoResource::default()
            }),
            quality_cutoff_not_met: false,
            added_at: "2026-11-17T08:00:00.000Z".to_owned(),
        }),
    }
}

/// A series resource with every array and nested object filled, including
/// fields Unseenarr has no source for, so the checker validates all of them.
fn full_series() -> SeriesResource {
    let mut series = series_resource(&show_view());
    series.alternate_titles = vec![AlternateTitleResource {
        title: "Expanse".to_owned(),
        season_number: Some(1),
        scene_season_number: Some(1),
        scene_origin: "tvdb".to_owned(),
        comment: "short".to_owned(),
    }];
    series.season_types = vec![SeasonTypeResource {
        name: "Aired Order".to_owned(),
        kind: "official".to_owned(),
        season_numbers: vec![1, 2],
        episode_count: 23,
    }];
    series.translations = vec![SeriesTranslationResource {
        language: language("fr"),
        title: "The Expanse".to_owned(),
        overview: "Synopsis".to_owned(),
    }];
    series.folders = vec![SeriesFolderResource {
        language: language("en"),
        folder: "The Expanse".to_owned(),
    }];
    series.genres = vec!["Science Fiction".to_owned(), "Drama".to_owned()];
    series.ratings = Ratings {
        votes: 120_000,
        value: 8.5,
    };
    series.episodes_changed = Some(false);
    series.seasons[0].images = vec![MediaCover {
        cover_type: MediaCoverTypes::Poster,
        url: "https://image.tmdb.org/t/p/original/s1.jpg".to_owned(),
        remote_url: "https://image.tmdb.org/t/p/original/s1.jpg".to_owned(),
    }];
    series
}

#[test]
fn series_and_seasons_have_every_field() {
    let series = full_series();
    assert_valid("SeriesResource", &series);
    assert_valid("SeasonResource", &series.seasons[0]);
    assert_valid("SeriesResource", &SeriesResource::default());
    assert_valid("SeasonResource", &SeasonResource::default());
}

#[test]
fn episodes_and_episode_files_have_every_field() {
    let mut episode = episode_resource(&episode_view(), "/media/tv/The Expanse");
    episode.last_search_time = Some(Timestamp::parse("2026-11-20"));
    episode.series = Some(full_series());
    episode.images = vec![MediaCover {
        cover_type: MediaCoverTypes::Screenshot,
        url: "https://image.tmdb.org/t/p/original/ep1.jpg".to_owned(),
        remote_url: "https://image.tmdb.org/t/p/original/ep1.jpg".to_owned(),
    }];
    let file = episode.episode_file.as_mut().unwrap();
    file.custom_formats = vec![CustomFormatResource {
        id: 1,
        name: "x265".to_owned(),
        include_custom_format_when_renaming: Some(false),
        specifications: vec![CustomFormatSpecificationSchema {
            id: 1,
            name: "x265".to_owned(),
            implementation: "ReleaseTitleSpecification".to_owned(),
            presets: vec![CustomFormatSpecificationSchema::default()],
            ..CustomFormatSpecificationSchema::default()
        }],
    }];
    file.indexer_flags = Some(0);
    assert_valid("EpisodeResource", &episode);
    assert_valid(
        "EpisodeFileResource",
        episode.episode_file.as_ref().unwrap(),
    );
    assert_valid(
        "EpisodeResourcePagingResource",
        &page(
            vec![episode],
            1,
            20,
            1,
            "airDateUtc",
            SortDirection::Descending,
        ),
    );

    let mut bare = episode_view();
    bare.file = None;
    bare.air_date = None;
    bare.air_date_utc = None;
    bare.absolute_number = None;
    assert_valid(
        "EpisodeResource",
        &episode_resource(&bare, "/media/tv/The Expanse"),
    );
    assert_valid("EpisodeResource", &EpisodeResource::default());
    assert_valid("EpisodeFileResource", &EpisodeFileResource::default());
    assert_valid(
        "EpisodesMonitoredResource",
        &EpisodesMonitoredResource {
            episode_ids: vec![101, 102],
            monitored: true,
        },
    );
}

#[test]
fn quality_and_language_profiles_have_every_field() {
    let mut profile = quality_profile(&ProfileView {
        id: 4,
        name: "HD-1080p".to_owned(),
        upgrade_allowed: true,
        cutoff_quality_id: CoreQuality::Remux1080p.id(),
        items: vec![
            Item::Quality {
                quality_id: CoreQuality::Bluray480p.id(),
                allowed: false,
            },
            Item::Group {
                id: 1000,
                name: "WEB 1080p".to_owned(),
                quality_ids: vec![CoreQuality::WebDl1080p.id(), CoreQuality::WebRip1080p.id()],
                allowed: true,
            },
            Item::Quality {
                quality_id: CoreQuality::Remux1080p.id(),
                allowed: true,
            },
        ],
        min_format_score: 0,
        cutoff_format_score: 10,
        min_upgrade_format_score: 1,
    });
    assert_valid("QualityProfileResource", &profile);
    profile.format_items = vec![ProfileFormatItemResource {
        id: 1,
        format: 2,
        name: "x265".to_owned(),
        score: 50,
    }];
    assert_valid("QualityProfileResource", &profile);
    assert_valid("QualityProfileResource", &QualityProfileResource::default());

    let lang_profile = language_profile(&LanguageProfileView {
        id: 1,
        name: "English".to_owned(),
        upgrade_allowed: true,
        cutoff: "en".to_owned(),
        languages: vec![("en".to_owned(), true), ("ja".to_owned(), false)],
    });
    assert_valid("LanguageProfileResource", &lang_profile);
    assert_valid(
        "LanguageProfileResource",
        &LanguageProfileResource::default(),
    );
}

#[test]
fn root_folders_and_tags_have_every_field() {
    let folder = root_folder(&RootFolderView {
        id: 1,
        path: "/media/tv".to_owned(),
        accessible: true,
        free_space: Some(1_000_000),
        total_space: Some(2_000_000),
        unmapped: vec!["Stray".to_owned()],
    });
    assert_valid("RootFolderResource", &folder);
    assert_valid("RootFolderResource", &RootFolderResource::default());
    assert_valid("TagResource", &map::tag(3, "anime"));
}

fn queue_view() -> QueueView {
    QueueView {
        id: 9,
        series: Some(show_view()),
        episode: Some(episode_view()),
        season_number: Some(1),
        quality_id: CoreQuality::Remux1080p.id(),
        languages: vec!["en".to_owned()],
        title: "The.Expanse.S01E01.1080p.BluRay.REMUX-GRP".to_owned(),
        size: 4e9,
        size_left: 1e9,
        time_left_seconds: Some(93_784),
        estimated_completion: Some("2026-11-26T10:00:00Z".to_owned()),
        added: Some("2026-11-25T08:00:00Z".to_owned()),
        status: QueueStatus::Downloading,
        tracked_status: TrackedDownloadStatus::Warning,
        tracked_state: TrackedDownloadState::Downloading,
        status_messages: vec![("Stalled".to_owned(), vec!["No seeds".to_owned()])],
        error_message: Some("none".to_owned()),
        download_id: Some("ABCDEF".to_owned()),
        protocol: DownloadProtocol::Torrent,
        download_client: Some("qBittorrent".to_owned()),
        indexer: Some("Indexer".to_owned()),
        output_path: Some("/downloads/x".to_owned()),
    }
}

#[test]
fn the_queue_has_every_field() {
    let item = queue_item(&queue_view());
    assert_eq!(item.timeleft.as_deref(), Some("1.02:03:04"));
    assert_eq!(item.quality.quality.id, 20);
    assert!(item.episode_has_file);
    assert_valid("QueueResource", &item);

    let mut nothing = queue_view();
    nothing.series = None;
    nothing.episode = None;
    nothing.season_number = None;
    nothing.time_left_seconds = None;
    nothing.estimated_completion = None;
    nothing.added = None;
    let nothing = queue_item(&nothing);
    assert!(!nothing.episode_has_file);
    assert_valid("QueueResource", &nothing);
    assert_valid("QueueResource", &QueueResource::default());
    assert_valid(
        "QueueResourcePagingResource",
        &page(
            vec![item, nothing],
            1,
            20,
            2,
            "timeleft",
            SortDirection::Default,
        ),
    );
    assert_valid(
        "QueueResourcePagingResource",
        &QueueResourcePagingResource::default(),
    );
}

#[test]
fn commands_and_system_have_every_field() {
    let done = command(&CommandView {
        id: 3,
        name: "MissingEpisodeSearch".to_owned(),
        status: CommandStatus::Completed,
        queued: "2026-11-25T10:00:00Z".to_owned(),
        started: Some("2026-11-25T10:00:01Z".to_owned()),
        ended: Some("2026-11-25T10:00:05Z".to_owned()),
    });
    assert_eq!(done.command_name, "Missing Episode Search");
    assert_eq!(done.result, CommandResult::Successful);
    assert_valid("CommandResource", &done);
    let mut with_duration = done;
    with_duration.duration = Some("00:00:04".to_owned());
    assert_valid("CommandResource", &with_duration);
    assert_valid("CommandResource", &CommandResource::default());

    let status = system(&SystemView {
        start_time: "2026-11-25T09:00:00Z".to_owned(),
        startup_path: "/app".to_owned(),
        app_data: "/data".to_owned(),
        os_name: "debian".to_owned(),
        os_version: "13".to_owned(),
        sqlite_version: "3.46.0".to_owned(),
    });
    assert_eq!(status.version, SONARR_VERSION);
    assert_eq!(status.app_name, "Sonarr");
    assert_eq!(status.url_base, "/sonarr");
    assert_valid("SystemResource", &status);
    assert_valid("SystemResource", &SystemResource::default());
}
