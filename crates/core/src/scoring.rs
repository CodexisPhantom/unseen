//! Release rejection and scoring against a quality profile, as specified in
//! `docs/07-release-scoring.md` ("Rejection rules", "Scoring", "Upgrade
//! rules").
//!
//! Not in this module yet: the blocklist rule.

use std::cmp::{Ordering, Reverse};
use std::collections::HashMap;
use std::fmt;

use chrono::{DateTime, Utc};

use crate::custom_format::{self, ScoredFormat, release_info_from_parsed, release_language_ids};
use crate::newznab::NzbRelease;
use crate::profile::{Item, LANGUAGE_ANY, LANGUAGE_ORIGINAL, Profile};
use crate::quality::{Quality, SizeCheck, SizeLimits};
use crate::release::{self, Language, ParsedRelease};
use crate::torznab::Release as TorznabRelease;

/// Download protocol of a release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Protocol {
    Torrent,
    Usenet,
}

impl Protocol {
    /// Stable lowercase identifier used in API responses and settings.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Torrent => "torrent",
            Self::Usenet => "usenet",
        }
    }
}

/// Which download protocol is preferred or allowed when comparing torrent and
/// Usenet releases.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ProtocolPreference {
    /// Neither protocol is preferred over the other.
    #[default]
    None,
    /// Prefer Usenet over torrents when quality and revision tie.
    PreferUsenet,
    /// Prefer torrents over Usenet when quality and revision tie.
    PreferTorrent,
    /// Accept only Usenet releases; reject torrents.
    OnlyUsenet,
    /// Accept only torrent releases; reject Usenet releases.
    OnlyTorrent,
}

impl ProtocolPreference {
    /// Resolves an optional per-profile preference against a global default.
    #[must_use]
    pub const fn resolve(profile: Option<Self>, global: Self) -> Self {
        match profile {
            Some(pref) => pref,
            None => global,
        }
    }

    /// Stable machine-readable identifier for API and settings storage.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::PreferUsenet => "prefer_usenet",
            Self::PreferTorrent => "prefer_torrent",
            Self::OnlyUsenet => "only_usenet",
            Self::OnlyTorrent => "only_torrent",
        }
    }

    /// Parses a stored preference string.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "none" => Some(Self::None),
            "prefer_usenet" => Some(Self::PreferUsenet),
            "prefer_torrent" => Some(Self::PreferTorrent),
            "only_usenet" => Some(Self::OnlyUsenet),
            "only_torrent" => Some(Self::OnlyTorrent),
            _ => None,
        }
    }

    /// Whether a release of `protocol` is allowed by this preference.
    #[must_use]
    pub const fn allows(self, protocol: Protocol) -> bool {
        !matches!(
            (self, protocol),
            (Self::OnlyUsenet, Protocol::Torrent) | (Self::OnlyTorrent, Protocol::Usenet)
        )
    }

    /// Whether `protocol` is the preferred protocol under this preference.
    #[must_use]
    pub const fn prefers(self, protocol: Protocol) -> bool {
        matches!(
            (self, protocol),
            (Self::PreferUsenet | Self::OnlyUsenet, Protocol::Usenet)
                | (Self::PreferTorrent | Self::OnlyTorrent, Protocol::Torrent)
        )
    }
}

