//! Release-name parser for TV shows (standard episodes, multi-episodes,
//! specials, split episodes, season packs, multi-season packs, daily shows,
//! and anime), specified in `docs/07-release-scoring.md` ("TV (M7)") and
//! mirroring Sonarr's parsing rules.
//!
//! The parser never fails and never panics: an unrecognised name yields a
//! result with `TvReleaseType::Unknown` and whatever quality or metadata tags
//! were found.

use std::sync::LazyLock;

use chrono::NaiveDate;
use regex::Regex;

use crate::quality::Quality;
use crate::release::{self, Audio, Codec, Hdr, Language, Modifier, Resolution, Source};

#[cfg(test)]
mod tests;

/// Maximum span expanded from a single episode or absolute range so adversarial
/// inputs cannot cause excessive allocations.
const MAX_EPISODE_SPAN: u16 = 2000;
/// Maximum span expanded for a standard `SxxExx-yy` multi-episode range.
const MAX_STANDARD_EPISODE_SPAN: u16 = 200;
/// Maximum span expanded for a multi-season range like `S01-S10`.
const MAX_SEASON_SPAN: u16 = 50;

/// Kind of TV release parsed from a release name, matching
/// `docs/07-release-scoring.md` and Sonarr's `ReleaseType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TvReleaseType {
    /// No recognisable episode, air date, absolute number, or full season pack.
    /// Also used for partial season packs and unnumbered specials, as in Sonarr.
    Unknown,
    /// A single episode (standard, special, daily, or single anime absolute).
    Episode,
    /// Two or more episodes (standard multi-episode or non-batch anime range).
    MultiEpisode,
    /// A full season pack, or an anime batch covering an absolute range.
    SeasonPack,
    /// Multiple seasons in one release (`S01-S03`), grabbed by hand only.
    MultiSeasonPack,
}

/// Everything the TV parser reads from one release name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTvRelease {
    /// Show title with separators replaced by spaces and any trailing year removed.
    pub title: String,
    /// Show premiere year extracted from the title (e.g. `Doctor Who 2005`).
    pub year: Option<u16>,
    /// Season numbers: `[3]`, `[0]` for specials, `[1, 2, 3]` for multi-season
    /// packs, or empty for purely daily or absolute-numbered releases.
    pub seasons: Vec<u16>,
    /// Episode numbers within the season.
    pub episodes: Vec<u16>,
    /// Absolute episode numbers for anime releases.
    pub absolute_episodes: Vec<u16>,
    /// Air date (`YYYY-MM-DD`) for daily shows.
    pub air_date: Option<String>,
    /// Classification of the release.
    pub release_type: TvReleaseType,
    /// True for split episodes such as `S01E01A`, `S01E01b`, or `S01E01.Part.1`.
    pub is_split_episode: bool,
    /// True for specials (`S00Exx`, `SxxE00`, `.5 [SP]`, anime `OVA`/`OVD`/
    /// `Special`/`NCOP`/`NCED`, or `Sxx.Special`).
    pub is_special: bool,
    /// True for partial season packs (`S02.Part.1`, `S07.Vol.1`, `S06.P1`).
    pub is_partial_season: bool,
    /// Part or volume number for partial seasons, split `Part N`, or multi-part
    /// daily releases.
    pub part: Option<u16>,
    /// Parsed TV quality; always satisfies `quality.is_tv()`.
    pub quality: Quality,
    /// Resolution tag, if present (excluding non-standard 360p/540p).
    pub resolution: Option<Resolution>,
    /// Source derived from the quality (never a movie-only source).
    pub source: Source,
    /// Source modifier (`None`, `RawHd`, or `Remux`).
    pub modifier: Modifier,
    /// Video codec tag, if any.
    pub codec: Option<Codec>,
    /// Audio tags, without duplicates.
    pub audio: Vec<Audio>,
    /// HDR tags, without duplicates.
    pub hdr: Vec<Hdr>,
    /// Release group (leading anime `[SubGroup]`, trailing `-GROUP`, or `[Group]`).
    pub release_group: Option<String>,
    /// 8-hex-digit CRC32 hash from anime brackets (e.g. `ABCD1234`).
    pub hash: Option<String>,
    /// The name has a `PROPER` tag.
    pub proper: bool,
    /// The name has a `REPACK` or `RERIP` tag.
    pub repack: bool,
    /// Revision: 1 for a first release, 2 for `PROPER`/`REPACK`/`v2`, etc.
    pub version: u8,
    /// Language tags; empty means unknown.
    pub languages: Vec<Language>,
    /// The name says subtitles are burned in.
    pub hardcoded_subs: bool,
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("static regex is valid")
}

// --- Pre-processing regexes ---

static MEDIA_EXT: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)\.(mkv|mp4|avi|m4v|wmv|mov|ts|m2ts|mpg|mpeg|flv|webm",
        r"|ogm|ogv|vob|iso|img|divx|xvid|wtv|3gp|rm|rmvb|asf|m2v)$",
    ))
});
static WEBSITE_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)^(?:\[\s*|\(\s*)?(?:www\.[a-z0-9-]+(?:\.[a-z0-9-]+)+",
        r"|[a-z0-9-]+\.(?:com|org|net|pl|info|me|tv|to|cc|io|xyz))",
        r"(?:\s*[\])][-. \s]*|[- ]{2,}|[-.\s]+)",
    ))
});
static WEBSITE_POSTFIX: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)(?:[-. \s]*[\[(]\s*(?:www\.)?[a-z0-9-]+\.(?:com|org|net|pl|info|me|tv|to|cc|io|xyz)\s*[\])])+$",
    )
});
static TORRENT_SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)(?:[-. \s]*\[(?:ettv|rartv|rarbg|cttv|publichd|eztv)\]|-\[rarbg\.com\])+$")
});
static LEADING_BRACKET: LazyLock<Regex> = LazyLock::new(|| re(r"^\[([^\[\]]+)\](?:[-_. ]+)?"));
static REQUEST_PREFIX: LazyLock<Regex> = LazyLock::new(|| re(r"^\[[^\[\]]+\]\[\d+\][-_. ]*"));
static TRAILING_YEAR: LazyLock<Regex> =
    LazyLock::new(|| re(r"^(.+?)(?:\s+|[- ]+)\(?\[?((?:19|20)\d{2})\]?\)?$"));
static HASH_BRACKET: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)[\[(]([0-9a-f]{8})[\])]"));
static SEASON_FOLDER_ONLY: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)^(?:season[-_. \s]*\d+|specials)$"));

// Tokens masked to ASCII spaces of identical byte length before numbering match.
static NOISE_MASK: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)\b4kto1080p\b",
        r"|\b(?:480|540|576|720|1080|1440|2160)[pi]\b",
        r"|\b(?:640x480|848x480|1280x720|1920x1080|3840x2160|4096x2160)\b",
        r"|\b[xh][. ]?26[456]\b",
        r"|\b(?:8|10)[- ]?bit\b",
        r"|\b(?:ddp?|e-?ac-?3|ac-?3|aac|flac|opus|dts)[- .]?[257][. ][01]\b",
        r"|\b[248]k\b",
    ))
});

// --- Numbering regexes (run on `masked`, where `_` and noise are ASCII spaces) ---

static REPEATED_SE: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)(?:^|[-. (\[\s])(S\d{1,4}[. ]?E\d{1,3}(?:[-. \s]+S\d{1,4}[. ]?E\d{1,3})+)(?:\b|$|[\])\-.\s])",
    )
});
static SINGLE_SE_TOKEN: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)S(\d{1,4})[. ]?E(\d{1,3})"));
static REPEATED_X: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)(?:^|[-. (\[\s])(\d{1,4}x\d{1,3}(?:[-. \s]+\d{1,4}x\d{1,3})+)(?:\b|$|[\])\-.\s])")
});
static SINGLE_X_TOKEN: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)(\d{1,4})x(\d{1,3})"));

static ANIME_ABS_THEN_SE: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)[-.\s]+(\d{2,4})(?:\.(\d))?[-.\s]+[\(\[]?",
        r"S(\d{1,2})E(\d{2,3})(?:[-Ee](\d{2,3}))?[\)\]]?(?:v\d)?(?:\b|$|[.\s\)\]])",
    ))
});
static ANIME_SE_THEN_ABS: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)(?:^|[-.\s])(?:S(\d{1,2})E|(\d{1,2})x)(\d{2,3})(?:[-Ee](\d{2,3}))?",
        r"(?:[-.\s]+|\s*\()(\d{2,4})(?:-(\d{2,4}))?\)?(?:\b|$|[.\s])",
    ))
});

static STANDARD_SE: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)(?:^|[-. (\[\s/])S(\d{1,4})(?:[-. \s]{0,3}ep?\.?[-. \s]?)(\d{1,5})",
        r"(?P<tail>(?:(?:[-. \s]{0,3}(?:ep?\.?|x)|-)\d{1,5})*)",
        r"(?:(?P<split>[a-d])(?:\b|$|[.\s\)\]-])|(?P<dec>\.5)[. \s]+\[sp\]|v\d)?",
    ))
});
static STANDARD_SE_GLUED: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)S(\d{2})E(\d{2})(?P<tail>(?:[-Ee]\d{2})*)"));
static SPLIT_PART_AFTER: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)^[-. \s]+part[-. \s]?([1-9])(?:\b|$)"));
static OF_COUNT_AFTER: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^[. \s]+of[. \s]+(\d{1,3})\b"));
static TRAILING_EP_NUM: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)(?:[-. \s]{0,3}(?:ep?\.?|x)|-)(\d{1,5})"));
static TRAILING_DATE_IN_TITLE: LazyLock<Regex> =
    LazyLock::new(|| re(r"[-. \s]+(?:19|20)\d{2}[-. \s]+[0-1]\d[-. \s]+[0-3]\d$"));

static STANDARD_X: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)(?:^|[-. (\[\s])(\d{1,2}|\d{4})x(\d{1,3})",
        r"(?P<tail>(?:(?:[-. \s]?x|-)\d{1,3})*)(?P<split>[a-d])?(?:\b|$|[.\s\)\]-])",
    ))
});
static SEASON_EPISODE_WORD: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)(?:^|[-. (\[\s])(?:season|saison|stagione|sezon|temporada)[-. \s]+(\d{1,2})",
        r"[-. \s]+(?:episode|ep\.?)[-. \s]+(\d{1,3})(?:-(\d{1,3}))?(?:\b|$)",
    ))
});
static STANDARD_S_DOT_EP: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)(?:^|[-. \s])S(\d{2})\.(\d{2,3})(?:\b|$)"));

