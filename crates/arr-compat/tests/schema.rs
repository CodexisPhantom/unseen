//! Contract test: what the Radarr DTOs serialise to is checked against the
//! pinned upstream OpenAPI document (docs/12-testing.md, "Compat contract
//! tests", step 2).
//!
//! The check is stricter than JSON Schema: the upstream document has no
//! `required` list, but docs/14 promises that no field is omitted, so every
//! property of a schema must be present. Keys outside the schema are errors
//! (`additionalProperties: false` upstream). A `null` is accepted where the
//! schema says `nullable`, and for the few objects that Radarr itself leaves
//! out when there is nothing to show (`NULL_OBJECTS`).

use std::collections::BTreeSet;
use std::path::PathBuf;

use arr_compat::radarr::RADARR_VERSION;
use arr_compat::radarr::dto::*;
use arr_compat::radarr::map::{self, *};
use serde::Serialize;
use serde_json::{Value, json};
use unseen_core::profile::Item;

/// `Schema.property` pairs that may be `null` although the schema says they
/// cannot: an object that stands for "nothing here" (docs/14).
const NULL_OBJECTS: [&str; 4] = [
    "MovieResource.movieFile",
    "MovieResource.collection",
    "QueueResource.movie",
    "QualityProfileQualityItemResource.quality",
];

fn pinned_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/radarr")
        .join(format!("openapi-{RADARR_VERSION}.json"))
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
    // Without the presence check the same value passes.
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

    let mut movie = serde_json::to_value(MovieResource::default()).unwrap();
    movie["status"] = json!("coming");
    movie["added"] = json!("yesterday");
    movie["movieFile"] = Value::Null;
    movie["originalLanguage"] = Value::Null;
    assert_eq!(checker.problems("MovieResource", &movie).len(), 3);

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
    assert_eq!(name, format!("openapi-{RADARR_VERSION}.json"));
    assert_eq!(pinned()["info"]["title"], "Radarr");
    // Radarr's own recorded answer has no `maxSize` from Bluray-1080p up, so
    // only the types are checked.
    let recorded = include_str!("../../../fixtures/radarr/qualitydefinition.json");
    let recorded: Vec<Value> = serde_json::from_str(recorded).unwrap();
    let checker = Checker::new(false);
    for row in &recorded {
        let problems = checker.problems("QualityDefinitionResource", row);
        assert!(problems.is_empty(), "{problems:?}");
    }
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
        // The serde name is the wire name.
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
        MovieStatusType,
        MediaCoverTypes,
        AddMovieMethod,
        MonitorTypes,
        RatingType,
        SourceType,
        QualitySource,
        Modifier,
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
fn every_core_quality_maps_to_a_schema_quality() {
    for core in unseen_core::quality::Quality::ALL {
        let quality = Quality::from(core);
        assert_valid("Quality", &quality);
        assert_eq!(quality.id, core.id());
    }
}

// -------------------------------------------------------------- resources

fn view() -> MovieView {
    MovieView {
        id: 7,
        tmdb_id: 603,
        imdb_id: Some("tt0133093".to_owned()),
        title: "The Matrix".to_owned(),
        original_title: Some("The Matrix".to_owned()),
        sort_title: "matrix".to_owned(),
        year: 1999,
        overview: Some("A hacker learns the truth.".to_owned()),
        runtime: Some(136),
        status: "released".to_owned(),
        in_cinemas: Some("1999-03-30".to_owned()),
        digital_release: Some("2004-09-21T00:00:00.000Z".to_owned()),
        physical_release: None,
        poster_path: Some("/poster.jpg".to_owned()),
        root_folder_path: "/media/movies".to_owned(),
        folder_name: "The Matrix (1999)".to_owned(),
        monitored: true,
        minimum_availability: "released".to_owned(),
        added_at: "2026-11-16T20:15:13.123Z".to_owned(),
        tags: vec![2, 5],
        quality_profile_id: 4,
        file: Some(FileView {
            id: 11,
            relative_path: "The Matrix (1999) Bluray-1080p.mkv".to_owned(),
            size: 8_000_000_000,
            quality_id: 7,
            release_group: Some("GRP".to_owned()),
            edition: Some("Director's Cut".to_owned()),
            scene_name: Some("The.Matrix.1999.1080p.BluRay.x264-GRP".to_owned()),
            languages: vec!["en".to_owned(), "fr".to_owned()],
            media_info: None,
            added_at: "2026-11-17T08:00:00.000Z".to_owned(),
        }),
    }
}

