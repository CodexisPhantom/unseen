use super::*;
use std::path::{Path, PathBuf};

use crate::release::{self, Language, Modifier, ParsedRelease, Resolution, Source};
use serde_json::Value;

const GIB: i64 = 1024 * 1024 * 1024;

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/custom_formats")
        .join(name)
}

fn radarr_export_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/radarr/customformat.json")
}

fn fixture(name: &str) -> Vec<CustomFormat> {
    CustomFormat::from_json(&std::fs::read_to_string(fixture_path(name)).expect("fixture"))
        .expect("fixture parses")
}

fn radarr_export() -> Vec<CustomFormat> {
    CustomFormat::from_json(&std::fs::read_to_string(radarr_export_path()).expect("export"))
        .expect("export parses")
}

fn fields(entries: &[(&str, Value)]) -> Vec<Field> {
    entries
        .iter()
        .map(|(name, value)| Field {
            name: (*name).to_owned(),
            value: value.clone(),
        })
        .collect()
}

fn number(value: i64) -> Value {
    Value::from(value)
}

fn string(value: &str) -> Value {
    Value::from(value)
}

fn test_format(specifications: Vec<Specification>) -> CustomFormat {
    CustomFormat {
        include_custom_format_when_renaming: false,
        name: "Test".to_owned(),
        specifications,
    }
}

fn test_specification(
    implementation: &str,
    required: bool,
    negate: bool,
    fields: Vec<Field>,
) -> Specification {
    Specification {
        name: implementation.to_owned(),
        implementation: implementation.to_owned(),
        negate,
        required,
        fields,
    }
}

fn source_id(source: Source) -> i32 {
    match source {
        Source::Unknown => 0,
        Source::Cam => 1,
        Source::Telesync => 2,
        Source::Telecine => 3,
        Source::Workprint => 4,
        Source::Dvd => 5,
        Source::Tv => 6,
        Source::WebDl => 7,
        Source::WebRip => 8,
        Source::Bluray => 9,
    }
}

fn resolution_id(resolution: Resolution) -> i32 {
    match resolution {
        Resolution::R480p => 480,
        Resolution::R576p => 576,
        Resolution::R720p => 720,
        Resolution::R1080p => 1080,
        Resolution::R2160p => 2160,
    }
}

fn modifier_id(modifier: Modifier) -> i32 {
    match modifier {
        Modifier::None => 0,
        Modifier::Regional => 1,
        Modifier::Screener => 2,
        Modifier::RawHd => 3,
        Modifier::BrDisk => 4,
        Modifier::Remux => 5,
    }
}

fn info<'a>(
    title: &'a str,
    parsed: &'a ParsedRelease,
    languages: &'a [i32],
    size_bytes: i64,
    indexer_flags: &'a [i32],
    movie_year: Option<i32>,
) -> ReleaseInfo<'a> {
    ReleaseInfo {
        title,
        release_group: parsed.release_group.as_deref(),
        edition: parsed.edition.as_deref(),
        languages,
        source: Some(source_id(parsed.source)),
        resolution: parsed.resolution.map(resolution_id),
        quality_modifier: Some(modifier_id(parsed.modifier)),
        size_bytes,
        indexer_flags,
        year: parsed.year.map(i32::from),
        movie_year,
    }
}

fn matched_names(formats: &[CustomFormat], release: &ReleaseInfo<'_>) -> Vec<String> {
    let mut names: Vec<String> = formats
        .iter()
        .filter(|format| format.matches(release))
        .map(|format| format.name.clone())
        .collect();
    names.sort();
    names
}

#[test]
fn every_fixture_and_the_radarr_export_parse() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/custom_formats");
    let mut count = 0;
    for entry in std::fs::read_dir(&directory).expect("fixtures/custom_formats") {
        let path = entry.expect("directory entry").path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        count += 1;
        let text = std::fs::read_to_string(&path).expect("fixture text");
        let formats = CustomFormat::from_json(&text)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(formats.len(), 1, "{}: one format", path.display());
        assert!(
            !formats[0].name.trim().is_empty(),
            "{}: empty name",
            path.display()
        );
    }
    assert!(count >= 19, "expected at least 19 fixtures, found {count}");

    let formats = radarr_export();
    assert_eq!(formats.len(), 2);
    assert_eq!(formats[0].name, "x265 (HD)");
    assert_eq!(formats[1].name, "Remux Tier 01");
}

