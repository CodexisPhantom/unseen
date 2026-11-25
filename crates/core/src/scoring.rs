//! Release rejection and scoring against a quality profile, as specified in
//! `docs/07-release-scoring.md` ("Rejection rules", "Scoring", "Upgrade
//! rules").
//!
//! Not in this module yet: the blocklist rule and custom formats (so the
//! custom format score is 0 for every release and its rules and its place in
//! the comparison do not apply).

use std::cmp::Ordering;
use std::collections::HashMap;

use crate::profile::{Item, LANGUAGE_ANY, LANGUAGE_ORIGINAL, Profile};
use crate::quality::{Quality, SizeCheck, SizeLimits};
use crate::release::{self, Language, ParsedRelease};
use crate::torznab::Release;

/// The movie a release is for.
#[derive(Debug, Clone, Copy)]
pub struct Movie<'a> {
    pub title: &'a str,
    pub year: Option<u16>,
    pub runtime_minutes: Option<u32>,
    /// ISO 639-1 code of the original language, such as `en`.
    pub original_language: Option<&'a str>,
    /// Ids of the tags of the movie.
    pub tags: &'a [i32],
}

/// The file the movie already has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Existing {
    pub quality: Quality,
    /// Revision of the file: 1 for a first release, 2 for a proper or repack.
    pub version: u8,
}

/// Everything a decision depends on besides the release itself.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    pub movie: Movie<'a>,
    pub profile: &'a Profile,
    pub upgrade_allowed: bool,
    /// Size limits per quality. A quality without an entry has no limits.
    pub size_limits: &'a HashMap<Quality, SizeLimits>,
    pub existing: Option<Existing>,
}

/// A release found by an indexer, with the facts of that indexer.
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    pub release: &'a Release,
    /// 1 to 50. A lower number wins.
    pub indexer_priority: i32,
    pub indexer_tags: &'a [i32],
}

/// Why a release is rejected. The variants are in the order of the rules in
/// docs/07.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rejection {
    /// The title or the year does not match the movie.
    WrongMovie,
    /// The quality is not allowed in the profile.
    QualityNotAllowed,
    /// The size is below the minimum of the quality definition.
    TooSmall,
    /// The size is above the maximum of the quality definition.
    TooLarge,
    /// The language does not satisfy the language of the profile.
    Language,
    /// The movie has a file and the profile does not allow upgrades.
    UpgradesDisabled,
    /// The existing file meets the cutoff.
    CutoffMet,
    /// The release is not better than the existing file.
    NotAnUpgrade,
    /// The indexer has tags and shares none with the movie.
    IndexerTags,
}

/// The verdict on one release.
#[derive(Debug, Clone)]
pub struct Decision<'a> {
    pub candidate: Candidate<'a>,
    pub parsed: ParsedRelease,
    pub quality: Quality,
    /// Empty when the release is accepted, else every rule it breaks.
    pub rejections: Vec<Rejection>,
    /// Position of the quality in the profile, higher is better. `None` when
    /// the profile does not list the quality.
    rank: Option<usize>,
    /// Bytes between the size and the preferred size, when both are known.
    preferred_distance: Option<u64>,
}

impl Decision<'_> {
    pub fn accepted(&self) -> bool {
        self.rejections.is_empty()
    }
}

/// The Radarr quality of a parsed release name.
pub fn quality_of(parsed: &ParsedRelease) -> Quality {
    let name = release::quality_name(parsed.source, parsed.modifier, parsed.resolution);
    Quality::ALL
        .into_iter()
        .find(|quality| quality.name() == name)
        .unwrap_or(Quality::Unknown)
}

/// Position of the item that holds `quality`, with its allowed flag.
fn find_item(profile: &Profile, quality: Quality) -> Option<(usize, bool)> {
    profile
        .items
        .iter()
        .enumerate()
        .find_map(|(index, item)| match item {
            Item::Quality {
                quality_id,
                allowed,
            } if *quality_id == quality.id() => Some((index, *allowed)),
            Item::Group {
                quality_ids,
                allowed,
                ..
            } if quality_ids.contains(&quality.id()) => Some((index, *allowed)),
            _ => None,
        })
}