static MULTI_SEASON_EXPLICIT: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)(?:^|[-. (\[\s])(?:complete[-. \s]+series[-. \s]+)?",
        r"(?:S|Season[-. \s]+|Saison[-. \s]+|Stagione[-. \s]+|Sezon[-. \s]+)(\d{1,2})",
        r"(?:[-. \s]{1,3}(?:S|Season[-. \s]+|Saison[-. \s]+|Stagione[-. \s]+|Sezon[-. \s]+)(\d{1,2}))+",
        r"(?:\b|$|[.\s\)\]])",
    ))
});
static MULTI_SEASON_DASH: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)(?:^|[-. (\[\s])(?:complete[-. \s]+series[-. \s]+)?",
        r"(?:S|Season[-. \s]+|Saison[-. \s]+|Stagione[-. \s]+|Sezon[-. \s]+)(\d{1,2})-(\d{1,2})",
        r"(?:\b|$|[.\s\)\]])",
    ))
});
static SEASON_NUM_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)(?:S|Season[-. \s]+|Saison[-. \s]+|Stagione[-. \s]+|Sezon[-. \s]+)(\d{1,2})")
});

static PARTIAL_SEASON: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)(?:^|[-. (\[\s])S(\d{1,2})[-. \s]+(?:(?:part|vol)\.?[-. \s]?|p)(\d{1,2})(?:\b|$)")
});

static DAILY_YMD: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?:^|[-. (\[\s])(19[5-9]\d|20\d{2})([-.\s])(\d{2})([-.\s])(\d{2})(?:\b|$|[.\s\)\]-])")
});
static DAILY_DMY_MDY: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?:^|[-. (\[\s])(\d{2})([-.\s])(\d{2})([-.\s])(19[7-9]\d|20\d{2})(?:\b|$|[.\s\)\]-])")
});
static DAILY_COMPACT8: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?:^|[-. (\[\s])(19[7-9]\d|20\d{2})(0[1-9]|1[0-2])(0[1-9]|[12]\d|3[01])(?:\b|$|[.\s\)\]-])",
    )
});
static DAILY_COMPACT6: LazyLock<Regex> =
    LazyLock::new(|| re(r"[-.\s]([1-9]\d)(0[1-9]|1[0-2])(0[1-9]|[12]\d|3[01])[-.\s]"));
static DAILY_SHORT_MONTH: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)(?:^|[-.\s])([1-2]\d|3[01]|[1-9])(?:st|nd|rd|th)[-. \s]+",
        r"(jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[-. \s]+(19[7-9]\d|20\d{2})(?:\b|$)",
    ))
});

static ANIME_SEASON_PAREN_ABS: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)[.\s]+\(Season[.\s]+(\d{1,2})\)[-. \s]+(\d{1,4})(?:\.(\d))?(?:v\d)?(?:\b|$|[.\s\)\[])")
});
static ANIME_PAREN_RANGE: LazyLock<Regex> =
    LazyLock::new(|| re(r"[.\s]+\((\d{1,4})\s*[-~]\s*(\d{1,4})\)"));
static ANIME_DASH_RANGE: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)(?:[.\s]+-[.\s]+|[.\s]+)(\d{1,4})\s*[-~]\s*(\d{1,4})(?:\b|$|[.\s\[\(])")
});
static ANIME_EP_KEYWORD: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)(?:[.\s]+-[.\s]+|[.\s]+)(?:episode[-. \s]+|episodio[-. \s]+|ep\.?[-. \s]?|#)(\d{1,4})(?:\.(\d))?(?:v\d)?(?:\b|$|[.\s\[\(])",
    )
});
static ANIME_DASH_SINGLE: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)[.\s]+-[.\s]+(\d{1,4})(?:\.(\d))?(?:v\d)?(?:\b|$|[.\s\[\(])"));
static ANIME_SPECIAL_AFTER: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)^[-. \s]+(?:special|ova|ovd)\b"));
static ANIME_OVA_ONLY: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)[.\s]+-[.\s]+(?:special|ova|ovd|ncop|nced)\b"));
static ANIME_BRACKET_EP: LazyLock<Regex> =
    LazyLock::new(|| re(r"[-.\s]+\[(\d{2,3})(?:-(\d{2,3}))?\]"));
static ANIME_BARE_TRAILING: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)(?:[.\s]+|-)(\d{2,4})(?:\.(\d))?(?:v\d)?(?:\s*[\[\(]|$)"));
static BATCH_OR_COMPLETE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(?:batch|complete)\b"));

static SINGLE_SEASON: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)(?:[-. (\[\s]+)(?:S|Season|Saison|Series|Stagione|Sezon|Temporada)",
        r"[-. \s]?(\d{1,2}|\d{4})(?:[-. \)\]\s]|$)",
    ))
});
static ANIME_FOLLOWS_SEASON: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)^[-.\s]*-[-. \s]*(?:ep\.?[-. \s]?|episode[-. \s]+)?\d{1,4}\b"));
static SPECIAL_TAG: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bspecial\b"));

static ANIME_E_3_4: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)[-. \s]+e(\d{3,4})(?:\.(\d))?(?:\b|$)"));
static ANIME_EP_RANGE: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)[-. \s]+ep?\.?(\d{1,3})-(\d{1,3})(?:\b|$)"));
static MINISERIES_E_MULTI: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)[-. \s]+e(\d{1,2})(?:-?e(\d{1,2}))+(?:\b|$)"));
static MINISERIES_PART_NUM: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)[-. \s]+part[-. \s]?(\d{1,2})(?:\b|$)"));
static MINISERIES_PART_WORD: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)[-. \s]+part[-. \s]+(one|two|three|four|five|six|seven|eight|nine)(?:\b|$)")
});
static MINISERIES_X_OF_Y: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)[-. \s]+(\d{1,2})of(\d{1,2})(?:\b|$)"));
static MINISERIES_E_SINGLE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)[-. \s]+e(\d{2})(?:\b|$)"));
static SEE_SINGLE: LazyLock<Regex> = LazyLock::new(|| re(r"[-.\s]+([1-9])([0-9]{2})(?:\b|$)"));
static SSEE_SINGLE: LazyLock<Regex> = LazyLock::new(|| re(r"[-.\s]+(\d{2})([0-9]{2})(?:\b|$)"));

// --- Quality, revision, group, and language regexes (Sonarr-aligned) ---

static TV_RAW_HD: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\braw[-_. ]?hd\b"));
static TV_MPEG2: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bmpeg[-_. ]?2\b"));
static TV_REMUX: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(?:(?:bd|uhd)[-. ]?)?remux\b"));
static TV_HR_WS: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bhr[-_. ]ws\b"));

static TV_RESOLUTION: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)\b(?:(?P<r360>360p)",
        r"|(?P<r480>480[pi]|640x480|848x480)",
        r"|(?P<r540>540p)",
        r"|(?P<r576>576[pi])",
        r"|(?P<r720>720[pi]|1280x720|960p)",
        r"|(?P<r1080>1080[pi]|1920x1080|1440p|fhd|4kto1080p)",
        r"|(?P<r2160>2160p|3840x2160|4k[-_. ](?:uhd|hevc|bd|h\.?26[56])|(?:uhd|hevc|bd|h\.?26[56])[-_. ]4k))\b",
    ))
});
static TV_ALT_RESOLUTION: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\buhd\b|\[4k\]|\b4k\b"));

static TV_SOURCE: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)(?:(?P<webrip>\b(?:web[-. ]?rip|webmux|web[-. ]?cap)\b)",
        r"|(?P<webdl>\b(?:web[-. ]?dl(?:mux)?|webdl|amazonhd|amazonsd|ituneshd|maxdomehd|netflixu?hd|webhd|hbomaxhd|disneyhd)\b",
        r"|(?:[. \[(]|^)web[. ](?:[xh][. ]?26[456]|avc|hevc|eac3|ac3|aac|ddp?(?:[. ]?\d[. ]\d)?|atmos|opus|flac|720p|1080p|2160p)",
        r"|(?:720|1080|2160)p[-. ]web(?:\b|$|[.\s\)\]])",
        r"|\b(?:amzn|atvp|dp|dsnp|nf|hmax|hulu|pmtp|pcok|stan|crav|sho|cr)[-. ]web(?:\b|$|[.\s\)\]])",
        r"|\[web\]|[\[(]web[ .]",
        r"|\s/\sweb\s/\s)",
        r"|(?P<bluray>\b(?:blu[-. ]?ray|hd[-. ]?dvd|bdmux|bd)\b)",
        r"|(?P<bdrip>\b(?:bdrip|bdlight|brrip)\b)",
        r"|(?P<hdtv>\bhdtv\b)",
        r"|(?P<dvd>\b(?:dvd(?:rip|mux)?|ntsc|pal|xvidvd)\b)",
        r"|(?P<tvsd>\b(?:ws[-. ]?dsr|dsr|pdtv|sdtv|tvrip)\b))",
    ))
});
static TV_END_UPPER_WEB: LazyLock<Regex> = LazyLock::new(|| re(r"[. ]WEB$"));
static TV_ANIME_BLURAY: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\b(?:bd|bluray)(720|1080|2160)p?\b"));
static TV_OTHER_SOURCE: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\b(?:(?P<hdtv>hd[-. ]tv)|(?P<sdtv>sd[-. ]tv))\b"));
static TV_X264_ONLY: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bx264\b"));
static TV_XVID_DIVX: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(?:xvid|divx)\b"));

static TV_VERSION: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)(?:\d[-._ ]?v(\d)(?:[-._ \])]|$)|\[v(\d)\]|\bv(\d)\b|(?:480|576|720|1080|2160)p[._ ]v(\d))",
    )
});
static TV_PROPER: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bproper(\d)?\b"));
static TV_REPACK: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(?:repack|rerip)(\d)?\b"));

static REPOST_SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)(?:-(?:rp|1|nzbgeek|\[n-z-b\]|obfuscated|scrambled|sample|pre|postbot|xpost",
        r"|rakuv[a-z0-9]*|whiterev|buymore|asrequested|alternativetorequested|gerov",
        r"|z0ids3n|chamele0n|4p|4planet|altezachen|repackpost))+$",
    ))
});
static HYPHEN_GROUP_TWO: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)-([a-z0-9]+)-([a-z0-9]+)\s*$"));
static HYPHEN_GROUP_ONE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)-([a-z0-9]+)\s*$"));
static BRACKET_GROUP_END: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\[([a-z0-9][a-z0-9 ._-]*)\]\s*$"));
static NOT_GROUP_WORD: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)^(?:dl|ray|rip|hd|x|ma|es|en|cat|eng|jap|jpn|ger|fra|fre|ita|spa|esp|rus|pol|pl",
        r"|hdrip|r|disk|bit|\d{1,2}(?:bit)?|\d{3,4}[pi]|[se]\d+|[0-9a-f]{8}",
        r"|264|265|266|x264|x265|x266|h264|h265|hevc|avc|av1|xvid|divx",
        r"|aac|ac3|eac3|dts|flac|opus|truehd|atmos|dd|ddp",
        r"|hdr|hdr10|dv|dovi|sdr|remux|proper|repack|rerip|v\d",
        r"|web|webdl|webrip|hdtv|sdtv|pdtv|dsr|dvd|dvdrip|bd|bdrip|brrip|bluray",
        r"|batch|complete|subs?|dub|dubbed|dual|multi|raw|ova|ovd|special|unrated)$",
    ))
});
static NOT_SUBGROUP_TAG: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)^(?:\d+|dolby[-. ]?vision|hdr\d*\+?|dv|dovi|\d{3,4}[pi]|4k|uhd",
        r"|hevc|avc|x26[456]|h\.?26[456]|bd|bluray|blu-ray|web|web-?dl|web-?rip|hdtv|sdtv|dvd",
        r"|remux|batch|complete|multi|dual[-. ]?audio|french|german|spanish|italian|english|raw)$",
    ))
});
static MULTI_LANG_EXTRA: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\b(?:multilang|multilanguage)\b"));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TvRes {
    Unknown,
    R360p,
    R480p,
    R540p,
    R576p,
    R720p,
    R1080p,
    R2160p,
}