#[test]
fn both_field_shapes_are_read() {
    let radarr = r#"[{
        "name": "Radarr",
        "includeCustomFormatWhenRenaming": false,
        "specifications": [{
            "name": "Resolution",
            "implementation": "ResolutionSpecification",
            "negate": false,
            "required": true,
            "fields": [{
                "order": 0,
                "name": "value",
                "label": "Resolution",
                "value": 1080,
                "type": "select"
            }]
        }]
    }]"#;
    let trash = r#"{
        "name": "TRaSH",
        "includeCustomFormatWhenRenaming": false,
        "specifications": [{
            "name": "Resolution",
            "implementation": "ResolutionSpecification",
            "negate": false,
            "required": true,
            "fields": {"value": 1080}
        }]
    }"#;
    let radarr_formats = CustomFormat::from_json(radarr).expect("Radarr list fields");
    let trash_formats = CustomFormat::from_json(trash).expect("TRaSH map fields");
    assert_eq!(
        radarr_formats[0].specifications[0].fields,
        trash_formats[0].specifications[0].fields
    );
    assert_eq!(
        radarr_formats[0].specifications[0].fields[0],
        Field {
            name: "value".to_owned(),
            value: Value::from(1080),
        }
    );

    let parsed = fixture("x265.json");
    let serialized = serde_json::to_value(&parsed[0]).expect("serializes");
    assert!(serialized["specifications"][0]["fields"].is_array());
    let again = CustomFormat::from_json(&serialized.to_string()).expect("round trip");
    assert_eq!(again[0].name, parsed[0].name);
    assert_eq!(
        again[0].specifications[0].fields,
        parsed[0].specifications[0].fields
    );
}

#[test]
fn a_release_name_match_table() {
    let mut formats = radarr_export();
    formats.extend(fixture("x265.json"));
    formats.extend(fixture("hdr.json"));

    let cases: &[(&str, &[&str])] = &[
        ("Movie.2019.1080p.BluRay.x264-GROUP", &[]),
        ("Movie.2019.1080p.BluRay.x265-GROUP", &["x265", "x265 (HD)"]),
        ("Movie.2019.2160p.BluRay.x265-GROUP", &["x265"]),
        (
            "Movie.2019.1080p.BluRay.REMUX.AVC-FraMeSToR",
            &["Remux Tier 01"],
        ),
        (
            "Movie.2020.2160p.BluRay.REMUX.HEVC.HDR10-FraMeSToR",
            &["HDR", "Remux Tier 01"],
        ),
        ("Movie.2021.1080p.WEB-DL.DDP5.1.H.264-GROUP", &[]),
    ];
    for (name, expected) in cases {
        let parsed = release::parse(name);
        let release = info(name, &parsed, &[], 0, &[], None);
        let expected: Vec<String> = expected.iter().map(|name| (*name).to_owned()).collect();
        assert_eq!(matched_names(&formats, &release), expected, "{name}");
    }
}

fn check_kind(
    implementation: &str,
    matching: Vec<Field>,
    missing: Vec<Field>,
    release: &ReleaseInfo<'_>,
) {
    let one = |fields: Vec<Field>, negate: bool| {
        test_format(vec![test_specification(
            implementation,
            true,
            negate,
            fields,
        )])
        .matches(release)
    };
    assert!(
        one(matching.clone(), false),
        "{implementation}: a matching required specification matches"
    );
    assert!(
        !one(matching.clone(), true),
        "{implementation}: negate flips the result"
    );
    assert!(
        !one(missing.clone(), false),
        "{implementation}: a non-matching required specification fails"
    );
    let two = test_format(vec![
        test_specification(implementation, true, false, matching),
        test_specification(implementation, true, false, missing),
    ]);
    assert!(
        !two.matches(release),
        "{implementation}: two required specifications, one failing"
    );
}

