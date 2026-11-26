use std::collections::HashMap;

use chrono::{DateTime, Duration, TimeZone, Utc};

use super::{
    Candidate, Context, Existing, Movie, Protocol, ProtocolPreference, Rejection as R, ReleaseRef,
    evaluate, rank,
};
use crate::newznab::NzbRelease;
use crate::profile::{Item, Profile};
use crate::quality::{Quality, SizeLimits};
use crate::torznab::Release;

const MB: u64 = 1024 * 1024;

fn fixed_now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 26, 12, 0, 0).unwrap()
}

fn release(title: &str, size_mb: u64, seeders: Option<u32>) -> Release {
    Release {
        title: title.to_owned(),
        guid: title.to_owned(),
        size: Some(size_mb * MB),
        download_url: None,
        magnet_url: None,
        info_hash: None,
        seeders,
        peers: None,
        published: None,
        categories: Vec::new(),
    }
}

fn nzb(title: &str, size_mb: u64, age_hours: Option<i64>) -> NzbRelease {
    let usenet_date = age_hours.map(|hours| fixed_now() - Duration::hours(hours));
    NzbRelease {
        title: title.to_owned(),
        guid: title.to_owned(),
        download_url: format!("https://nzb.example/dl/{title}"),
        size: Some(size_mb * MB),
        published: None,
        usenet_date,
        categories: Vec::new(),
        grabs: None,
        imdb_id: None,
        tmdb_id: None,
        files: None,
        poster: None,
        group: None,
        password_protected: None,
    }
}

/// HDTV-720p, the WEB 1080p group, Bluray-1080p (cutoff) and Remux-1080p
/// (not allowed), lowest first.
fn profile(language: &str) -> Profile {
    Profile {
        name: "HD".into(),
        items: vec![
            Item::Quality {
                quality_id: Quality::Hdtv720p.id(),
                allowed: true,
            },
            Item::Group {
                id: 1002,
                name: "WEB 1080p".into(),
                quality_ids: vec![Quality::WebDl1080p.id(), Quality::WebRip1080p.id()],
                allowed: true,
            },
            Item::Quality {
                quality_id: Quality::Bluray1080p.id(),
                allowed: true,
            },
            Item::Quality {
                quality_id: Quality::Remux1080p.id(),
                allowed: false,
            },
        ],
        cutoff_quality_id: Quality::Bluray1080p.id(),
        language: language.into(),
        min_upgrade_format_score: 1,
    }
}

/// 10 to 100 MB per minute, 50 preferred, for the three allowed qualities.
fn limits() -> HashMap<Quality, SizeLimits> {
    [
        Quality::Hdtv720p,
        Quality::WebDl1080p,
        Quality::WebRip1080p,
        Quality::Bluray1080p,
    ]
    .into_iter()
    .map(|quality| {
        let limits = SizeLimits {
            min: 10.0,
            preferred: Some(50.0),
            max: Some(100.0),
        };
        (quality, limits)
    })
    .collect()
}

struct Fixture {
    profile: Profile,
    upgrade_allowed: bool,
    existing: Option<Existing>,
    limits: HashMap<Quality, SizeLimits>,
    original_language: Option<&'static str>,
    movie_tags: Vec<i32>,
    protocol_preference: ProtocolPreference,
    retention_days: Option<u32>,
    now: Option<DateTime<Utc>>,
}

impl Fixture {
    fn new() -> Self {
        Self {
            profile: profile("any"),
            upgrade_allowed: true,
            existing: None,
            limits: limits(),
            original_language: Some("en"),
            movie_tags: vec![1],
            protocol_preference: ProtocolPreference::None,
            retention_days: None,
            now: Some(fixed_now()),
        }
    }