impl TvRes {
    fn to_resolution(self) -> Option<Resolution> {
        match self {
            Self::R480p => Some(Resolution::R480p),
            Self::R576p => Some(Resolution::R576p),
            Self::R720p => Some(Resolution::R720p),
            Self::R1080p => Some(Resolution::R1080p),
            Self::R2160p => Some(Resolution::R2160p),
            Self::Unknown | Self::R360p | Self::R540p => None,
        }
    }
}

struct NumberingMatch {
    title_end: usize,
    tags_start: usize,
    quality_start: usize,
    seasons: Vec<u16>,
    episodes: Vec<u16>,
    absolute_episodes: Vec<u16>,
    air_date: Option<String>,
    release_type: TvReleaseType,
    is_split_episode: bool,
    is_special: bool,
    is_partial_season: bool,
    part: Option<u16>,
}

/// Parses a TV release name (standard episode, multi-episode, special, split
/// episode, season pack, multi-season pack, daily show, or anime).
///
/// Never fails and never panics.
pub fn parse(name: &str) -> ParsedTvRelease {
    let raw = name.trim();
    let ext = MEDIA_EXT.captures(raw).map(|c| c[1].to_ascii_lowercase());
    let no_ext = MEDIA_EXT.replace(raw, "");
    let work = no_ext.replace('【', "[").replace('】', "]");
    let work = WEBSITE_PREFIX.replace(&work, "");
    let work = WEBSITE_POSTFIX.replace(&work, "");
    let work = TORRENT_SUFFIX.replace(&work, "");
    let work = work.trim();

    let (leading_subgroup, body, stripped_prefix) = extract_leading_subgroup(work);
    let masked = mask_noise(body);

    let numbering = match_numbering(body, &masked, leading_subgroup.is_some());

    let raw_title = &body[..numbering.title_end];
    let (title, year) = clean_tv_title(raw_title);

    // Quality/version/tags scan starts from the numbering token onward so
    // `12v2` or `S01E01v2` is visible while show-title words like `The Web`
    // cannot trigger false source or language matches.
    let mut scan_region = body[numbering.quality_start..].to_string();
    if let Some(prefix) = stripped_prefix {
        scan_region.push(' ');
        scan_region.push_str(prefix);
    }
    let tags_region = &body[numbering.tags_start..];

    let hash = extract_hash(&scan_region);
    let (release_group, tags_no_group) =
        extract_release_group(&scan_region, tags_region, leading_subgroup);

    let (proper, repack, version) = parse_revisions(&scan_region);
    let (quality, resolution) = parse_tv_quality(&tags_no_group, &scan_region, ext.as_deref());
    debug_assert!(
        quality.is_tv(),
        "TV parser must never return movie-only quality"
    );

    let mut languages = release::languages(tags_region);
    if MULTI_LANG_EXTRA.is_match(tags_region) && !languages.contains(&Language::Multi) {
        languages.push(Language::Multi);
    }

    ParsedTvRelease {
        title,
        year,
        seasons: numbering.seasons,
        episodes: numbering.episodes,
        absolute_episodes: numbering.absolute_episodes,
        air_date: numbering.air_date,
        release_type: numbering.release_type,
        is_split_episode: numbering.is_split_episode,
        is_special: numbering.is_special,
        is_partial_season: numbering.is_partial_season,
        part: numbering.part,
        quality,
        resolution,
        source: release_source(quality.source()),
        modifier: release_modifier(quality.modifier()),
        codec: release::codec(&tags_no_group),
        audio: release::audio(&tags_no_group),
        hdr: release::hdr(&scan_region),
        release_group,
        hash,
        proper,
        repack,
        version,
        languages,
        hardcoded_subs: release::hardcoded_subs(tags_region),
    }
}

fn release_source(source: crate::quality::Source) -> Source {
    use crate::quality::Source as Q;
    match source {
        Q::Tv => Source::Tv,
        Q::Dvd => Source::Dvd,
        Q::WebDl => Source::WebDl,
        Q::WebRip => Source::WebRip,
        Q::Bluray => Source::Bluray,
        _ => Source::Unknown,
    }
}

fn release_modifier(modifier: crate::quality::Modifier) -> Modifier {
    use crate::quality::Modifier as Q;
    match modifier {
        Q::RawHd => Modifier::RawHd,
        Q::Remux => Modifier::Remux,
        _ => Modifier::None,
    }
}

/// Extracts an optional leading anime `[SubGroup]` prefix, or strips leading
/// request-id brackets / leading metadata brackets (e.g. `[Dolby Vision]`).
fn extract_leading_subgroup(work: &str) -> (Option<String>, &str, Option<&str>) {
    if let Some(m) = REQUEST_PREFIX.find(work) {
        return (None, &work[m.end()..], None);
    }
    if let Some(c) = LEADING_BRACKET.captures(work) {
        let inner = c.get(1).map_or("", |m| m.as_str()).trim();
        let rest = &work[c.get(0).map_or(0, |m| m.end())..];
        if rest.is_empty() {
            return (None, work, None);
        }
        if inner.is_empty() || NOT_SUBGROUP_TAG.is_match(inner) {
            return (None, rest, Some(inner));
        }
        return (Some(inner.to_string()), rest, None);
    }
    (None, work, None)
}

/// Replaces `_` and technical noise tokens (resolutions, codecs, bit depths,
/// audio channels, CRC32 hashes) with ASCII spaces of identical byte length so
/// all byte offsets remain valid slices into `body`.
fn mask_noise(body: &str) -> String {
    let mut bytes = body.as_bytes().to_vec();
    for b in &mut bytes {
        if *b == b'_' {
            *b = b' ';
        }
    }
    let prelim = String::from_utf8(bytes).expect("replacing ASCII '_' preserves UTF-8");
    let mut out = prelim.as_bytes().to_vec();
    for m in NOISE_MASK.find_iter(&prelim) {
        out[m.start()..m.end()].fill(b' ');
    }
    for c in HASH_BRACKET.captures_iter(&prelim) {
        let hex = &c[1];
        if !looks_like_yyyymmdd(hex) {
            let full = c.get(0).expect("group 0");
            out[full.start()..full.end()].fill(b' ');
        }
    }
    String::from_utf8(out).expect("replacing ASCII matches with spaces preserves UTF-8")
}

fn looks_like_yyyymmdd(s: &str) -> bool {
    if s.len() != 8 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let y: i32 = s[0..4].parse().unwrap_or(0);
    let m: u32 = s[4..6].parse().unwrap_or(0);
    let d: u32 = s[6..8].parse().unwrap_or(0);
    (1950..=2099).contains(&y) && NaiveDate::from_ymd_opt(y, m, d).is_some()
}

fn match_numbering(body: &str, masked: &str, has_subgroup: bool) -> NumberingMatch {
    if SEASON_FOLDER_ONLY.is_match(masked.trim()) {
        return fallback_numbering(body, masked);
    }

    if let Some(m) = match_repeated_se(masked) {
        return m;
    }
    if let Some(m) = match_anime_combined(masked, has_subgroup) {
        return m;
    }
    if let Some(m) = match_standard_se(body, masked) {
        return m;
    }
    if let Some(m) = match_standard_x(masked) {
        return m;
    }
    if let Some(m) = match_season_episode_word(masked) {
        return m;
    }
    if let Some(m) = match_standard_s_dot_ep(masked) {
        return m;
    }
    if let Some(m) = match_multi_season(masked) {
        return m;
    }
    if let Some(m) = match_partial_season(masked) {
        return m;
    }
    if let Some(m) = match_daily(masked) {
        return m;
    }
    if has_subgroup && let Some(m) = match_anime_with_subgroup_priority(masked) {
        return m;
    }
    if let Some(m) = match_single_season(masked) {
        return m;
    }
    if has_subgroup && let Some(m) = match_anime_with_subgroup_fallback(masked) {
        return m;
    }
    if let Some(m) = match_anime_without_subgroup(masked) {
        return m;
    }
    if let Some(m) = match_miniseries_and_see(body, masked) {
        return m;
    }
    if let Some(m) = match_standard_se_glued(masked) {
        return m;
    }
    fallback_numbering(body, masked)
}

fn expand_range(first: u16, last: u16, max_span: u16) -> Option<Vec<u16>> {
    if last < first || last - first > max_span {
        return None;
    }
    Some((first..=last).collect())
}

fn match_repeated_se(masked: &str) -> Option<NumberingMatch> {
    if let Some(c) = REPEATED_SE.captures(masked) {
        let span = c.get(1)?;
        let pairs: Vec<(u16, u16)> = SINGLE_SE_TOKEN
            .captures_iter(span.as_str())
            .filter_map(|p| Some((p[1].parse().ok()?, p[2].parse().ok()?)))
            .collect();
        if pairs.len() >= 2 {
            return build_repeated_match(span.start(), span.end(), &pairs);
        }
    }
    if let Some(c) = REPEATED_X.captures(masked) {
        let span = c.get(1)?;
        let pairs: Vec<(u16, u16)> = SINGLE_X_TOKEN
            .captures_iter(span.as_str())
            .filter_map(|p| Some((p[1].parse().ok()?, p[2].parse().ok()?)))
            .collect();
        if pairs.len() >= 2 {
            return build_repeated_match(span.start(), span.end(), &pairs);
        }
    }
    None
}

fn build_repeated_match(start: usize, end: usize, pairs: &[(u16, u16)]) -> Option<NumberingMatch> {
    let mut seasons: Vec<u16> = pairs.iter().map(|&(s, _)| s).collect();
    seasons.sort_unstable();
    seasons.dedup();
    let first_ep = pairs.first()?.1;
    let last_ep = pairs.last()?.1;
    let episodes = expand_range(first_ep, last_ep, MAX_STANDARD_EPISODE_SPAN)?;
    let is_special = seasons == [0] || episodes == [0];
    let release_type = if episodes.len() > 1 {
        TvReleaseType::MultiEpisode
    } else {
        TvReleaseType::Episode
    };
    Some(NumberingMatch {
        title_end: start,
        tags_start: end,
        quality_start: start,
        seasons,
        episodes,
        absolute_episodes: Vec::new(),
        air_date: None,
        release_type,
        is_split_episode: false,
        is_special,
        is_partial_season: false,
        part: None,
    })
}