#[test]
fn required_and_negate_cover_every_type() {
    let name = "Movie.2019.1080p.BluRay.REMUX.IMAX.HEVC.DTS-HD.MA.5.1-GROUP";
    let parsed = release::parse(name);
    assert_eq!(parsed.release_group.as_deref(), Some("GROUP"));
    assert_eq!(parsed.edition.as_deref(), Some("IMAX"));
    assert_eq!(parsed.source, Source::Bluray);
    assert_eq!(parsed.modifier, Modifier::Remux);
    assert_eq!(parsed.resolution, Some(Resolution::R1080p));
    let flags = [3];
    let release = info(name, &parsed, &[], 15 * GIB, &flags, Some(2005));

    check_kind(
        "ReleaseTitleSpecification",
        fields(&[("value", string(r"\bMovie\b"))]),
        fields(&[("value", string(r"\bDune\b"))]),
        &release,
    );
    check_kind(
        "ReleaseGroupSpecification",
        fields(&[("value", string("^GROUP$"))]),
        fields(&[("value", string("^OTHER$"))]),
        &release,
    );
    check_kind(
        "EditionSpecification",
        fields(&[("value", string(r"\bIMAX\b"))]),
        fields(&[("value", string(r"\bExtended\b"))]),
        &release,
    );
    check_kind(
        "SourceSpecification",
        fields(&[("value", number(9))]),
        fields(&[("value", number(7))]),
        &release,
    );
    check_kind(
        "ResolutionSpecification",
        fields(&[("value", number(1080))]),
        fields(&[("value", number(2160))]),
        &release,
    );
    check_kind(
        "QualityModifierSpecification",
        fields(&[("value", number(5))]),
        fields(&[("value", number(0))]),
        &release,
    );
    check_kind(
        "SizeSpecification",
        fields(&[("min", number(10)), ("max", number(20))]),
        fields(&[("min", number(20)), ("max", number(30))]),
        &release,
    );
    check_kind(
        "IndexerFlagSpecification",
        fields(&[("value", number(3))]),
        fields(&[("value", number(4))]),
        &release,
    );
    check_kind(
        "YearSpecification",
        fields(&[("min", number(2010)), ("max", number(2020))]),
        fields(&[("min", number(2000)), ("max", number(2010))]),
        &release,
    );

    let german_name = "Movie.2019.German.1080p.BluRay.REMUX.IMAX.HEVC.DTS-HD.MA.5.1-GROUP";
    let german = release::parse(german_name);
    assert_eq!(german.languages, vec![Language::German]);
    let german_flags = [3];
    let german_release = info(
        german_name,
        &german,
        &[4],
        15 * GIB,
        &german_flags,
        Some(2005),
    );
    check_kind(
        "LanguageSpecification",
        fields(&[("value", number(4))]),
        fields(&[("value", number(1))]),
        &german_release,
    );
    let except_german = test_format(vec![test_specification(
        "LanguageSpecification",
        true,
        false,
        fields(&[("value", number(4)), ("exceptLanguage", Value::Bool(true))]),
    )]);
    assert!(!except_german.matches(&german_release));
    let except_english = test_format(vec![test_specification(
        "LanguageSpecification",
        true,
        false,
        fields(&[("value", number(1)), ("exceptLanguage", Value::Bool(true))]),
    )]);
    assert!(except_english.matches(&german_release));
}

#[test]
fn non_required_specifications_are_grouped_by_implementation() {
    let name = "Movie.2019.1080p.BluRay.x264-GROUP";
    let parsed = release::parse(name);
    let release = info(name, &parsed, &[], 0, &[], None);

    let grouped = test_format(vec![
        test_specification(
            "ReleaseGroupSpecification",
            false,
            false,
            fields(&[("value", string("^GROUP$"))]),
        ),
        test_specification(
            "ReleaseGroupSpecification",
            false,
            false,
            fields(&[("value", string("^OTHER$"))]),
        ),
    ]);
    assert!(grouped.matches(&release));

    let other_name = "Movie.2019.1080p.BluRay.x264-RELEASE";
    let other = release::parse(other_name);
    let other_release = info(other_name, &other, &[], 0, &[], None);
    assert!(!grouped.matches(&other_release));

    let two_groups = test_format(vec![
        test_specification(
            "ReleaseTitleSpecification",
            false,
            false,
            fields(&[("value", string("Movie"))]),
        ),
        test_specification(
            "ReleaseGroupSpecification",
            false,
            false,
            fields(&[("value", string("^OTHER$"))]),
        ),
    ]);
    assert!(!two_groups.matches(&release));

    let required_match = test_format(vec![
        test_specification(
            "ReleaseGroupSpecification",
            true,
            false,
            fields(&[("value", string("^GROUP$"))]),
        ),
        test_specification(
            "ReleaseGroupSpecification",
            false,
            false,
            fields(&[("value", string("^OTHER$"))]),
        ),
    ]);
    assert!(required_match.matches(&release));

    let required_miss = test_format(vec![
        test_specification(
            "ReleaseGroupSpecification",
            true,
            false,
            fields(&[("value", string("^OTHER$"))]),
        ),
        test_specification(
            "ReleaseGroupSpecification",
            false,
            false,
            fields(&[("value", string("^GROUP$"))]),
        ),
    ]);
    assert!(!required_miss.matches(&release));

    let empty = test_format(Vec::new());
    assert!(empty.matches(&release));
    assert!(empty.matches(&other_release));
}