    fn ctx(&self) -> Context<'_> {
        Context {
            movie: Movie {
                title: "The Matrix",
                year: Some(1999),
                runtime_minutes: Some(100),
                original_language: self.original_language,
                tags: &self.movie_tags,
            },
            profile: &self.profile,
            upgrade_allowed: self.upgrade_allowed,
            size_limits: &self.limits,
            existing: self.existing,
            protocol_preference: self.protocol_preference,
            retention_days: self.retention_days,
            now: self.now,
        }
    }

    fn rejections<'a>(&self, release: impl Into<ReleaseRef<'a>>, tags: &[i32]) -> Vec<R> {
        let candidate = Candidate {
            release: release.into(),
            indexer_priority: 25,
            indexer_tags: tags,
        };
        evaluate(&self.ctx(), candidate).rejections
    }
}

const GOOD: &str = "The.Matrix.1999.1080p.BluRay.x264-SPARKS";
const FRENCH: &str = "The.Matrix.1999.FRENCH.1080p.BluRay.x264-A";

#[test]
fn a_matching_release_is_accepted() {
    let fixture = Fixture::new();
    assert_eq!(fixture.rejections(&release(GOOD, 5000, None), &[]), vec![]);
}

type Tweak = fn(&mut Fixture);

/// Name, release title, size in MB, fixture change, indexer tags, expected.
type Case<'a> = (&'a str, &'a str, u64, Tweak, &'a [i32], Vec<R>);

fn nothing(_: &mut Fixture) {}

#[test]
fn each_rejection_reason() {
    let other_title = "The.Matrix.Reloaded.2003.1080p.BluRay.x264-A";
    let other_year = "The.Matrix.2003.1080p.BluRay.x264-A";
    let dvd = "The.Matrix.1999.480p.DVDRip.XviD-A";
    let remux = "The.Matrix.1999.1080p.BluRay.REMUX.AVC-A";
    let multi = "The.Matrix.1999.MULTI.1080p.BluRay.x264-A";
    let cases: Vec<Case> = vec![
        (
            "other title",
            other_title,
            5000,
            nothing,
            &[],
            vec![R::WrongMovie],
        ),
        (
            "other year",
            other_year,
            5000,
            nothing,
            &[],
            vec![R::WrongMovie],
        ),
        (
            "case and punctuation are ignored",
            "the matrix (1999) [1080p] [BluRay]",
            5000,
            nothing,
            &[],
            vec![],
        ),
        (
            "quality not in profile",
            dvd,
            5000,
            nothing,
            &[],
            vec![R::QualityNotAllowed],
        ),
        (
            "quality listed but not allowed",
            remux,
            5000,
            nothing,
            &[],
            vec![R::QualityNotAllowed],
        ),
        (
            "below the minimum",
            GOOD,
            999,
            nothing,
            &[],
            vec![R::TooSmall],
        ),
        ("at the minimum", GOOD, 1000, nothing, &[], vec![]),
        ("at the maximum", GOOD, 10_000, nothing, &[], vec![]),
        (
            "above the maximum",
            GOOD,
            10_001,
            nothing,
            &[],
            vec![R::TooLarge],
        ),
        (
            "quality without limits",
            GOOD,
            1,
            |f| f.limits.clear(),
            &[],
            vec![],
        ),
        (
            "tagged language is wrong",
            FRENCH,
            5000,
            |f| f.profile.language = "en".into(),
            &[],
            vec![R::Language],
        ),
        (
            "tagged language is right",
            FRENCH,
            5000,
            |f| f.profile.language = "fr".into(),
            &[],
            vec![],
        ),
        (
            "untagged counts as original",
            GOOD,
            5000,
            |f| f.profile.language = "fr".into(),
            &[],
            vec![R::Language],
        ),
        (
            "untagged in the original language",
            GOOD,
            5000,
            |f| f.profile.language = "en".into(),
            &[],
            vec![],
        ),
        (
            "untagged with profile original",
            GOOD,
            5000,
            |f| f.profile.language = "original".into(),
            &[],
            vec![],
        ),
        (
            "multi satisfies any language",
            multi,
            5000,
            |f| f.profile.language = "fr".into(),
            &[],
            vec![],
        ),
        (
            "tagged, profile original",
            FRENCH,
            5000,
            |f| f.profile.language = "original".into(),
            &[],
            vec![R::Language],
        ),
        (
            "indexer tags share none",
            GOOD,
            5000,
            nothing,
            &[2, 3],
            vec![R::IndexerTags],
        ),
        (
            "indexer tags share one",
            GOOD,
            5000,
            nothing,
            &[2, 1],
            vec![],
        ),
    ];
    for (name, title, size, tweak, tags, expected) in cases {
        let mut fixture = Fixture::new();
        tweak(&mut fixture);
        let actual = fixture.rejections(&release(title, size, None), tags);
        assert_eq!(actual, expected, "{name}");
    }
}