/// Position of the item the cutoff names.
fn cutoff_rank(profile: &Profile) -> Option<usize> {
    profile.items.iter().position(|item| match item {
        Item::Quality { quality_id, .. } => *quality_id == profile.cutoff_quality_id,
        Item::Group { id, .. } => *id == profile.cutoff_quality_id,
    })
}

fn normalize(title: &str) -> String {
    title
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn same_movie(movie: &Movie, parsed: &ParsedRelease) -> bool {
    normalize(&parsed.title) == normalize(movie.title)
        && match (movie.year, parsed.year) {
            (Some(expected), Some(found)) => expected == found,
            _ => true,
        }
}

fn language_of_code(code: &str) -> Option<Language> {
    Some(match code {
        "en" => Language::English,
        "fr" => Language::French,
        "de" => Language::German,
        "es" => Language::Spanish,
        "it" => Language::Italian,
        "nl" => Language::Dutch,
        "pt" => Language::Portuguese,
        "ru" => Language::Russian,
        "ja" => Language::Japanese,
        "ko" => Language::Korean,
        "zh" => Language::Chinese,
        "sv" => Language::Swedish,
        "no" | "nb" => Language::Norwegian,
        "da" => Language::Danish,
        "fi" => Language::Finnish,
        "pl" => Language::Polish,
        "hi" => Language::Hindi,
        _ => return None,
    })
}

/// A name without a language tag counts as the original language of the
/// movie. A `MULTI` release satisfies any language. A language code that the
/// parser has no tag for cannot be matched by a tagged release.
fn language_ok(profile: &Profile, movie: &Movie, parsed: &ParsedRelease) -> bool {
    let wanted = match profile.language.as_str() {
        LANGUAGE_ANY => return true,
        LANGUAGE_ORIGINAL => movie.original_language,
        code => Some(code),
    };
    if parsed.languages.is_empty() {
        // The release is in the original language. Unknown: give the doubt.
        return match (wanted, movie.original_language) {
            (Some(wanted), Some(original)) => wanted == original,
            _ => true,
        };
    }
    parsed.languages.contains(&Language::Multi)
        || wanted
            .and_then(language_of_code)
            .is_some_and(|language| parsed.languages.contains(&language))
}

/// Whether the release replaces the existing file, as in the "Upgrade rules"
/// of docs/07. Equal quality and revision is not an upgrade.
fn is_upgrade(ctx: &Context, existing: Existing, rank: usize, version: u8) -> bool {
    if !ctx.upgrade_allowed {
        return false;
    }
    let Some((current, _)) = find_item(ctx.profile, existing.quality) else {
        // The profile dropped the quality of the file: anything allowed is
        // better.
        return true;
    };
    match rank.cmp(&current) {
        Ordering::Greater => cutoff_rank(ctx.profile).is_none_or(|cutoff| current < cutoff),
        Ordering::Equal => version > existing.version,
        Ordering::Less => false,
    }
}

/// Judges one release: parses it and lists every rule it breaks.
pub fn evaluate<'a>(ctx: &Context<'a>, candidate: Candidate<'a>) -> Decision<'a> {
    let release = candidate.release;
    let parsed = release::parse(&release.title);
    let quality = quality_of(&parsed);
    let item = find_item(ctx.profile, quality);
    let rank = item.map(|(index, _)| index);
    let limits = ctx.size_limits.get(&quality);
    let mut rejections = Vec::new();

    if !same_movie(&ctx.movie, &parsed) {
        rejections.push(Rejection::WrongMovie);
    }
    let allowed = item.is_some_and(|(_, allowed)| allowed);
    if !allowed {
        rejections.push(Rejection::QualityNotAllowed);
    }
    if let (Some(limits), Some(size)) = (limits, release.size) {
        match limits.check(size, ctx.movie.runtime_minutes) {
            SizeCheck::Within => {}
            SizeCheck::TooSmall => rejections.push(Rejection::TooSmall),
            SizeCheck::TooLarge => rejections.push(Rejection::TooLarge),
        }
    }
    if !language_ok(ctx.profile, &ctx.movie, &parsed) {
        rejections.push(Rejection::Language);
    }
    if let (Some(existing), Some(rank), true) = (ctx.existing, rank, allowed) {
        if !ctx.upgrade_allowed {
            rejections.push(Rejection::UpgradesDisabled);
        } else if !is_upgrade(ctx, existing, rank, parsed.version) {
            let met = find_item(ctx.profile, existing.quality)
                .zip(cutoff_rank(ctx.profile))
                .is_some_and(|((current, _), cutoff)| current >= cutoff);
            // Below the cutoff a lower or equal quality is simply no gain;
            // at the cutoff the file is already as good as the profile asks.
            rejections.push(if met {
                Rejection::CutoffMet
            } else {
                Rejection::NotAnUpgrade
            });
        }
    }
    if !candidate.indexer_tags.is_empty()
        && !candidate
            .indexer_tags
            .iter()
            .any(|tag| ctx.movie.tags.contains(tag))
    {
        rejections.push(Rejection::IndexerTags);
    }

    let preferred_distance = limits
        .and_then(|limits| limits.preferred_bytes(ctx.movie.runtime_minutes))
        .zip(release.size)
        .map(|(preferred, size)| (size as f64 - preferred).abs() as u64);
    Decision {
        candidate,
        parsed,
        quality,
        rejections,
        rank,
        preferred_distance,
    }
}

/// Seeders on a log scale, so 40 and 45 seeders count as equal.
fn seeder_bucket(seeders: Option<u32>) -> u32 {
    (f64::from(seeders.unwrap_or(0)) + 1.0).log10().round() as u32
}

/// `Less` when `a` is the better release: accepted before rejected, then
/// quality rank, revision, indexer priority, seeders and size (docs/07,
/// "Scoring"). The title and the guid end every tie, so the order is total.
fn compare(a: &Decision, b: &Decision) -> Ordering {
    let (ra, rb) = (a.candidate.release, b.candidate.release);
    let size = |x: &Decision, y: &Decision| match (x.preferred_distance, y.preferred_distance) {
        // Closest to the preferred size; on a tie the larger release.
        (Some(dx), Some(dy)) => dx
            .cmp(&dy)
            .then(y.candidate.release.size.cmp(&x.candidate.release.size)),
        // No preferred size: larger wins.
        _ => y.candidate.release.size.cmp(&x.candidate.release.size),
    };
    b.accepted()
        .cmp(&a.accepted())
        .then(b.rank.cmp(&a.rank))
        .then(b.parsed.version.cmp(&a.parsed.version))
        .then(
            a.candidate
                .indexer_priority
                .cmp(&b.candidate.indexer_priority),
        )
        .then(seeder_bucket(rb.seeders).cmp(&seeder_bucket(ra.seeders)))
        .then_with(|| size(a, b))
        .then_with(|| ra.title.cmp(&rb.title))
        .then_with(|| ra.guid.cmp(&rb.guid))
}

/// Judges and orders releases, best first. Accepted releases come first;
/// rejected ones follow, with their reasons, for manual search.
pub fn rank<'a>(
    ctx: &Context<'a>,
    candidates: impl IntoIterator<Item = Candidate<'a>>,
) -> Vec<Decision<'a>> {
    let mut decisions: Vec<_> = candidates
        .into_iter()
        .map(|candidate| evaluate(ctx, candidate))
        .collect();
    decisions.sort_by(compare);
    decisions
}

#[cfg(test)]
mod tests;