/// A movie resource with every array and nested object filled, including
/// the fields Unseen has no source for, so the checker sees all of them.
fn full_movie() -> MovieResource {
    let mut movie = movie_resource(&view(), "2026-11-25");
    movie.alternate_titles = vec![AlternativeTitleResource {
        id: 1,
        source_type: SourceType::Tmdb,
        movie_metadata_id: 7,
        title: "Matrix".to_owned(),
        clean_title: "matrix".to_owned(),
    }];
    movie.genres = vec!["Action".to_owned()];
    movie.keywords = vec!["simulation".to_owned()];
    movie.collection = Some(MovieCollectionResource {
        title: "The Matrix Collection".to_owned(),
        tmdb_id: 2344,
    });
    let child = RatingChild {
        votes: 10,
        value: 8.7,
        kind: RatingType::User,
    };
    movie.ratings = Ratings {
        imdb: child.clone(),
        tmdb: child.clone(),
        metacritic: child.clone(),
        rotten_tomatoes: RatingChild {
            kind: RatingType::Critic,
            ..child.clone()
        },
        trakt: child,
    };
    movie.secondary_year = Some(2000);
    movie.last_search_time = Some(Timestamp::parse("2026-11-20"));
    movie.release_date = Some(Timestamp::parse("2004-09-21"));
    let file = movie.movie_file.as_mut().unwrap();
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
    file.media_info = MediaInfoResource {
        id: 1,
        audio_bitrate: 640_000,
        audio_channels: 5.1,
        audio_codec: "AC3".to_owned(),
        video_codec: "x264".to_owned(),
        resolution: "1920x1080".to_owned(),
        run_time: "02:16:00".to_owned(),
        ..MediaInfoResource::default()
    };
    movie
}

#[test]
fn a_movie_with_a_file_has_every_field() {
    let movie = full_movie();
    assert_valid("MovieResource", &movie);
    assert_valid("MovieFileResource", movie.movie_file.as_ref().unwrap());
    assert_valid(
        "MovieResourcePagingResource",
        &page(vec![movie], 1, 20, 1, "title", SortDirection::Ascending),
    );
}

#[test]
fn a_movie_without_a_file_or_a_collection_has_every_field() {
    let mut bare = view();
    bare.file = None;
    bare.imdb_id = None;
    bare.poster_path = None;
    assert_valid("MovieResource", &movie_resource(&bare, "2026-11-25"));
    assert_valid("MovieResource", &MovieResource::default());
    assert_valid("MovieFileResource", &MovieFileResource::default());
}

#[test]
fn a_profile_with_a_group_has_every_field() {
    let profile = quality_profile(&ProfileView {
        id: 4,
        name: "HD-1080p".to_owned(),
        upgrade_allowed: true,
        cutoff_quality_id: 1000,
        items: vec![
            Item::Quality {
                quality_id: 7,
                allowed: false,
            },
            Item::Group {
                id: 1000,
                name: "WEB 1080p".to_owned(),
                quality_ids: vec![3, 15],
                allowed: true,
            },
        ],
        language: "en".to_owned(),
        min_format_score: 0,
        cutoff_format_score: 10,
        min_upgrade_format_score: 1,
    });
    assert_valid("QualityProfileResource", &profile);
    let mut with_format = profile;
    with_format.format_items = vec![ProfileFormatItemResource {
        id: 1,
        format: 2,
        name: "x265".to_owned(),
        score: 50,
    }];
    assert_valid("QualityProfileResource", &with_format);
    assert_valid("QualityProfileResource", &QualityProfileResource::default());
}

#[test]
fn root_folders_and_tags_have_every_field() {
    let folder = root_folder(&RootFolderView {
        id: 1,
        path: "/media/movies".to_owned(),
        accessible: true,
        free_space: Some(1_000_000),
        unmapped: vec!["Stray".to_owned()],
    });
    assert_valid("RootFolderResource", &folder);
    assert_valid("RootFolderResource", &RootFolderResource::default());
    assert_valid("TagResource", &map::tag(3, "4k"));
}

fn queue() -> QueueView {
    QueueView {
        id: 9,
        movie: Some(view()),
        quality_id: 7,
        title: "The.Matrix.1999.1080p.BluRay.x264-GRP".to_owned(),
        size: 8e9,
        size_left: 2e9,
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
    let item = queue_item(&queue(), "2026-11-25");
    assert_eq!(item.timeleft.as_deref(), Some("1.02:03:04"));
    assert_valid("QueueResource", &item);
    let mut nothing = queue();
    nothing.movie = None;
    nothing.time_left_seconds = None;
    nothing.estimated_completion = None;
    nothing.added = None;
    let nothing = queue_item(&nothing, "2026-11-25");
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
        name: "MoviesSearch".to_owned(),
        status: CommandStatus::Completed,
        queued: "2026-11-25T10:00:00Z".to_owned(),
        started: Some("2026-11-25T10:00:01Z".to_owned()),
        ended: Some("2026-11-25T10:00:05Z".to_owned()),
    });
    assert_eq!(done.command_name, "Movies Search");
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
    });
    assert_eq!(status.version, RADARR_VERSION);
    assert_eq!(status.app_name, "Radarr");
    assert_eq!(status.url_base, "/radarr");
    assert_valid("SystemResource", &status);
    assert_valid("SystemResource", &SystemResource::default());
}