#[test]
fn a_movie_without_a_runtime_skips_the_size_check() {
    let fixture = Fixture::new();
    let mut ctx = fixture.ctx();
    ctx.movie.runtime_minutes = None;
    let release = release(GOOD, 1, None);
    let candidate = Candidate {
        release: (&release).into(),
        indexer_priority: 25,
        indexer_tags: &[],
    };
    assert_eq!(evaluate(&ctx, candidate).rejections, vec![]);
}

#[test]
fn every_broken_rule_is_listed_in_the_order_of_the_doc() {
    let fixture = Fixture::new();
    let wrong = release("Other.Film.2003.480p.DVDRip.XviD-A", 1, None);
    assert_eq!(
        fixture.rejections(&wrong, &[9]),
        vec![R::WrongMovie, R::QualityNotAllowed, R::IndexerTags]
    );
}

#[test]
fn upgrade_decisions_against_an_existing_file() {
    let existing = |quality, version| Some(Existing { quality, version });
    let hdtv = Quality::Hdtv720p;
    let web = Quality::WebDl1080p;
    let bluray = Quality::Bluray1080p;
    let proper = "The.Matrix.1999.PROPER.1080p.BluRay.x264-A";
    let web_rip = "The.Matrix.1999.1080p.WEBRip.x264-A";
    let web_dl = "The.Matrix.1999.1080p.WEB-DL.x264-A";
    let hdtv_name = "The.Matrix.1999.720p.HDTV.x264-A";
    let cases = [
        ("no file", None, true, GOOD, vec![]),
        (
            "better quality below the cutoff",
            existing(hdtv, 1),
            true,
            GOOD,
            vec![],
        ),
        (
            "equal to the existing file",
            existing(bluray, 1),
            true,
            GOOD,
            vec![R::CutoffMet],
        ),
        (
            "equal below the cutoff",
            existing(web, 1),
            true,
            web_dl,
            vec![R::NotAnUpgrade],
        ),
        (
            "group members are equal",
            existing(web, 1),
            true,
            web_rip,
            vec![R::NotAnUpgrade],
        ),
        (
            "lower quality",
            existing(web, 1),
            true,
            hdtv_name,
            vec![R::NotAnUpgrade],
        ),
        (
            "lower quality at the cutoff",
            existing(bluray, 1),
            true,
            web_dl,
            vec![R::CutoffMet],
        ),
        (
            "upgrades off",
            existing(hdtv, 1),
            false,
            GOOD,
            vec![R::UpgradesDisabled],
        ),
        (
            "proper replaces the same quality",
            existing(bluray, 1),
            true,
            proper,
            vec![],
        ),
        (
            "same revision again",
            existing(bluray, 2),
            true,
            proper,
            vec![R::CutoffMet],
        ),
        (
            "proper below the cutoff",
            existing(web, 1),
            true,
            "The.Matrix.1999.PROPER.1080p.WEB-DL.x264-A",
            vec![],
        ),
    ];
    for (name, existing, upgrade_allowed, title, expected) in cases {
        let mut fixture = Fixture::new();
        fixture.existing = existing;
        fixture.upgrade_allowed = upgrade_allowed;
        let actual = fixture.rejections(&release(title, 5000, None), &[]);
        assert_eq!(actual, expected, "{name}");
    }
}