fn match_anime_combined(masked: &str, has_subgroup: bool) -> Option<NumberingMatch> {
    if !has_subgroup {
        return None;
    }
    if let Some(c) = ANIME_ABS_THEN_SE.captures(masked) {
        let full = c.get(0)?;
        let abs: u16 = c[1].parse().ok()?;
        if (1900..=2099).contains(&abs) {
            return None;
        }
        let is_decimal = c.get(2).is_some();
        let season: u16 = c[3].parse().ok()?;
        let ep1: u16 = c[4].parse().ok()?;
        let ep2: u16 = c
            .get(5)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(ep1);
        let episodes = expand_range(ep1, ep2, MAX_STANDARD_EPISODE_SPAN)?;
        let absolute_episodes = if is_decimal { Vec::new() } else { vec![abs] };
        let is_special = is_decimal || season == 0 || ep1 == 0;
        let release_type = if episodes.len() > 1 {
            TvReleaseType::MultiEpisode
        } else {
            TvReleaseType::Episode
        };
        return Some(NumberingMatch {
            title_end: full.start(),
            tags_start: full.end(),
            quality_start: full.start(),
            seasons: vec![season],
            episodes,
            absolute_episodes,
            air_date: None,
            release_type,
            is_split_episode: false,
            is_special,
            is_partial_season: false,
            part: None,
        });
    }
    if let Some(c) = ANIME_SE_THEN_ABS.captures(masked) {
        let full = c.get(0)?;
        let season: u16 = c
            .get(1)
            .or_else(|| c.get(2))
            .and_then(|m| m.as_str().parse().ok())?;
        let ep1: u16 = c[3].parse().ok()?;
        let ep2: u16 = c
            .get(4)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(ep1);
        let abs1: u16 = c[5].parse().ok()?;
        let abs2: u16 = c
            .get(6)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(abs1);
        // Reject years like 2023 immediately following S01E01.
        if (1900..=2099).contains(&abs1) && c.get(6).is_none() {
            return None;
        }
        let episodes = expand_range(ep1, ep2, MAX_STANDARD_EPISODE_SPAN)?;
        let absolute_episodes = expand_range(abs1, abs2, MAX_EPISODE_SPAN)?;
        let release_type = if episodes.len() > 1 || absolute_episodes.len() > 1 {
            TvReleaseType::MultiEpisode
        } else {
            TvReleaseType::Episode
        };
        return Some(NumberingMatch {
            title_end: full.start(),
            tags_start: full.end(),
            quality_start: full.start(),
            seasons: vec![season],
            episodes,
            absolute_episodes,
            air_date: None,
            release_type,
            is_split_episode: false,
            is_special: season == 0 || ep1 == 0,
            is_partial_season: false,
            part: None,
        });
    }
    None
}

fn match_standard_se(body: &str, masked: &str) -> Option<NumberingMatch> {
    let c = STANDARD_SE.captures(masked)?;
    let full = c.get(0)?;
    let season: u16 = c[1].parse().ok()?;
    let first: u16 = c[2].parse().ok()?;
    let tail = c.name("tail").map_or("", |m| m.as_str());

    let mut last = first;
    let mut match_end = c.get(2)?.end();
    if !tail.is_empty() {
        let mut valid_tail_end = match_end;
        let tail_offset = c.name("tail")?.start();
        for tc in TRAILING_EP_NUM.captures_iter(tail) {
            let token = tc.get(0)?;
            let num_match = tc.get(1)?;
            let next: u16 = match num_match.as_str().parse() {
                Ok(n) => n,
                Err(_) => break,
            };
            let after_byte = tail_offset + token.end();
            // Reject `S07E12-31st` where digits are immediately followed by letters (except `v\d` or `e\d`).
            if let Some(&b) = masked.as_bytes().get(after_byte)
                && b.is_ascii_alphabetic()
                && !matches!(b, b'e' | b'E' | b'v' | b'V')
            {
                break;
            }
            let has_ep_prefix = token
                .as_str()
                .bytes()
                .any(|b| matches!(b, b'e' | b'E' | b'x' | b'X'));
            // Guard against bare `-2024` or `-10426008` or descending `-3`.
            if !has_ep_prefix && (next <= last || next - first > MAX_STANDARD_EPISODE_SPAN) {
                break;
            }
            if next < last || next - first > MAX_STANDARD_EPISODE_SPAN {
                break;
            }
            last = next;
            valid_tail_end = after_byte;
        }
        match_end = valid_tail_end;
    }

    let episodes = expand_range(first, last, MAX_STANDARD_EPISODE_SPAN)?;
    let mut is_split_episode = c.name("split").is_some() && episodes.len() == 1;
    let is_decimal_special = c.name("dec").is_some();
    if is_split_episode {
        match_end = c.name("split")?.end();
    } else if is_decimal_special {
        match_end = c.name("dec")?.end();
    }

    let mut part = None;
    let after = &masked[match_end..];
    if episodes.len() == 1
        && !is_split_episode
        && let Some(pc) = SPLIT_PART_AFTER.captures(after)
    {
        is_split_episode = true;
        part = pc[1].parse().ok();
        match_end += pc.get(0)?.end();
    }

    let mut final_episodes = episodes;
    let mut release_type = if final_episodes.len() > 1 {
        TvReleaseType::MultiEpisode
    } else {
        TvReleaseType::Episode
    };

    // `(S01E01-08 of 8)` covers the full season, as in Sonarr.
    if final_episodes.len() > 1
        && let Some(oc) = OF_COUNT_AFTER.captures(&masked[match_end..])
        && let Ok(total) = oc[1].parse::<u16>()
        && first == 1
        && last == total
    {
        final_episodes.clear();
        release_type = TvReleaseType::SeasonPack;
        match_end += oc.get(0)?.end();
    }

    // If a full airdate immediately precedes `SxxExx` (e.g. `Show 2016 02 25 S20E142`),
    // trim the airdate out of the title as Sonarr does.
    let mut title_end = full.start();
    if let Some(dm) = TRAILING_DATE_IN_TITLE.find(&body[..title_end]) {
        title_end = dm.start();
    }

    let is_special =
        season == 0 || (final_episodes.len() == 1 && final_episodes[0] == 0) || is_decimal_special;
    Some(NumberingMatch {
        title_end,
        tags_start: match_end,
        quality_start: full.start(),
        seasons: vec![season],
        episodes: final_episodes,
        absolute_episodes: Vec::new(),
        air_date: None,
        release_type,
        is_split_episode,
        is_special,
        is_partial_season: false,
        part,
    })
}

fn match_standard_se_glued(masked: &str) -> Option<NumberingMatch> {
    let c = STANDARD_SE_GLUED.captures(masked)?;
    let full = c.get(0)?;
    let season: u16 = c[1].parse().ok()?;
    let first: u16 = c[2].parse().ok()?;
    let mut last = first;
    let tail = c.name("tail").map_or("", |m| m.as_str());
    for tc in TRAILING_EP_NUM.captures_iter(tail) {
        if let Ok(next) = tc[1].parse::<u16>()
            && next >= last
            && next - first <= MAX_STANDARD_EPISODE_SPAN
        {
            last = next;
        }
    }
    let episodes = expand_range(first, last, MAX_STANDARD_EPISODE_SPAN)?;
    let release_type = if episodes.len() > 1 {
        TvReleaseType::MultiEpisode
    } else {
        TvReleaseType::Episode
    };
    Some(NumberingMatch {
        title_end: full.start(),
        tags_start: full.end(),
        quality_start: full.start(),
        seasons: vec![season],
        episodes,
        absolute_episodes: Vec::new(),
        air_date: None,
        release_type,
        is_split_episode: false,
        is_special: season == 0 || first == 0,
        is_partial_season: false,
        part: None,
    })
}

fn match_standard_x(masked: &str) -> Option<NumberingMatch> {
    for c in STANDARD_X.captures_iter(masked) {
        let full = c.get(0)?;
        let season: u16 = c[1].parse().ok()?;
        let first: u16 = c[2].parse().ok()?;
        if season >= 100 && !(1900..=2099).contains(&season) {
            continue;
        }
        let tail = c.name("tail").map_or("", |m| m.as_str());
        let mut last = first;
        let mut match_end = c.get(2)?.end();
        if !tail.is_empty() {
            let tail_offset = c.name("tail")?.start();
            for tc in TRAILING_EP_NUM.captures_iter(tail) {
                let Ok(next) = tc[1].parse::<u16>() else {
                    break;
                };
                if next <= last || next - first > MAX_STANDARD_EPISODE_SPAN {
                    break;
                }
                last = next;
                match_end = tail_offset + tc.get(0)?.end();
            }
        }
        let episodes = expand_range(first, last, MAX_STANDARD_EPISODE_SPAN)?;
        let is_split_episode = c.name("split").is_some() && episodes.len() == 1;
        if is_split_episode {
            match_end = c.name("split")?.end();
        }
        let release_type = if episodes.len() > 1 {
            TvReleaseType::MultiEpisode
        } else {
            TvReleaseType::Episode
        };
        return Some(NumberingMatch {
            title_end: full.start(),
            tags_start: match_end,
            quality_start: full.start(),
            seasons: vec![season],
            episodes,
            absolute_episodes: Vec::new(),
            air_date: None,
            release_type,
            is_split_episode,
            is_special: season == 0 || first == 0,
            is_partial_season: false,
            part: None,
        });
    }
    None
}

fn match_season_episode_word(masked: &str) -> Option<NumberingMatch> {
    let c = SEASON_EPISODE_WORD.captures(masked)?;
    let full = c.get(0)?;
    let season: u16 = c[1].parse().ok()?;
    let ep1: u16 = c[2].parse().ok()?;
    let ep2: u16 = c
        .get(3)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(ep1);
    let episodes = expand_range(ep1, ep2, MAX_STANDARD_EPISODE_SPAN)?;
    let release_type = if episodes.len() > 1 {
        TvReleaseType::MultiEpisode
    } else {
        TvReleaseType::Episode
    };
    Some(NumberingMatch {
        title_end: full.start(),
        tags_start: full.end(),
        quality_start: full.start(),
        seasons: vec![season],
        episodes,
        absolute_episodes: Vec::new(),
        air_date: None,
        release_type,
        is_split_episode: false,
        is_special: season == 0 || ep1 == 0,
        is_partial_season: false,
        part: None,
    })
}