#[test]
fn unknown_implementations_and_bad_patterns_do_not_match_or_panic() {
    let name = "Movie.2019.1080p.BluRay.x264-GROUP";
    let parsed = release::parse(name);
    let release = info(name, &parsed, &[], 0, &[], None);

    let unknown = test_format(vec![test_specification(
        "FutureSpecification",
        true,
        false,
        fields(&[("value", number(1))]),
    )]);
    assert!(!unknown.matches(&release));

    let unknown_negated = test_format(vec![test_specification(
        "FutureSpecification",
        false,
        true,
        fields(&[("value", number(1))]),
    )]);
    assert!(!unknown_negated.matches(&release));

    let bad = test_format(vec![test_specification(
        "ReleaseTitleSpecification",
        true,
        false,
        fields(&[("value", string("("))]),
    )]);
    assert!(!bad.matches(&release));

    let empty = test_format(vec![test_specification(
        "ReleaseTitleSpecification",
        true,
        false,
        fields(&[("value", string("   "))]),
    )]);
    assert!(!empty.matches(&release));
}

#[test]
fn size_bounds_are_strict_at_the_lower_edge_and_inclusive_at_the_upper() {
    let name = "Movie.2019.1080p.BluRay.x264-GROUP";
    let parsed = release::parse(name);
    let format = test_format(vec![test_specification(
        "SizeSpecification",
        false,
        false,
        fields(&[("min", number(10)), ("max", number(20))]),
    )]);
    let matches_size = |size_bytes: i64| {
        let release = info(name, &parsed, &[], size_bytes, &[], None);
        format.matches(&release)
    };
    assert!(!matches_size(10 * GIB));
    assert!(matches_size(10 * GIB + 1));
    assert!(matches_size(20 * GIB));
    assert!(!matches_size(20 * GIB + 1));
    assert!(!matches_size(0));
}

#[test]
fn year_bounds_are_inclusive_and_fall_back_to_the_movie_year() {
    let format = test_format(vec![test_specification(
        "YearSpecification",
        false,
        false,
        fields(&[("min", number(2000)), ("max", number(2010))]),
    )]);
    let matches_year = |name: &str, movie_year: Option<i32>| {
        let parsed = release::parse(name);
        let release = info(name, &parsed, &[], 0, &[], movie_year);
        format.matches(&release)
    };
    assert!(matches_year("Movie.2000.1080p.BluRay.x264-GROUP", None));
    assert!(matches_year("Movie.2010.1080p.BluRay.x264-GROUP", None));
    assert!(!matches_year("Movie.1999.1080p.BluRay.x264-GROUP", None));
    assert!(!matches_year("Movie.2011.1080p.BluRay.x264-GROUP", None));
    assert!(matches_year("Movie.1080p.BluRay.x264-GROUP", Some(2005)));
    assert!(!matches_year("Movie.1080p.BluRay.x264-GROUP", None));
    assert!(!matches_year(
        "Movie.2019.1080p.BluRay.x264-GROUP",
        Some(2005)
    ));
}

#[test]
fn regexes_are_case_insensitive() {
    let format = test_format(vec![test_specification(
        "ReleaseTitleSpecification",
        true,
        false,
        fields(&[("value", string("x265"))]),
    )]);
    let name = "Movie.2019.1080p.BluRay.X265-GROUP";
    let parsed = release::parse(name);
    let release = info(name, &parsed, &[], 0, &[], None);
    assert!(format.matches(&release));
}

#[test]
fn regexes_against_a_missing_group_or_edition_do_not_match() {
    let name = "Movie.2019.1080p.BluRay.x264";
    let parsed = release::parse(name);
    assert_eq!(parsed.release_group, None);
    assert_eq!(parsed.edition, None);
    let release = info(name, &parsed, &[], 0, &[], None);
    let group = test_format(vec![test_specification(
        "ReleaseGroupSpecification",
        true,
        false,
        fields(&[("value", string(".*"))]),
    )]);
    let edition = test_format(vec![test_specification(
        "EditionSpecification",
        true,
        false,
        fields(&[("value", string(".*"))]),
    )]);
    assert!(!group.matches(&release));
    assert!(!edition.matches(&release));
}