fn order(fixture: &Fixture, candidates: &[(Release, i32)]) -> Vec<String> {
    let list = candidates.iter().map(|(release, priority)| Candidate {
        release: release.into(),
        indexer_priority: *priority,
        indexer_tags: &[],
    });
    rank(&fixture.ctx(), list)
        .into_iter()
        .map(|decision| decision.candidate.release.title().to_owned())
        .collect()
}

#[test]
fn releases_compare_by_quality_then_revision_and_rejected_ones_come_last() {
    let fixture = Fixture::new();
    let name = |tag: &str| format!("The.Matrix.1999.{tag}-A");
    let bluray = name("1080p.BluRay.x264");
    let proper = name("PROPER.1080p.BluRay.x264");
    let web = name("1080p.WEB-DL.x264");
    let hdtv = name("720p.HDTV.x264");
    let remux = name("1080p.BluRay.REMUX.AVC");
    let candidates = [
        (release(&hdtv, 5000, Some(900)), 1),
        (release(&web, 5000, Some(900)), 1),
        (release(&remux, 5000, Some(900)), 1),
        (release(&bluray, 5000, Some(10)), 25),
        (release(&proper, 5000, Some(1)), 40),
    ];
    assert_eq!(
        order(&fixture, &candidates),
        [proper, bluray, web, hdtv, remux]
    );
}

#[test]
fn priority_seeders_and_size_break_ties_in_turn() {
    let fixture = Fixture::new();
    let title = |tag: &str| format!("The.Matrix.1999.1080p.BluRay.x264-{tag}");
    // A lower priority number wins over more seeders.
    let candidates = [
        (release(&title("A"), 5000, Some(1000)), 30),
        (release(&title("B"), 5000, Some(1)), 10),
    ];
    assert_eq!(order(&fixture, &candidates), [title("B"), title("A")]);
    // Seeders count on a log scale.
    let candidates = [
        (release(&title("A"), 5000, Some(10)), 25),
        (release(&title("B"), 5000, Some(1000)), 25),
    ];
    assert_eq!(order(&fixture, &candidates), [title("B"), title("A")]);
    // 40 and 45 seeders are equal, so the size decides: closest to the
    // preferred 5000 MB wins.
    let candidates = [
        (release(&title("A"), 9000, Some(45)), 25),
        (release(&title("B"), 4000, Some(40)), 25),
        (release(&title("C"), 5200, Some(40)), 25),
    ];
    assert_eq!(
        order(&fixture, &candidates),
        [title("C"), title("B"), title("A")]
    );
}

#[test]
fn without_a_preferred_size_the_larger_release_wins() {
    let mut fixture = Fixture::new();
    for limits in fixture.limits.values_mut() {
        limits.preferred = None;
    }
    let title = |tag: &str| format!("The.Matrix.1999.1080p.BluRay.x264-{tag}");
    let candidates = [
        (release(&title("A"), 3000, None), 25),
        (release(&title("B"), 8000, None), 25),
    ];
    assert_eq!(order(&fixture, &candidates), [title("B"), title("A")]);
}

#[test]
fn the_order_does_not_depend_on_the_input_order() {
    let fixture = Fixture::new();
    let title = |tag: &str| format!("The.Matrix.1999.1080p.BluRay.x264-{tag}");
    // Equal for every criterion: only the title decides.
    let mut candidates: Vec<_> = ["C", "A", "D", "B"]
        .into_iter()
        .map(|tag| (release(&title(tag), 5000, Some(5)), 25))
        .collect();
    let expected = order(&fixture, &candidates);
    assert_eq!(expected, [title("A"), title("B"), title("C"), title("D")]);
    for _ in 0..candidates.len() {
        candidates.rotate_left(1);
        assert_eq!(order(&fixture, &candidates), expected);
    }
    candidates.reverse();
    assert_eq!(order(&fixture, &candidates), expected);
}