fn match_standard_s_dot_ep(masked: &str) -> Option<NumberingMatch> {
    let c = STANDARD_S_DOT_EP.captures(masked)?;
    let full = c.get(0)?;
    let season: u16 = c[1].parse().ok()?;
    let ep: u16 = c[2].parse().ok()?;
    Some(NumberingMatch {
        title_end: full.start(),
        tags_start: full.end(),
        quality_start: full.start(),
        seasons: vec![season],
        episodes: vec![ep],
        absolute_episodes: Vec::new(),
        air_date: None,
        release_type: TvReleaseType::Episode,
        is_split_episode: false,
        is_special: season == 0 || ep == 0,
        is_partial_season: false,
        part: None,
    })
}

fn match_multi_season(masked: &str) -> Option<NumberingMatch> {
    if let Some(m) = MULTI_SEASON_EXPLICIT.find(masked) {
        let mut raw_seasons: Vec<u16> = SEASON_NUM_TOKEN
            .captures_iter(m.as_str())
            .filter_map(|c| c[1].parse().ok())
            .collect();
        raw_seasons.sort_unstable();
        raw_seasons.dedup();
        if raw_seasons.is_empty() {
            return None;
        }
        let seasons = if raw_seasons.len() == 2 {
            expand_range(raw_seasons[0], raw_seasons[1], MAX_SEASON_SPAN)?
        } else {
            raw_seasons
        };
        let release_type = if seasons.len() > 1 {
            TvReleaseType::MultiSeasonPack
        } else {
            TvReleaseType::SeasonPack
        };
        return Some(NumberingMatch {
            title_end: m.start(),
            tags_start: m.end(),
            quality_start: m.start(),
            seasons,
            episodes: Vec::new(),
            absolute_episodes: Vec::new(),
            air_date: None,
            release_type,
            is_split_episode: false,
            is_special: false,
            is_partial_season: false,
            part: None,
        });
    }
    if let Some(c) = MULTI_SEASON_DASH.captures(masked) {
        let full = c.get(0)?;
        let s1: u16 = c[1].parse().ok()?;
        let s2: u16 = c[2].parse().ok()?;
        if s2 <= s1 || s2 > 50 {
            return None;
        }
        // Reject if followed by `-NN` (e.g. `S01 - 01-12` won't match here anyway).
        if masked.as_bytes().get(full.end()) == Some(&b'-') {
            return None;
        }
        let seasons = expand_range(s1, s2, MAX_SEASON_SPAN)?;
        return Some(NumberingMatch {
            title_end: full.start(),
            tags_start: full.end(),
            quality_start: full.start(),
            seasons,
            episodes: Vec::new(),
            absolute_episodes: Vec::new(),
            air_date: None,
            release_type: TvReleaseType::MultiSeasonPack,
            is_split_episode: false,
            is_special: false,
            is_partial_season: false,
            part: None,
        });
    }
    None
}

fn match_partial_season(masked: &str) -> Option<NumberingMatch> {
    let c = PARTIAL_SEASON.captures(masked)?;
    let full = c.get(0)?;
    let season: u16 = c[1].parse().ok()?;
    let part: u16 = c[2].parse().ok()?;
    Some(NumberingMatch {
        title_end: full.start(),
        tags_start: full.end(),
        quality_start: full.start(),
        seasons: vec![season],
        episodes: Vec::new(),
        absolute_episodes: Vec::new(),
        air_date: None,
        release_type: TvReleaseType::Unknown,
        is_split_episode: false,
        is_special: season == 0,
        is_partial_season: true,
        part: Some(part),
    })
}

fn match_daily(masked: &str) -> Option<NumberingMatch> {
    // 1. YYYY.MM.DD / YYYY-MM-DD / YYYY MM DD (with scene-fail swap when month > 12).
    for c in DAILY_YMD.captures_iter(masked) {
        let full = c.get(0)?;
        if c[2] != c[4] {
            continue;
        }
        let year: i32 = c[1].parse().ok()?;
        let mut month: u32 = c[3].parse().ok()?;
        let mut day: u32 = c[5].parse().ok()?;
        if month > 12 && day <= 12 {
            std::mem::swap(&mut month, &mut day);
        }
        if year < 1970 || NaiveDate::from_ymd_opt(year, month, day).is_none() {
            continue;
        }
        return Some(build_daily_match(
            masked,
            full.start(),
            full.end(),
            year,
            month,
            day,
        ));
    }

    // 2. Unambiguous DD.MM.YYYY or MM.DD.YYYY (reject when both <= 12, as in Sonarr).
    for c in DAILY_DMY_MDY.captures_iter(masked) {
        let full = c.get(0)?;
        if c[2] != c[4] {
            continue;
        }
        let first: u32 = c[1].parse().ok()?;
        let second: u32 = c[3].parse().ok()?;
        let year: i32 = c[5].parse().ok()?;
        if first <= 12 && second <= 12 {
            continue;
        }
        let (month, day) = if first > 12 {
            (second, first)
        } else {
            (first, second)
        };
        if NaiveDate::from_ymd_opt(year, month, day).is_none() {
            continue;
        }
        return Some(build_daily_match(
            masked,
            full.start(),
            full.end(),
            year,
            month,
            day,
        ));
    }

    // 3. Compact 8-digit YYYYMMDD.
    for c in DAILY_COMPACT8.captures_iter(masked) {
        let full = c.get(0)?;
        let year: i32 = c[1].parse().ok()?;
        let month: u32 = c[2].parse().ok()?;
        let day: u32 = c[3].parse().ok()?;
        if NaiveDate::from_ymd_opt(year, month, day).is_none() {
            continue;
        }
        return Some(build_daily_match(
            masked,
            full.start(),
            full.end(),
            year,
            month,
            day,
        ));
    }

    // 4. Short month format (`5th Mar 2025`, `31st Jan 2025`).
    if let Some(c) = DAILY_SHORT_MONTH.captures(masked) {
        let full = c.get(0)?;
        let day: u32 = c[1].parse().ok()?;
        let month = short_month(&c[2])?;
        let year: i32 = c[3].parse().ok()?;
        if NaiveDate::from_ymd_opt(year, month, day).is_some() {
            return Some(build_daily_match(
                masked,
                full.start(),
                full.end(),
                year,
                month,
                day,
            ));
        }
    }

    // 5. Six-digit YYMMDD bounded by separators (`140722` -> `2014-07-22`).
    for c in DAILY_COMPACT6.captures_iter(masked) {
        let full = c.get(0)?;
        let yy: i32 = c[1].parse().ok()?;
        let month: u32 = c[2].parse().ok()?;
        let day: u32 = c[3].parse().ok()?;
        let year = 2000 + yy;
        if NaiveDate::from_ymd_opt(year, month, day).is_some() {
            return Some(build_daily_match(
                masked,
                full.start(),
                full.end(),
                year,
                month,
                day,
            ));
        }
    }

    None
}

fn short_month(word: &str) -> Option<u32> {
    match word.to_ascii_lowercase().as_str() {
        "jan" => Some(1),
        "feb" => Some(2),
        "mar" => Some(3),
        "apr" => Some(4),
        "may" => Some(5),
        "jun" => Some(6),
        "jul" => Some(7),
        "aug" => Some(8),
        "sep" => Some(9),
        "oct" => Some(10),
        "nov" => Some(11),
        "dec" => Some(12),
        _ => None,
    }
}

fn build_daily_match(
    masked: &str,
    start: usize,
    mut end: usize,
    year: i32,
    month: u32,
    day: u32,
) -> NumberingMatch {
    let mut part = None;
    if let Some(pc) = SPLIT_PART_AFTER.captures(&masked[end..]) {
        part = pc[1].parse().ok();
        end += pc.get(0).map_or(0, |m| m.end());
    }
    NumberingMatch {
        title_end: start,
        tags_start: end,
        quality_start: start,
        seasons: Vec::new(),
        episodes: Vec::new(),
        absolute_episodes: Vec::new(),
        air_date: Some(format!("{year:04}-{month:02}-{day:02}")),
        release_type: TvReleaseType::Episode,
        is_split_episode: false,
        is_special: false,
        is_partial_season: false,
        part,
    }
}

/// High-priority anime patterns (when a leading `[SubGroup]` is present) that
/// must run before generic single-season packs so `[Group] Show S2 - 07` and
/// `[Group] Show S01 - 01-12` are parsed as absolute episodes, as in Sonarr.
fn match_anime_with_subgroup_priority(masked: &str) -> Option<NumberingMatch> {
    // 1. `[Group] Show (Season 2) - 33`
    if let Some(c) = ANIME_SEASON_PAREN_ABS.captures(masked) {
        let full = c.get(0)?;
        let season: u16 = c[1].parse().ok()?;
        let ep: u16 = c[2].parse().ok()?;
        let is_decimal = c.get(3).is_some();
        return Some(NumberingMatch {
            title_end: full.start(),
            tags_start: full.end(),
            quality_start: full.start(),
            seasons: vec![season],
            episodes: Vec::new(),
            absolute_episodes: if is_decimal { Vec::new() } else { vec![ep] },
            air_date: None,
            release_type: TvReleaseType::Episode,
            is_split_episode: false,
            is_special: is_decimal,
            is_partial_season: false,
            part: None,
        });
    }

    // 2. `[Group] Show (01-28) [BD 1080p] [Batch]`
    if let Some(c) = ANIME_PAREN_RANGE.captures(masked) {
        let full = c.get(0)?;
        let first: u16 = c[1].parse().ok()?;
        let last: u16 = c[2].parse().ok()?;
        if first < last
            && let Some(range) = expand_range(first, last, MAX_EPISODE_SPAN)
        {
            let after = &masked[full.end()..];
            let is_batch = BATCH_OR_COMPLETE.is_match(after) || first == 1;
            return Some(NumberingMatch {
                title_end: full.start(),
                tags_start: full.end(),
                quality_start: full.start(),
                seasons: Vec::new(),
                episodes: Vec::new(),
                absolute_episodes: range,
                air_date: None,
                release_type: if is_batch {
                    TvReleaseType::SeasonPack
                } else {
                    TvReleaseType::MultiEpisode
                },
                is_split_episode: false,
                is_special: false,
                is_partial_season: false,
                part: None,
            });
        }
    }

    // 3. `[Group] Show - 05-08 [720p]` or `[Erai-raws] Show - 01 ~ 12 [1080p]`
    if let Some(m) = match_anime_dash_range(masked) {
        return Some(m);
    }

    // 4. `[Group] Show Ep01` / `[Group] Show #957` / `[Group] Show - Episode 01`
    if let Some(c) = ANIME_EP_KEYWORD.captures(masked) {
        let full = c.get(0)?;
        let ep: u16 = c[1].parse().ok()?;
        let is_decimal = c.get(2).is_some();
        return Some(NumberingMatch {
            title_end: full.start(),
            tags_start: full.end(),
            quality_start: full.start(),
            seasons: Vec::new(),
            episodes: Vec::new(),
            absolute_episodes: if is_decimal { Vec::new() } else { vec![ep] },
            air_date: None,
            release_type: TvReleaseType::Episode,
            is_split_episode: false,
            is_special: is_decimal,
            is_partial_season: false,
            part: None,
        });
    }

    // 5. `[SubsPlease] Frieren - 12 (1080p)` / `12v2` / `1085` / `12.5` / `01 - OVA`
    if let Some(m) = match_anime_dash_single(masked) {
        return Some(m);
    }

    // 6. `[Group] Show S2 [05][1080p]` (bracketed absolute episode)
    if let Some(c) = ANIME_BRACKET_EP.captures(masked) {
        let full = c.get(0)?;
        let ep1: u16 = c[1].parse().ok()?;
        let ep2: u16 = c
            .get(2)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(ep1);
        if !(1900..=2099).contains(&ep1)
            && !clean_title_simple(&masked[..full.start()]).is_empty()
            && let Some(range) = expand_range(ep1, ep2, MAX_EPISODE_SPAN)
        {
            let release_type = if range.len() > 1 {
                TvReleaseType::MultiEpisode
            } else {
                TvReleaseType::Episode
            };
            return Some(NumberingMatch {
                title_end: full.start(),
                tags_start: full.end(),
                quality_start: full.start(),
                seasons: Vec::new(),
                episodes: Vec::new(),
                absolute_episodes: range,
                air_date: None,
                release_type,
                is_split_episode: false,
                is_special: false,
                is_partial_season: false,
                part: None,
            });
        }
    }

    // 7. `[Group] Show - OVA` / `NCOP` / `NCED`
    if let Some(m) = ANIME_OVA_ONLY.find(masked) {
        return Some(NumberingMatch {
            title_end: m.start(),
            tags_start: m.end(),
            quality_start: m.start(),
            seasons: Vec::new(),
            episodes: Vec::new(),
            absolute_episodes: Vec::new(),
            air_date: None,
            release_type: TvReleaseType::Unknown,
            is_split_episode: false,
            is_special: true,
            is_partial_season: false,
            part: None,
        });
    }

    None
}