/// A release from either a Torznab (torrent) or Newznab (Usenet) indexer.
#[derive(Debug, Clone, Copy)]
pub enum ReleaseRef<'a> {
    Torrent(&'a TorznabRelease),
    Usenet(&'a NzbRelease),
}

impl<'a> ReleaseRef<'a> {
    #[must_use]
    pub const fn protocol(self) -> Protocol {
        match self {
            Self::Torrent(_) => Protocol::Torrent,
            Self::Usenet(_) => Protocol::Usenet,
        }
    }

    #[must_use]
    pub fn title(self) -> &'a str {
        match self {
            Self::Torrent(r) => &r.title,
            Self::Usenet(r) => &r.title,
        }
    }

    #[must_use]
    pub fn guid(self) -> &'a str {
        match self {
            Self::Torrent(r) => &r.guid,
            Self::Usenet(r) => &r.guid,
        }
    }

    #[must_use]
    pub const fn size(self) -> Option<u64> {
        match self {
            Self::Torrent(r) => r.size,
            Self::Usenet(r) => r.size,
        }
    }

    #[must_use]
    pub const fn seeders(self) -> Option<u32> {
        match self {
            Self::Torrent(r) => r.seeders,
            Self::Usenet(_) => None,
        }
    }

    /// Posting date on Usenet (`usenetdate` falling back to `pubDate`) for
    /// NZBs, or `pubDate` for torrents.
    #[must_use]
    pub fn published(self) -> Option<DateTime<Utc>> {
        match self {
            Self::Torrent(r) => r.published,
            Self::Usenet(r) => r.posted_at(),
        }
    }

    /// Age in whole hours relative to `now`, clamped to 0 for future dates.
    #[must_use]
    pub fn age_hours(self, now: DateTime<Utc>) -> Option<u64> {
        let posted = self.published()?;
        let seconds = now.signed_duration_since(posted).num_seconds().max(0);
        Some(u64::try_from(seconds).unwrap_or(0) / 3600)
    }

    /// Age in whole days relative to `now`, clamped to 0 for future dates.
    #[must_use]
    pub fn age_days(self, now: DateTime<Utc>) -> Option<u64> {
        Some(self.age_hours(now)? / 24)
    }
}

impl<'a> From<&'a TorznabRelease> for ReleaseRef<'a> {
    fn from(release: &'a TorznabRelease) -> Self {
        Self::Torrent(release)
    }
}

impl<'a> From<&'a NzbRelease> for ReleaseRef<'a> {
    fn from(release: &'a NzbRelease) -> Self {
        Self::Usenet(release)
    }
}

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
    /// Custom format score of the existing file under this profile.
    pub custom_format_score: i32,
}

/// Everything a decision depends on besides the release itself.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    pub movie: Movie<'a>,
    pub profile: &'a Profile,
    pub upgrade_allowed: bool,
    /// Size limits per quality. A quality without an entry has no limits.
    pub size_limits: &'a HashMap<Quality, SizeLimits>,
    /// Custom formats and the scores this profile assigns to them.
    pub custom_formats: &'a [ScoredFormat],
    pub existing: Option<Existing>,
    /// Effective protocol preference (use [`ProtocolPreference::resolve`] to
    /// combine a profile override with a global default).
    pub protocol_preference: ProtocolPreference,
    /// Maximum age in days for Usenet releases (`None` or `Some(0)` means
    /// unlimited). Torrents ignore this limit.
    pub retention_days: Option<u32>,
    /// Reference timestamp used to compute release age for Usenet retention
    /// and age scoring. `None` skips age-based rules.
    pub now: Option<DateTime<Utc>>,
}

/// A release found by an indexer, with the facts of that indexer.
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    pub release: ReleaseRef<'a>,
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
    /// The custom format score is below `minFormatScore`.
    MinFormatScore,
    /// The movie has a file and the profile does not allow upgrades.
    UpgradesDisabled,
    /// The existing file meets the cutoff.
    CutoffMet,
    /// The release is not better than the existing file.
    NotAnUpgrade,
    /// The indexer has tags and shares none with the movie.
    IndexerTags,
    /// The release's protocol is disallowed by the protocol preference.
    ProtocolNotAllowed,
    /// The Usenet release is older than the configured retention limit.
    RetentionExceeded,
}