#[test]
fn rejection_codes_and_reasons_are_stable_and_distinct() {
    let all = [
        R::WrongMovie,
        R::QualityNotAllowed,
        R::TooSmall,
        R::TooLarge,
        R::Language,
        R::UpgradesDisabled,
        R::CutoffMet,
        R::NotAnUpgrade,
        R::IndexerTags,
        R::ProtocolNotAllowed,
        R::RetentionExceeded,
    ];
    let mut codes = std::collections::BTreeSet::new();
    for rejection in all {
        assert!(codes.insert(rejection.code()), "duplicate code");
        assert!(!rejection.reason().is_empty());
        assert_eq!(rejection.to_string(), rejection.reason());
    }
    assert_eq!(codes.len(), all.len());
}

#[test]
fn decision_exposes_ranking_factors_and_rank_entries_keeps_metadata() {
    let fixture = Fixture::new();
    let bluray = release("The.Matrix.1999.1080p.BluRay.x264-A", 5200, Some(42));
    let web = release("The.Matrix.1999.1080p.WEB-DL.x264-B", 5000, Some(100));
    let ranked = super::rank_entries(
        &fixture.ctx(),
        [
            (
                "indexer-web",
                Candidate {
                    release: (&web).into(),
                    indexer_priority: 10,
                    indexer_tags: &[],
                },
            ),
            (
                "indexer-bluray",
                Candidate {
                    release: (&bluray).into(),
                    indexer_priority: 25,
                    indexer_tags: &[],
                },
            ),
        ],
    );
    assert_eq!(ranked.len(), 2);
    assert_eq!(ranked[0].0, "indexer-bluray");
    assert_eq!(ranked[0].1.quality_rank(), Some(2));
    assert_eq!(ranked[0].1.seeder_score(), 2);
    assert_eq!(ranked[0].1.preferred_distance(), Some(200 * MB));
    assert_eq!(ranked[1].0, "indexer-web");
    assert_eq!(ranked[1].1.quality_rank(), Some(1));
    assert_eq!(ranked[1].1.seeder_score(), 2);
    assert_eq!(ranked[1].1.preferred_distance(), Some(0));
}

enum AnyRelease {
    Torrent(Release),
    Usenet(NzbRelease),
}

impl AnyRelease {
    fn as_ref(&self) -> ReleaseRef<'_> {
        match self {
            Self::Torrent(r) => r.into(),
            Self::Usenet(r) => r.into(),
        }
    }
}

fn order_mixed(fixture: &Fixture, candidates: &[(AnyRelease, i32)]) -> Vec<String> {
    let list = candidates.iter().map(|(release, priority)| Candidate {
        release: release.as_ref(),
        indexer_priority: *priority,
        indexer_tags: &[],
    });
    rank(&fixture.ctx(), list)
        .into_iter()
        .map(|decision| decision.candidate.release.title().to_owned())
        .collect()
}

#[test]
fn protocol_preference_resolution_and_round_trip() {
    use ProtocolPreference as P;
    let cases = [
        (
            "profile overrides global",
            Some(P::PreferUsenet),
            P::PreferTorrent,
            P::PreferUsenet,
        ),
        (
            "profile explicit none overrides global",
            Some(P::None),
            P::PreferUsenet,
            P::None,
        ),
        (
            "profile none inherits global",
            None,
            P::PreferTorrent,
            P::PreferTorrent,
        ),
        ("both none", None, P::None, P::None),
        (
            "profile only usenet overrides global",
            Some(P::OnlyUsenet),
            P::PreferTorrent,
            P::OnlyUsenet,
        ),
    ];
    for (name, profile_pref, global_pref, expected) in cases {
        assert_eq!(P::resolve(profile_pref, global_pref), expected, "{name}");
    }
    for pref in [
        P::None,
        P::PreferUsenet,
        P::PreferTorrent,
        P::OnlyUsenet,
        P::OnlyTorrent,
    ] {
        assert_eq!(P::parse(pref.as_str()), Some(pref));
    }
    assert_eq!(P::parse("unknown"), None);
    assert_eq!(Protocol::Torrent.as_str(), "torrent");
    assert_eq!(Protocol::Usenet.as_str(), "usenet");
}