fn match_anime_dash_range(masked: &str) -> Option<NumberingMatch> {
    let mut best = None;
    for c in ANIME_DASH_RANGE.captures_iter(masked) {
        let full = c.get(0)?;
        let first: u16 = c[1].parse().ok()?;
        let last: u16 = c[2].parse().ok()?;
        if first >= last || last - first > MAX_EPISODE_SPAN {
            continue;
        }
        if clean_title_simple(&masked[..full.start()]).is_empty() {
            continue;
        }
        let after = &masked[full.end()..];
        let is_batch = BATCH_OR_COMPLETE.is_match(after);
        let range = expand_range(first, last, MAX_EPISODE_SPAN)?;
        best = Some(NumberingMatch {
            title_end: full.start(),
            tags_start: full.end(),
            quality_start: full.start(),
            seasons: Vec::new(),
            episodes: Vec::new(),
            absolute_episodes: range,
            air_date: None,
            release_type: if is_batch {
                TvReleaseType::SeasonPack
            } else {
                TvReleaseType::MultiEpisode
            },
            is_split_episode: false,
            is_special: false,
            is_partial_season: false,
            part: None,
        });
    }
    best
}

fn match_anime_dash_single(masked: &str) -> Option<NumberingMatch> {
    let candidates: Vec<_> = ANIME_DASH_SINGLE.captures_iter(masked).collect();
    if candidates.is_empty() {
        return None;
    }
    // Prefer a candidate followed by brackets/parens/end/OVA/Special (handles
    // titles containing ` - 100 Years Quest - 01 (1080p)` as well as
    // `Show - 11 - 511 Kinderheim`).
    let chosen = candidates
        .iter()
        .rfind(|c| {
            let end = c.get(0).map_or(0, |m| m.end());
            let rest = masked[end..].trim_start();
            rest.is_empty()
                || rest.starts_with('[')
                || rest.starts_with('(')
                || ANIME_SPECIAL_AFTER.is_match(&masked[end..])
        })
        .or_else(|| candidates.first())?;

    let full = chosen.get(0)?;
    if clean_title_simple(&masked[..full.start()]).is_empty() {
        return None;
    }
    let ep: u16 = chosen[1].parse().ok()?;
    let is_decimal = chosen.get(2).is_some();
    let after = &masked[full.end()..];
    let is_special = is_decimal || ANIME_SPECIAL_AFTER.is_match(after);
    Some(NumberingMatch {
        title_end: full.start(),
        tags_start: full.end(),
        quality_start: full.start(),
        seasons: Vec::new(),
        episodes: Vec::new(),
        absolute_episodes: if is_decimal { Vec::new() } else { vec![ep] },
        air_date: None,
        release_type: TvReleaseType::Episode,
        is_split_episode: false,
        is_special,
        is_partial_season: false,
        part: None,
    })
}

fn match_single_season(masked: &str) -> Option<NumberingMatch> {
    for c in SINGLE_SEASON.captures_iter(masked) {
        let full = c.get(0)?;
        if clean_title_simple(&masked[..full.start()]).is_empty() {
            continue;
        }
        let season: u16 = c[1].parse().ok()?;
        if season >= 100 && !(1950..=2099).contains(&season) {
            continue;
        }
        let after = &masked[full.end()..];
        if ANIME_FOLLOWS_SEASON.is_match(after) {
            continue;
        }
        let is_special = season == 0 || SPECIAL_TAG.is_match(after);
        let release_type = if season != 0 && SPECIAL_TAG.is_match(after) {
            TvReleaseType::Unknown
        } else {
            TvReleaseType::SeasonPack
        };
        return Some(NumberingMatch {
            title_end: full.start(),
            tags_start: full.end(),
            quality_start: full.start(),
            seasons: vec![season],
            episodes: Vec::new(),
            absolute_episodes: Vec::new(),
            air_date: None,
            release_type,
            is_split_episode: false,
            is_special,
            is_partial_season: false,
            part: None,
        });
    }
    None
}

/// Lower-priority anime patterns (with `[SubGroup]`) that run after single
/// season packs so `[Vivid] Show S01 [1080p]` matches as a season pack first.
fn match_anime_with_subgroup_fallback(masked: &str) -> Option<NumberingMatch> {
    for c in ANIME_BARE_TRAILING.captures_iter(masked) {
        let full = c.get(0)?;
        let ep: u16 = c[1].parse().ok()?;
        let is_decimal = c.get(2).is_some();
        if (1900..=2099).contains(&ep) || clean_title_simple(&masked[..full.start()]).is_empty() {
            continue;
        }
        return Some(NumberingMatch {
            title_end: full.start(),
            tags_start: full.end(),
            quality_start: full.start(),
            seasons: Vec::new(),
            episodes: Vec::new(),
            absolute_episodes: if is_decimal { Vec::new() } else { vec![ep] },
            air_date: None,
            release_type: TvReleaseType::Episode,
            is_split_episode: false,
            is_special: is_decimal,
            is_partial_season: false,
            part: None,
        });
    }
    None
}

fn match_anime_without_subgroup(masked: &str) -> Option<NumberingMatch> {
    // 1. 3- or 4-digit `E###` / `E####` (e.g. `One.Piece.E1135.GERMAN.1080p`)
    if let Some(c) = ANIME_E_3_4.captures(masked) {
        let full = c.get(0)?;
        let ep: u16 = c[1].parse().ok()?;
        let is_decimal = c.get(2).is_some();
        if !clean_title_simple(&masked[..full.start()]).is_empty() {
            return Some(NumberingMatch {
                title_end: full.start(),
                tags_start: full.end(),
                quality_start: full.start(),
                seasons: Vec::new(),
                episodes: Vec::new(),
                absolute_episodes: if is_decimal { Vec::new() } else { vec![ep] },
                air_date: None,
                release_type: TvReleaseType::Episode,
                is_split_episode: false,
                is_special: is_decimal,
                is_partial_season: false,
                part: None,
            });
        }
    }

    // 2. `Ep01-12` / `E01-12` range
    if let Some(c) = ANIME_EP_RANGE.captures(masked) {
        let full = c.get(0)?;
        let ep1: u16 = c[1].parse().ok()?;
        let ep2: u16 = c[2].parse().ok()?;
        if ep1 < ep2
            && !clean_title_simple(&masked[..full.start()]).is_empty()
            && let Some(range) = expand_range(ep1, ep2, MAX_EPISODE_SPAN)
        {
            let is_batch = BATCH_OR_COMPLETE.is_match(&masked[full.end()..]);
            return Some(NumberingMatch {
                title_end: full.start(),
                tags_start: full.end(),
                quality_start: full.start(),
                seasons: Vec::new(),
                episodes: Vec::new(),
                absolute_episodes: range,
                air_date: None,
                release_type: if is_batch {
                    TvReleaseType::SeasonPack
                } else {
                    TvReleaseType::MultiEpisode
                },
                is_split_episode: false,
                is_special: false,
                is_partial_season: false,
                part: None,
            });
        }
    }

    // 3. `Show Episode 56` / `Show EP06` / `Show Episodio 13`
    if let Some(c) = ANIME_EP_KEYWORD.captures(masked) {
        let full = c.get(0)?;
        let ep: u16 = c[1].parse().ok()?;
        let is_decimal = c.get(2).is_some();
        if !clean_title_simple(&masked[..full.start()]).is_empty() {
            return Some(NumberingMatch {
                title_end: full.start(),
                tags_start: full.end(),
                quality_start: full.start(),
                seasons: Vec::new(),
                episodes: Vec::new(),
                absolute_episodes: if is_decimal { Vec::new() } else { vec![ep] },
                air_date: None,
                release_type: TvReleaseType::Episode,
                is_split_episode: false,
                is_special: is_decimal,
                is_partial_season: false,
                part: None,
            });
        }
    }

    // 4. `Show - 01-12 [1080p]` / `Show 1-13 [720p]`
    if let Some(m) = match_anime_dash_range(masked) {
        return Some(m);
    }

    // 5. `Show - 031 - Episode Title [Lunar]` / `Show S3 - 15 [720p]`
    if let Some(m) = match_anime_dash_single(masked) {
        return Some(m);
    }

    // 6. `Show_2_[01-05]_[Group]`
    if let Some(c) = ANIME_BRACKET_EP.captures(masked) {
        let full = c.get(0)?;
        let ep1: u16 = c[1].parse().ok()?;
        let ep2: u16 = c
            .get(2)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(ep1);
        if !(1900..=2099).contains(&ep1)
            && !clean_title_simple(&masked[..full.start()]).is_empty()
            && let Some(range) = expand_range(ep1, ep2, MAX_EPISODE_SPAN)
        {
            let release_type = if range.len() > 1 {
                TvReleaseType::MultiEpisode
            } else {
                TvReleaseType::Episode
            };
            return Some(NumberingMatch {
                title_end: full.start(),
                tags_start: full.end(),
                quality_start: full.start(),
                seasons: Vec::new(),
                episodes: Vec::new(),
                absolute_episodes: range,
                air_date: None,
                release_type,
                is_split_episode: false,
                is_special: false,
                is_partial_season: false,
                part: None,
            });
        }
    }

    None
}