impl Rejection {
    /// Stable machine-readable code for API responses.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::WrongMovie => "wrong_movie",
            Self::QualityNotAllowed => "quality_not_allowed",
            Self::TooSmall => "size_too_small",
            Self::TooLarge => "size_too_large",
            Self::Language => "language_not_allowed",
            Self::MinFormatScore => "min_format_score",
            Self::UpgradesDisabled => "upgrades_disabled",
            Self::CutoffMet => "cutoff_met",
            Self::NotAnUpgrade => "not_an_upgrade",
            Self::IndexerTags => "indexer_tags_mismatch",
            Self::ProtocolNotAllowed => "protocol_not_allowed",
            Self::RetentionExceeded => "retention_exceeded",
        }
    }

    /// Human-readable explanation of why the release was rejected.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::WrongMovie => "Title or year does not match the movie",
            Self::QualityNotAllowed => "Quality is not allowed in the quality profile",
            Self::TooSmall => "Size is below the quality definition minimum",
            Self::TooLarge => "Size is above the quality definition maximum",
            Self::Language => "Language does not match the quality profile",
            Self::MinFormatScore => "Custom format score is below the quality profile minimum",
            Self::UpgradesDisabled => "Movie already has a file and upgrades are disabled",
            Self::CutoffMet => "Existing file already meets the quality profile cutoff",
            Self::NotAnUpgrade => "Release is not an upgrade over the existing file",
            Self::IndexerTags => "Indexer tags do not match any tag on the movie",
            Self::ProtocolNotAllowed => "Protocol is not allowed by the protocol preference",
            Self::RetentionExceeded => "Release age exceeds the configured Usenet retention limit",
        }
    }
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.reason())
    }
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
    /// Total custom format score for the profile.
    custom_format_score: i32,
    /// Whether this release's protocol is preferred by the context.
    preferred_protocol: bool,
    /// Age of the release in whole hours when a timestamp and `now` are known.
    age_hours: Option<u64>,
    /// Age bucket on a log10 scale for Usenet releases; smaller is newer.
    age_bucket: Option<u32>,
    /// Bytes between the size and the preferred size, when both are known.
    preferred_distance: Option<u64>,
}

impl Decision<'_> {
    pub fn accepted(&self) -> bool {
        self.rejections.is_empty()
    }

    /// Download protocol of the candidate release.
    #[must_use]
    pub const fn protocol(&self) -> Protocol {
        self.candidate.release.protocol()
    }

    /// Whether this release's protocol is the preferred one under the scoring
    /// context.
    #[must_use]
    pub const fn preferred_protocol(&self) -> bool {
        self.preferred_protocol
    }

    /// Position of the quality in the profile, higher is better.
    #[must_use]
    pub fn quality_rank(&self) -> Option<usize> {
        self.rank
    }

    /// Total custom format score for the profile.
    #[must_use]
    pub const fn custom_format_score(&self) -> i32 {
        self.custom_format_score
    }

    /// Seeders bucketed on a log10 scale (`round(log10(seeders + 1))`).
    /// Returns `0` for Usenet releases.
    #[must_use]
    pub fn seeder_score(&self) -> u32 {
        seeder_bucket(self.candidate.release.seeders())
    }

    /// Age of the release in whole hours, when a posting timestamp and `now`
    /// were supplied.
    #[must_use]
    pub const fn age_hours(&self) -> Option<u64> {
        self.age_hours
    }

    /// Age bucket on a log10 scale (`round(log10(age_hours + 1))`) for Usenet
    /// releases; smaller is newer. `None` for torrents or unknown timestamps.
    #[must_use]
    pub const fn age_score(&self) -> Option<u32> {
        self.age_bucket
    }

    /// Distance in bytes from the quality definition's preferred size, when
    /// both the preferred size and release size are known.
    #[must_use]
    pub fn preferred_distance(&self) -> Option<u64> {
        self.preferred_distance
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
/// of docs/07. Equal quality, score and revision is not an upgrade.
fn is_upgrade(
    ctx: &Context,
    existing: Existing,
    rank: usize,
    custom_format_score: i32,
    version: u8,
) -> bool {
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
        Ordering::Equal => {
            let score_upgrade = existing.custom_format_score < ctx.profile.cutoff_format_score
                && custom_format_score
                    >= existing
                        .custom_format_score
                        .saturating_add(ctx.profile.min_upgrade_format_score);
            let revision_upgrade =
                custom_format_score == existing.custom_format_score && version > existing.version;
            score_upgrade || revision_upgrade
        }
        Ordering::Less => false,
    }
}