#[test]
fn usenet_retention_and_protocol_rejection_table() {
    use ProtocolPreference as P;
    let torrent_old = {
        let mut r = release(GOOD, 5000, Some(50));
        r.published = Some(fixed_now() - Duration::days(5000));
        r
    };
    let nzb_at_limit = nzb(GOOD, 5000, Some(1000 * 24));
    let nzb_within_last_day = nzb(GOOD, 5000, Some(1000 * 24 + 23));
    let nzb_over_limit = nzb(GOOD, 5000, Some(1001 * 24));
    let nzb_unknown_age = nzb(GOOD, 5000, None);
    let nzb_future = nzb(GOOD, 5000, Some(-12));
    // usenet_date (1200 days) takes precedence over pubDate (5 days).
    let mut nzb_usenet_date_old = nzb(GOOD, 5000, Some(1200 * 24));
    nzb_usenet_date_old.published = Some(fixed_now() - Duration::days(5));
    // Falls back to pubDate when usenet_date is missing.
    let mut nzb_pubdate_fallback_old = nzb(GOOD, 5000, None);
    nzb_pubdate_fallback_old.published = Some(fixed_now() - Duration::days(1200));

    type RetentionCase<'a> = (
        &'a str,
        ReleaseRef<'a>,
        P,
        Option<u32>,
        Option<DateTime<Utc>>,
        Vec<R>,
    );
    let cases: Vec<RetentionCase<'_>> = vec![
        (
            "nzb at retention boundary is accepted",
            (&nzb_at_limit).into(),
            P::None,
            Some(1000),
            Some(fixed_now()),
            vec![],
        ),
        (
            "nzb 1000d23h is still 1000 whole days",
            (&nzb_within_last_day).into(),
            P::None,
            Some(1000),
            Some(fixed_now()),
            vec![],
        ),
        (
            "nzb over retention is rejected",
            (&nzb_over_limit).into(),
            P::None,
            Some(1000),
            Some(fixed_now()),
            vec![R::RetentionExceeded],
        ),
        (
            "retention 0 means unlimited",
            (&nzb_over_limit).into(),
            P::None,
            Some(0),
            Some(fixed_now()),
            vec![],
        ),
        (
            "retention None means unlimited",
            (&nzb_over_limit).into(),
            P::None,
            None,
            Some(fixed_now()),
            vec![],
        ),
        (
            "now None skips retention check",
            (&nzb_over_limit).into(),
            P::None,
            Some(1000),
            None,
            vec![],
        ),
        (
            "nzb with unknown age skips retention",
            (&nzb_unknown_age).into(),
            P::None,
            Some(1000),
            Some(fixed_now()),
            vec![],
        ),
        (
            "nzb with future timestamp clamps to 0",
            (&nzb_future).into(),
            P::None,
            Some(1),
            Some(fixed_now()),
            vec![],
        ),
        (
            "usenet_date overrides pubDate for retention",
            (&nzb_usenet_date_old).into(),
            P::None,
            Some(1000),
            Some(fixed_now()),
            vec![R::RetentionExceeded],
        ),
        (
            "pubDate fallback used for retention",
            (&nzb_pubdate_fallback_old).into(),
            P::None,
            Some(1000),
            Some(fixed_now()),
            vec![R::RetentionExceeded],
        ),
        (
            "torrent ignores retention limit",
            (&torrent_old).into(),
            P::None,
            Some(10),
            Some(fixed_now()),
            vec![],
        ),
        (
            "only_usenet rejects torrent",
            (&torrent_old).into(),
            P::OnlyUsenet,
            None,
            Some(fixed_now()),
            vec![R::ProtocolNotAllowed],
        ),
        (
            "only_usenet accepts nzb",
            (&nzb_at_limit).into(),
            P::OnlyUsenet,
            Some(2000),
            Some(fixed_now()),
            vec![],
        ),
        (
            "only_torrent rejects nzb",
            (&nzb_at_limit).into(),
            P::OnlyTorrent,
            None,
            Some(fixed_now()),
            vec![R::ProtocolNotAllowed],
        ),
        (
            "only_torrent accepts torrent",
            (&torrent_old).into(),
            P::OnlyTorrent,
            None,
            Some(fixed_now()),
            vec![],
        ),
        (
            "only_torrent + expired nzb lists both reasons in order",
            (&nzb_over_limit).into(),
            P::OnlyTorrent,
            Some(1000),
            Some(fixed_now()),
            vec![R::ProtocolNotAllowed, R::RetentionExceeded],
        ),
    ];

    for (name, rel, pref, retention, now, expected) in cases {
        let mut fixture = Fixture::new();
        fixture.protocol_preference = pref;
        fixture.retention_days = retention;
        fixture.now = now;
        assert_eq!(fixture.rejections(rel, &[]), expected, "{name}");
    }
}

