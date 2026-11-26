use std::collections::HashMap;

use super::{Candidate, Context, Existing, Movie, Rejection as R, evaluate, rank};
use crate::profile::{Item, Profile};
use crate::quality::{Quality, SizeLimits};
use crate::torznab::Release;

const MB: u64 = 1024 * 1024;

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
        }
    }

    fn rejections(&self, release: &Release, tags: &[i32]) -> Vec<R> {
        let candidate = Candidate {
            release,
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
        release: &release,
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
        release,
        indexer_priority: *priority,
        indexer_tags: &[],
    });
    rank(&fixture.ctx(), list)
        .into_iter()
        .map(|decision| decision.candidate.release.title.clone())
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
                    release: &web,
                    indexer_priority: 10,
                    indexer_tags: &[],
                },
            ),
            (
                "indexer-bluray",
                Candidate {
                    release: &bluray,
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