/// Judges one release: parses it and lists every rule it breaks.
pub fn evaluate<'a>(ctx: &Context<'a>, candidate: Candidate<'a>) -> Decision<'a> {
    let release = candidate.release;
    let protocol = release.protocol();
    let parsed = release::parse(release.title());
    let quality = quality_of(&parsed);
    let item = find_item(ctx.profile, quality);
    let rank = item.map(|(index, _)| index);
    let limits = ctx.size_limits.get(&quality);
    let age_hours = ctx.now.and_then(|now| release.age_hours(now));
    let age_days = age_hours.map(|hours| hours / 24);
    let language_ids = release_language_ids(&parsed.languages, ctx.movie.original_language);
    let size_bytes = release
        .size()
        .and_then(|size| i64::try_from(size).ok())
        .unwrap_or(0);
    let release_info = release_info_from_parsed(
        release.title(),
        &parsed,
        &language_ids,
        size_bytes,
        &[],
        ctx.movie.year.map(i32::from),
    );
    let custom_format_score = custom_format::score_release(ctx.custom_formats, &release_info);
    let mut rejections = Vec::new();

    if !same_movie(&ctx.movie, &parsed) {
        rejections.push(Rejection::WrongMovie);
    }
    let allowed = item.is_some_and(|(_, allowed)| allowed);
    if !allowed {
        rejections.push(Rejection::QualityNotAllowed);
    }
    if let (Some(limits), Some(size)) = (limits, release.size()) {
        match limits.check(size, ctx.movie.runtime_minutes) {
            SizeCheck::Within => {}
            SizeCheck::TooSmall => rejections.push(Rejection::TooSmall),
            SizeCheck::TooLarge => rejections.push(Rejection::TooLarge),
        }
    }
    if !language_ok(ctx.profile, &ctx.movie, &parsed) {
        rejections.push(Rejection::Language);
    }
    if custom_format_score < ctx.profile.min_format_score {
        rejections.push(Rejection::MinFormatScore);
    }
    if let (Some(existing), Some(rank), true) = (ctx.existing, rank, allowed) {
        if !ctx.upgrade_allowed {
            rejections.push(Rejection::UpgradesDisabled);
        } else if !is_upgrade(ctx, existing, rank, custom_format_score, parsed.version) {
            let quality_met = find_item(ctx.profile, existing.quality)
                .zip(cutoff_rank(ctx.profile))
                .is_some_and(|((current, _), cutoff)| current >= cutoff);
            let format_met = existing.custom_format_score >= ctx.profile.cutoff_format_score;
            // Below either cutoff a non-upgrading release is simply no gain;
            // when both cutoffs are met the file is already as good as the
            // profile asks.
            rejections.push(if quality_met && format_met {
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
    if !ctx.protocol_preference.allows(protocol) {
        rejections.push(Rejection::ProtocolNotAllowed);
    }
    if protocol == Protocol::Usenet
        && let (Some(limit), Some(days)) = (ctx.retention_days.filter(|&d| d > 0), age_days)
        && days > u64::from(limit)
    {
        rejections.push(Rejection::RetentionExceeded);
    }

    let preferred_distance = limits
        .and_then(|limits| limits.preferred_bytes(ctx.movie.runtime_minutes))
        .zip(release.size())
        .map(|(preferred, size)| (size as f64 - preferred).abs() as u64);
    let age_bucket = if protocol == Protocol::Usenet {
        age_bucket(age_hours)
    } else {
        None
    };
    Decision {
        candidate,
        parsed,
        quality,
        rejections,
        rank,
        custom_format_score,
        preferred_protocol: ctx.protocol_preference.prefers(protocol),
        age_hours,
        age_bucket,
        preferred_distance,
    }
}

/// Seeders on a log scale, so 40 and 45 seeders count as equal.
fn seeder_bucket(seeders: Option<u32>) -> u32 {
    (f64::from(seeders.unwrap_or(0)) + 1.0).log10().round() as u32
}

/// Age in hours on a log10 scale, so Usenet releases of similar age count as
/// equal and fall through to size. Smaller is newer (better).
fn age_bucket(age_hours: Option<u64>) -> Option<u32> {
    age_hours.map(|hours| (hours as f64 + 1.0).log10().round() as u32)
}

/// Protocol-specific health key: Usenet compares by age bucket (smaller/newer
/// wins, known beats unknown), torrents compare by seeder bucket (higher wins).
/// Using a tagged key keeps `compare` a strict total order even when
/// `ProtocolPreference::None` mixes both protocols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ProtocolHealth {
    Usenet(u32),
    Torrent(Reverse<u32>),
}

impl Decision<'_> {
    fn protocol_health(&self) -> ProtocolHealth {
        match self.protocol() {
            Protocol::Usenet => ProtocolHealth::Usenet(self.age_bucket.unwrap_or(u32::MAX)),
            Protocol::Torrent => ProtocolHealth::Torrent(Reverse(self.seeder_score())),
        }
    }
}

/// `Less` when `a` is the better release: accepted before rejected, then
/// quality rank, custom format score, revision, protocol preference, indexer
/// priority, protocol health (seeders for torrents, age for Usenet) and size
/// (docs/07, "Scoring"). The title and the guid end every tie, so the order is
/// total.
fn compare(a: &Decision, b: &Decision) -> Ordering {
    let (ra, rb) = (a.candidate.release, b.candidate.release);
    let size = |x: &Decision, y: &Decision| match (x.preferred_distance, y.preferred_distance) {
        // Closest to the preferred size; on a tie the larger release.
        (Some(dx), Some(dy)) => dx
            .cmp(&dy)
            .then(y.candidate.release.size().cmp(&x.candidate.release.size())),
        // No preferred size: larger wins.
        _ => y.candidate.release.size().cmp(&x.candidate.release.size()),
    };
    b.accepted()
        .cmp(&a.accepted())
        .then(b.rank.cmp(&a.rank))
        .then(b.custom_format_score.cmp(&a.custom_format_score))
        .then(b.parsed.version.cmp(&a.parsed.version))
        .then(b.preferred_protocol.cmp(&a.preferred_protocol))
        .then(
            a.candidate
                .indexer_priority
                .cmp(&b.candidate.indexer_priority),
        )
        .then(a.protocol_health().cmp(&b.protocol_health()))
        .then_with(|| size(a, b))
        .then_with(|| ra.title().cmp(rb.title()))
        .then_with(|| ra.guid().cmp(rb.guid()))
}

/// Judges and orders releases, best first. Accepted releases come first;
/// rejected ones follow, with their reasons, for manual search.
pub fn rank<'a>(
    ctx: &Context<'a>,
    candidates: impl IntoIterator<Item = Candidate<'a>>,
) -> Vec<Decision<'a>> {
    rank_entries(ctx, candidates.into_iter().map(|candidate| ((), candidate)))
        .into_iter()
        .map(|((), decision)| decision)
        .collect()
}

/// Judges and orders items paired with their candidates, best first, keeping
/// caller metadata alongside each [`Decision`].
pub fn rank_entries<'a, T>(
    ctx: &Context<'a>,
    entries: impl IntoIterator<Item = (T, Candidate<'a>)>,
) -> Vec<(T, Decision<'a>)> {
    let mut decisions: Vec<_> = entries
        .into_iter()
        .map(|(meta, candidate)| (meta, evaluate(ctx, candidate)))
        .collect();
    decisions.sort_by(|(_, a), (_, b)| compare(a, b));
    decisions
}

#[cfg(test)]
mod tests;