#[test]
fn protocol_preference_chooses_preferred_protocol_when_both_are_available() {
    use ProtocolPreference as P;
    let title = |tag: &str| format!("The.Matrix.1999.1080p.BluRay.x264-{tag}");
    let web_title = "The.Matrix.1999.1080p.WEB-DL.x264-WEB";
    let proper_title = "The.Matrix.1999.PROPER.1080p.BluRay.x264-PROPER";

    // Table of (case name, preference, candidates, expected top title).
    type PreferenceCase<'a> = (&'a str, P, Vec<(AnyRelease, i32)>, String);
    let cases: Vec<PreferenceCase<'_>> = vec![
        (
            "prefer_usenet picks usenet over higher-priority torrent of same quality",
            P::PreferUsenet,
            vec![
                (
                    AnyRelease::Torrent(release(&title("TOR"), 5000, Some(500))),
                    1,
                ),
                (AnyRelease::Usenet(nzb(&title("NZB"), 5000, Some(200))), 40),
            ],
            title("NZB"),
        ),
        (
            "prefer_torrent picks torrent over higher-priority usenet of same quality",
            P::PreferTorrent,
            vec![
                (AnyRelease::Usenet(nzb(&title("NZB"), 5000, Some(1))), 1),
                (
                    AnyRelease::Torrent(release(&title("TOR"), 5000, Some(2))),
                    40,
                ),
            ],
            title("TOR"),
        ),
        (
            "higher quality still beats protocol preference",
            P::PreferUsenet,
            vec![
                (AnyRelease::Usenet(nzb(web_title, 5000, Some(1))), 1),
                (
                    AnyRelease::Torrent(release(&title("TOR"), 5000, Some(10))),
                    25,
                ),
            ],
            title("TOR"),
        ),
        (
            "higher revision still beats protocol preference",
            P::PreferUsenet,
            vec![
                (AnyRelease::Usenet(nzb(&title("NZB"), 5000, Some(1))), 1),
                (
                    AnyRelease::Torrent(release(proper_title, 5000, Some(10))),
                    25,
                ),
            ],
            proper_title.to_owned(),
        ),
        (
            "no preference lets indexer priority decide across protocols",
            P::None,
            vec![
                (AnyRelease::Usenet(nzb(&title("NZB"), 5000, Some(1))), 30),
                (
                    AnyRelease::Torrent(release(&title("TOR"), 5000, Some(10))),
                    10,
                ),
            ],
            title("TOR"),
        ),
        (
            "only_usenet accepts usenet and pushes rejected torrent last",
            P::OnlyUsenet,
            vec![
                (
                    AnyRelease::Torrent(release(proper_title, 5000, Some(999))),
                    1,
                ),
                (AnyRelease::Usenet(nzb(web_title, 5000, Some(500))), 50),
            ],
            web_title.to_owned(),
        ),
        (
            "only_torrent accepts torrent and pushes rejected usenet last",
            P::OnlyTorrent,
            vec![
                (AnyRelease::Usenet(nzb(proper_title, 5000, Some(1))), 1),
                (AnyRelease::Torrent(release(web_title, 5000, Some(1))), 50),
            ],
            web_title.to_owned(),
        ),
    ];

    for (name, pref, candidates, expected_first) in cases {
        let mut fixture = Fixture::new();
        fixture.protocol_preference = pref;
        let ordered = order_mixed(&fixture, &candidates);
        assert_eq!(ordered[0], expected_first, "{name}: got {ordered:?}");
    }
}