fn match_miniseries_and_see(body: &str, masked: &str) -> Option<NumberingMatch> {
    if let Some(c) = MINISERIES_E_MULTI.captures(masked) {
        let full = c.get(0)?;
        let ep1: u16 = c[1].parse().ok()?;
        let ep2: u16 = c[2].parse().ok()?;
        if !clean_title_simple(&masked[..full.start()]).is_empty()
            && let Some(episodes) = expand_range(ep1, ep2, MAX_STANDARD_EPISODE_SPAN)
        {
            return Some(NumberingMatch {
                title_end: full.start(),
                tags_start: full.end(),
                quality_start: full.start(),
                seasons: vec![1],
                episodes,
                absolute_episodes: Vec::new(),
                air_date: None,
                release_type: TvReleaseType::MultiEpisode,
                is_split_episode: false,
                is_special: false,
                is_partial_season: false,
                part: None,
            });
        }
    }

    if let Some(c) = MINISERIES_PART_NUM.captures(masked) {
        let full = c.get(0)?;
        let ep: u16 = c[1].parse().ok()?;
        if ep > 0 && !clean_title_simple(&masked[..full.start()]).is_empty() {
            return Some(NumberingMatch {
                title_end: full.start(),
                tags_start: full.end(),
                quality_start: full.start(),
                seasons: vec![1],
                episodes: vec![ep],
                absolute_episodes: Vec::new(),
                air_date: None,
                release_type: TvReleaseType::Episode,
                is_split_episode: false,
                is_special: false,
                is_partial_season: false,
                part: None,
            });
        }
    }

    if let Some(c) = MINISERIES_PART_WORD.captures(masked) {
        let full = c.get(0)?;
        let ep = word_number(&c[1])?;
        if !clean_title_simple(&masked[..full.start()]).is_empty() {
            return Some(NumberingMatch {
                title_end: full.start(),
                tags_start: full.end(),
                quality_start: full.start(),
                seasons: vec![1],
                episodes: vec![ep],
                absolute_episodes: Vec::new(),
                air_date: None,
                release_type: TvReleaseType::Episode,
                is_split_episode: false,
                is_special: false,
                is_partial_season: false,
                part: None,
            });
        }
    }

    if let Some(c) = MINISERIES_X_OF_Y.captures(masked) {
        let full = c.get(0)?;
        let ep: u16 = c[1].parse().ok()?;
        let total: u16 = c[2].parse().ok()?;
        if ep > 0 && ep <= total && !clean_title_simple(&masked[..full.start()]).is_empty() {
            return Some(NumberingMatch {
                title_end: full.start(),
                tags_start: full.end(),
                quality_start: full.start(),
                seasons: vec![1],
                episodes: vec![ep],
                absolute_episodes: Vec::new(),
                air_date: None,
                release_type: TvReleaseType::Episode,
                is_split_episode: false,
                is_special: false,
                is_partial_season: false,
                part: None,
            });
        }
    }

    if let Some(c) = MINISERIES_E_SINGLE.captures(masked) {
        let full = c.get(0)?;
        let ep: u16 = c[1].parse().ok()?;
        if !clean_title_simple(&masked[..full.start()]).is_empty() {
            return Some(NumberingMatch {
                title_end: full.start(),
                tags_start: full.end(),
                quality_start: full.start(),
                seasons: vec![1],
                episodes: vec![ep],
                absolute_episodes: Vec::new(),
                air_date: None,
                release_type: TvReleaseType::Episode,
                is_split_episode: false,
                is_special: false,
                is_partial_season: false,
                part: None,
            });
        }
    }

    // SEE (`103`, `113`) and SSEE (`1013`) when followed by quality/codec tags.
    for c in SEE_SINGLE.captures_iter(masked) {
        let full = c.get(0)?;
        let season: u16 = c[1].parse().ok()?;
        let ep: u16 = c[2].parse().ok()?;
        let after = &body[full.end()..];
        if ep > 0 && !clean_title_simple(&masked[..full.start()]).is_empty() && has_media_tag(after)
        {
            return Some(NumberingMatch {
                title_end: full.start(),
                tags_start: full.end(),
                quality_start: full.start(),
                seasons: vec![season],
                episodes: vec![ep],
                absolute_episodes: Vec::new(),
                air_date: None,
                release_type: TvReleaseType::Episode,
                is_split_episode: false,
                is_special: false,
                is_partial_season: false,
                part: None,
            });
        }
    }

    for c in SSEE_SINGLE.captures_iter(masked) {
        let full = c.get(0)?;
        let season: u16 = c[1].parse().ok()?;
        let ep: u16 = c[2].parse().ok()?;
        let combined = season * 100 + ep;
        let after = &body[full.end()..];
        if ep > 0
            && season > 0
            && !(1900..=2099).contains(&combined)
            && !clean_title_simple(&masked[..full.start()]).is_empty()
            && has_media_tag(after)
        {
            return Some(NumberingMatch {
                title_end: full.start(),
                tags_start: full.end(),
                quality_start: full.start(),
                seasons: vec![season],
                episodes: vec![ep],
                absolute_episodes: Vec::new(),
                air_date: None,
                release_type: TvReleaseType::Episode,
                is_split_episode: false,
                is_special: false,
                is_partial_season: false,
                part: None,
            });
        }
    }

    None
}

fn has_media_tag(s: &str) -> bool {
    TV_RESOLUTION.is_match(s)
        || TV_SOURCE.is_match(s)
        || TV_RAW_HD.is_match(s)
        || release::codec(s).is_some()
}

fn word_number(word: &str) -> Option<u16> {
    match word.to_ascii_lowercase().as_str() {
        "one" => Some(1),
        "two" => Some(2),
        "three" => Some(3),
        "four" => Some(4),
        "five" => Some(5),
        "six" => Some(6),
        "seven" => Some(7),
        "eight" => Some(8),
        "nine" => Some(9),
        _ => None,
    }
}

fn fallback_numbering(body: &str, masked: &str) -> NumberingMatch {
    let first_tag = [
        &*TV_RESOLUTION,
        &*TV_SOURCE,
        &*TV_RAW_HD,
        &*TV_PROPER,
        &*TV_REPACK,
    ]
    .iter()
    .filter_map(|rx| rx.find(body))
    .map(|m| m.start())
    .filter(|&start| start > 0)
    .min()
    .unwrap_or(masked.len());
    NumberingMatch {
        title_end: first_tag,
        tags_start: first_tag,
        quality_start: first_tag,
        seasons: Vec::new(),
        episodes: Vec::new(),
        absolute_episodes: Vec::new(),
        air_date: None,
        release_type: TvReleaseType::Unknown,
        is_split_episode: false,
        is_special: false,
        is_partial_season: false,
        part: None,
    }
}

fn clean_title_simple(raw: &str) -> String {
    raw.split(|c: char| c == '.' || c == '_' || c.is_whitespace())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .trim_start_matches([' ', '-', '(', '['])
        .trim_end_matches([' ', '-', '(', '[', '/'])
        .to_string()
}

fn clean_tv_title(raw: &str) -> (String, Option<u16>) {
    let cleaned = clean_title_simple(raw);
    if let Some(c) = TRAILING_YEAR.captures(&cleaned) {
        let prefix = c[1].trim_end_matches([' ', '-', '(', '[']);
        if !prefix.is_empty()
            && let Ok(year) = c[2].parse::<u16>()
        {
            return (prefix.to_string(), Some(year));
        }
    }
    (cleaned, None)
}

fn extract_hash(scan: &str) -> Option<String> {
    HASH_BRACKET
        .captures_iter(scan)
        .filter(|c| !looks_like_yyyymmdd(&c[1]))
        .last()
        .map(|c| c[1].to_string())
}

fn extract_release_group(
    scan: &str,
    tags_region: &str,
    leading_subgroup: Option<String>,
) -> (Option<String>, String) {
    if let Some(sub) = leading_subgroup {
        return (Some(sub), scan.to_string());
    }

    // Strip repost suffixes and trailing CRC32 hash before looking for `-GROUP` / `[Group]`.
    let cleaned_scan = REPOST_SUFFIX.replace(tags_region.trim(), "");
    let cleaned_scan = MEDIA_EXT.replace(&cleaned_scan, "");
    let cleaned_scan = cleaned_scan.trim_end_matches([' ', '-', '.']);

    // Also handle trailing language word right after `-GROUP` (`-SKGTV English`).
    let group_candidate = strip_trailing_language(cleaned_scan);

    if let Some(c) = HYPHEN_GROUP_TWO.captures(group_candidate) {
        let m = c.get(0).expect("group 0");
        let part1 = &c[1];
        let part2 = &c[2];
        let before = &group_candidate[..m.start()];
        if is_valid_hyphen_group(before, part1) && is_valid_hyphen_group(part1, part2) {
            let group = format!("{part1}-{part2}");
            let stripped = strip_group_from_scan(scan, m.as_str());
            return (Some(group), stripped);
        }
    }

    if let Some(c) = HYPHEN_GROUP_ONE.captures(group_candidate) {
        let m = c.get(0).expect("group 0");
        let group = &c[1];
        let before = &group_candidate[..m.start()];
        if is_valid_hyphen_group(before, group) {
            let stripped = strip_group_from_scan(scan, m.as_str());
            return (Some(group.to_string()), stripped);
        }
    }

    if let Some(c) = BRACKET_GROUP_END.captures(cleaned_scan) {
        let m = c.get(0).expect("group 0");
        let group = c[1].trim();
        if is_valid_bracket_group(group) {
            let stripped = strip_group_from_scan(scan, m.as_str());
            return (Some(group.to_string()), stripped);
        }
    }

    (None, scan.to_string())
}

fn strip_trailing_language(s: &str) -> &str {
    let trimmed = s.trim_end();
    if let Some(idx) = trimmed.rfind([' ', '.', '_']) {
        let last = &trimmed[idx + 1..];
        if !release::languages(last).is_empty() {
            return &trimmed[..idx];
        }
    }
    trimmed
}

fn is_valid_hyphen_group(before: &str, group: &str) -> bool {
    if group.is_empty()
        || group.bytes().all(|b| b.is_ascii_digit())
        || NOT_GROUP_WORD.is_match(group)
    {
        return false;
    }
    // Reject `WEB-DL`, `Blu-Ray`, `DTS-HD`, `DTS-MA`, `10-bit`, `HDTV-720p`, `WEBDL-1080p`.
    let before_lower = before.to_ascii_lowercase();
    let bad_prefix = [
        "web", "blu", "dts", "hdtv", "sdtv", "webdl", "webrip", "480p", "576p", "720p", "1080p",
        "2160p", "raw",
    ];
    if bad_prefix.iter().any(|p| before_lower.ends_with(p)) && NOT_GROUP_WORD.is_match(group) {
        return false;
    }
    // Reject if the matched token looks like a quality suffix (`HDTV-1080p` already caught by NOT_GROUP_WORD).
    true
}

fn is_valid_bracket_group(group: &str) -> bool {
    !(group.is_empty()
        || group.bytes().all(|b| b.is_ascii_digit())
        || NOT_GROUP_WORD.is_match(group)
        || TV_RESOLUTION.is_match(group)
        || TV_SOURCE.is_match(group)
        || TV_ANIME_BLURAY.is_match(group)
        || TV_RAW_HD.is_match(group)
        || TV_REMUX.is_match(group)
        || release::codec(group).is_some()
        || !release::audio(group).is_empty()
        || !release::hdr(group).is_empty()
        || !release::languages(group).is_empty())
}

fn strip_group_from_scan(scan: &str, group_token: &str) -> String {
    if let Some(idx) = scan.rfind(group_token) {
        let mut out = String::with_capacity(scan.len());
        out.push_str(&scan[..idx]);
        out.push_str(&scan[idx + group_token.len()..]);
        out
    } else {
        scan.to_string()
    }
}

fn parse_revisions(scan: &str) -> (bool, bool, u8) {
    let norm = scan.replace('_', " ");
    let mut base_version: u8 = 1;
    let mut has_explicit_version = false;

    for c in TV_VERSION.captures_iter(&norm) {
        for idx in 1..=4 {
            if let Some(m) = c.get(idx)
                && let Ok(v) = m.as_str().parse::<u8>()
                && v >= 2
            {
                base_version = base_version.max(v);
                has_explicit_version = true;
            }
        }
    }

    let mut proper = false;
    let mut repack = false;
    let mut version = base_version;

    for c in TV_PROPER.captures_iter(&norm) {
        proper = true;
        let num = c.get(1).and_then(|m| m.as_str().parse::<u8>().ok());
        let next = match num {
            Some(n) => 1u8.saturating_add(n),
            None if has_explicit_version => base_version.saturating_add(1),
            None => 2,
        };
        version = version.max(next);
    }

    for c in TV_REPACK.captures_iter(&norm) {
        repack = true;
        let num = c.get(1).and_then(|m| m.as_str().parse::<u8>().ok());
        let next = match num {
            Some(n) => 1u8.saturating_add(n),
            None if has_explicit_version => base_version.saturating_add(1),
            None => 2,
        };
        version = version.max(next);
    }

    (proper, repack, version)
}

fn parse_tv_resolution(norm: &str) -> TvRes {
    if let Some(c) = TV_RESOLUTION.captures(norm) {
        if c.name("r360").is_some() {
            return TvRes::R360p;
        }
        if c.name("r480").is_some() {
            return TvRes::R480p;
        }
        if c.name("r540").is_some() {
            return TvRes::R540p;
        }
        if c.name("r576").is_some() {
            return TvRes::R576p;
        }
        if c.name("r720").is_some() {
            return TvRes::R720p;
        }
        if c.name("r1080").is_some() {
            return TvRes::R1080p;
        }
        if c.name("r2160").is_some() {
            return TvRes::R2160p;
        }
    }
    if TV_ALT_RESOLUTION.is_match(norm) {
        return TvRes::R2160p;
    }
    TvRes::Unknown
}

fn extension_quality(ext: Option<&str>) -> Quality {
    match ext {
        Some("m2ts") => Quality::Bluray720p,
        Some("mkv" | "ts" | "wtv") => Quality::Hdtv720p,
        Some("img" | "iso" | "vob") => Quality::Dvd,
        Some(
            "mp4" | "avi" | "m4v" | "wmv" | "mov" | "mpg" | "mpeg" | "flv" | "ogm" | "ogv" | "divx"
            | "xvid" | "3gp" | "rm" | "rmvb" | "asf" | "m2v",
        ) => Quality::Sdtv,
        _ => Quality::Unknown,
    }
}

/// Mirrors Sonarr's `QualityParser.ParseQuality` so every parsed quality is a
/// valid TV quality (`quality.is_tv() == true`).
fn parse_tv_quality(
    tags_no_group: &str,
    full_scan: &str,
    ext: Option<&str>,
) -> (Quality, Option<Resolution>) {
    let norm = tags_no_group.replace('_', " ");
    let norm = norm.trim();

    let res = parse_tv_resolution(norm);
    let exposed_res = res.to_resolution();

    if TV_RAW_HD.is_match(norm) {
        return (Quality::RawHd, exposed_res.or(Some(Resolution::R1080p)));
    }

    let remux = TV_REMUX.is_match(norm);
    let xvid_or_divx = TV_XVID_DIVX.is_match(norm);

    if let Some(last_source) = TV_SOURCE.captures_iter(norm).last() {
        if last_source.name("bluray").is_some() {
            if xvid_or_divx {
                return (Quality::Bluray480p, exposed_res);
            }
            let q = match res {
                TvRes::R2160p => {
                    if remux {
                        Quality::Remux2160p
                    } else {
                        Quality::Bluray2160p
                    }
                }
                TvRes::R1080p => {
                    if remux {
                        Quality::Remux1080p
                    } else {
                        Quality::Bluray1080p
                    }
                }
                TvRes::R576p => Quality::Bluray576p,
                TvRes::R360p | TvRes::R480p | TvRes::R540p => Quality::Bluray480p,
                TvRes::Unknown if remux => Quality::Remux1080p,
                TvRes::R720p | TvRes::Unknown => Quality::Bluray720p,
            };
            return (q, exposed_res);
        }

        if last_source.name("webdl").is_some() {
            let q = match res {
                TvRes::R2160p => Quality::WebDl2160p,
                TvRes::R1080p => Quality::WebDl1080p,
                TvRes::R720p => Quality::WebDl720p,
                TvRes::Unknown
                    if full_scan.contains("[WEBDL]")
                        || full_scan.contains("[WEB]")
                        || full_scan.contains("[Web]") =>
                {
                    Quality::WebDl720p
                }
                _ => Quality::WebDl480p,
            };
            return (q, exposed_res);
        }

        if last_source.name("webrip").is_some() {
            let q = match res {
                TvRes::R2160p => Quality::WebRip2160p,
                TvRes::R1080p => Quality::WebRip1080p,
                TvRes::R720p => Quality::WebRip720p,
                _ => Quality::WebRip480p,
            };
            return (q, exposed_res);
        }

        if last_source.name("hdtv").is_some() {
            if TV_MPEG2.is_match(norm) {
                return (Quality::RawHd, exposed_res);
            }
            let q = match res {
                TvRes::R2160p => Quality::Hdtv2160p,
                TvRes::R1080p => Quality::Hdtv1080p,
                TvRes::R720p => Quality::Hdtv720p,
                TvRes::Unknown if full_scan.contains("[HDTV]") => Quality::Hdtv720p,
                _ => Quality::Sdtv,
            };
            return (q, exposed_res);
        }

        if last_source.name("bdrip").is_some() {
            let q = match res {
                TvRes::R2160p => Quality::Bluray2160p,
                TvRes::R1080p => Quality::Bluray1080p,
                TvRes::R720p => Quality::Bluray720p,
                _ => Quality::Bluray480p,
            };
            return (q, exposed_res);
        }

        if last_source.name("dvd").is_some() {
            return (Quality::Dvd, exposed_res);
        }

        if last_source.name("tvsd").is_some() {
            let q = match res {
                TvRes::R1080p => Quality::Hdtv1080p,
                TvRes::R720p => Quality::Hdtv720p,
                _ if TV_HR_WS.is_match(norm) => Quality::Hdtv720p,
                _ => Quality::Sdtv,
            };
            return (q, exposed_res);
        }
    }

    // Trailing uppercase `.WEB` or ` WEB` (e.g. `Series - S01E11 1080p MKV WEB`).
    if TV_END_UPPER_WEB.is_match(norm) {
        let q = match res {
            TvRes::R2160p => Quality::WebDl2160p,
            TvRes::R1080p => Quality::WebDl1080p,
            TvRes::R720p => Quality::WebDl720p,
            _ => Quality::WebDl480p,
        };
        return (q, exposed_res);
    }

    // Compact anime/release bluray tags (`BD720p`, `BD1080p`, `Bluray1080p`).
    if let Some(c) = TV_ANIME_BLURAY.captures(norm) {
        let (q, r) = match &c[1] {
            "2160" => (
                if remux {
                    Quality::Remux2160p
                } else {
                    Quality::Bluray2160p
                },
                Some(Resolution::R2160p),
            ),
            "1080" => (
                if remux {
                    Quality::Remux1080p
                } else {
                    Quality::Bluray1080p
                },
                Some(Resolution::R1080p),
            ),
            _ => (Quality::Bluray720p, Some(Resolution::R720p)),
        };
        return (q, exposed_res.or(r));
    }

    // Remux with resolution and no explicit source.
    if remux && res != TvRes::Unknown {
        let q = match res {
            TvRes::R2160p => Quality::Remux2160p,
            TvRes::R1080p => Quality::Remux1080p,
            TvRes::R720p => Quality::Bluray720p,
            _ => Quality::Bluray480p,
        };
        return (q, exposed_res);
    }

    // Resolution without source: Sonarr maps to HDTV (or Bluray for `.m2ts`).
    if res != TvRes::Unknown {
        let ext_is_bluray = ext == Some("m2ts");
        let q = match (res, ext_is_bluray) {
            (TvRes::R2160p, true) => Quality::Bluray2160p,
            (TvRes::R2160p, false) => Quality::Hdtv2160p,
            (TvRes::R1080p, true) => Quality::Bluray1080p,
            (TvRes::R1080p, false) => Quality::Hdtv1080p,
            (TvRes::R720p, true) => Quality::Bluray720p,
            (TvRes::R720p, false) => Quality::Hdtv720p,
            (_, true) => Quality::Bluray480p,
            (_, false) => Quality::Sdtv,
        };
        return (q, exposed_res);
    }

    // Bare `x264` with no source or resolution -> SDTV in Sonarr.
    if TV_X264_ONLY.is_match(norm) {
        return (Quality::Sdtv, None);
    }

    // `HD TV` / `SD TV` fallback.
    if let Some(c) = TV_OTHER_SOURCE.captures(norm) {
        let q = if c.name("hdtv").is_some() {
            Quality::Hdtv720p
        } else {
            Quality::Sdtv
        };
        return (q, None);
    }

    // Extension-based fallback (e.g. `.mkv` -> HDTV-720p, `.avi` -> SDTV).
    (extension_quality(ext), None)
}