#[test]
fn usenet_age_replaces_seeders_and_falls_through_to_size_within_same_bucket() {
    let fixture = Fixture::new();
    let title = |tag: &str| format!("The.Matrix.1999.1080p.BluRay.x264-{tag}");

    // Newer age bucket beats older age bucket even when the older one is closer
    // to preferred size (5000 MB).
    let candidates = [
        (
            AnyRelease::Usenet(nzb(&title("OLD"), 5000, Some(24 * 60))),
            25,
        ), // bucket 3 (~60d)
        (AnyRelease::Usenet(nzb(&title("NEW"), 9000, Some(1))), 25), // bucket 0 (1h)
        (
            AnyRelease::Usenet(nzb(&title("MID"), 5000, Some(24 * 5))),
            25,
        ), // bucket 2 (5d)
        (AnyRelease::Usenet(nzb(&title("UNK"), 5000, None)), 25),    // unknown age
    ];
    assert_eq!(
        order_mixed(&fixture, &candidates),
        [title("NEW"), title("MID"), title("OLD"), title("UNK")]
    );

    // Within the same age bucket (~100d vs ~105d are both bucket 3), size decides
    // (closest to 5000 MB wins).
    let same_bucket = [
        (
            AnyRelease::Usenet(nzb(&title("FAR"), 9000, Some(24 * 100))),
            25,
        ),
        (
            AnyRelease::Usenet(nzb(&title("MID"), 4000, Some(24 * 102))),
            25,
        ),
        (
            AnyRelease::Usenet(nzb(&title("NEAR"), 5200, Some(24 * 105))),
            25,
        ),
    ];
    assert_eq!(
        order_mixed(&fixture, &same_bucket),
        [title("NEAR"), title("MID"), title("FAR")]
    );

    // Decision exposes age_hours and age_score for Usenet releases, and 0 seeder_score.
    let rel = nzb(&title("CHECK"), 5000, Some(10));
    let decision = evaluate(
        &fixture.ctx(),
        Candidate {
            release: (&rel).into(),
            indexer_priority: 25,
            indexer_tags: &[],
        },
    );
    assert_eq!(decision.protocol(), Protocol::Usenet);
    assert_eq!(decision.age_hours(), Some(10));
    assert_eq!(decision.age_score(), Some(1));
    assert_eq!(decision.seeder_score(), 0);
}

#[test]
fn mixed_protocol_order_is_transitive_and_independent_of_input_order() {
    let title = |tag: &str| format!("The.Matrix.1999.1080p.BluRay.x264-{tag}");
    for pref in [
        ProtocolPreference::None,
        ProtocolPreference::PreferUsenet,
        ProtocolPreference::PreferTorrent,
    ] {
        let mut fixture = Fixture::new();
        fixture.protocol_preference = pref;
        let mut candidates = vec![
            (
                AnyRelease::Torrent(release(&title("T1"), 4000, Some(1000))),
                25,
            ),
            (
                AnyRelease::Torrent(release(&title("T2"), 5000, Some(10))),
                25,
            ),
            (AnyRelease::Usenet(nzb(&title("U1"), 4500, Some(1))), 25),
            (
                AnyRelease::Usenet(nzb(&title("U2"), 5000, Some(24 * 200))),
                25,
            ),
        ];
        let expected = order_mixed(&fixture, &candidates);
        for _ in 0..candidates.len() {
            candidates.rotate_left(1);
            assert_eq!(order_mixed(&fixture, &candidates), expected, "{pref:?}");
        }
        candidates.reverse();
        assert_eq!(order_mixed(&fixture, &candidates), expected, "{pref:?}");
    }
}
