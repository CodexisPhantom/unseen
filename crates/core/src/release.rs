//! Release-name parser for movies, specified in `docs/07-release-scoring.md`.
//!
//! The parser never fails: a name it does not understand yields a result
//! with an unknown source and the whole name as the title.

use std::sync::LazyLock;

use regex::Regex;

#[cfg(test)]
mod tests;

/// Vertical resolution of a release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    R480p,
    R576p,
    R720p,
    R1080p,
    R2160p,
}

/// Where the video comes from, as in Radarr's `QualitySource`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Unknown,
    Workprint,
    Cam,
    Telesync,
    Telecine,
    Dvd,
    Tv,
    WebDl,
    WebRip,
    Bluray,
}

/// Refinement of the source, as in Radarr's quality `Modifier`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    None,
    Regional,
    Screener,
    RawHd,
    BrDisk,
    Remux,
}

/// Video codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    X264,
    X265,
    Av1,
    Xvid,
}

/// Audio format tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audio {
    Aac,
    Ac3,
    Eac3,
    Dts,
    DtsHdMa,
    TrueHd,
    Atmos,
}

/// HDR format tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hdr {
    Hdr10,
    Hdr10Plus,
    DolbyVision,
}

/// Language tag of a release name. `Multi` is the `MULTI` tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    French,
    German,
    Spanish,
    Italian,
    Dutch,
    Portuguese,
    Russian,
    Japanese,
    Korean,
    Chinese,
    Swedish,
    Norwegian,
    Danish,
    Finnish,
    Polish,
    Hindi,
    Multi,
}

/// Everything the parser reads from one release name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRelease {
    /// Movie title with separators replaced by spaces.
    pub title: String,
    /// Release year of the movie.
    pub year: Option<u16>,
    /// Resolution tag, if any.
    pub resolution: Option<Resolution>,
    /// Source; `Source::Unknown` when the name has no source tag.
    pub source: Source,
    /// Source modifier such as remux.
    pub modifier: Modifier,
    /// Video codec tag, if any.
    pub codec: Option<Codec>,
    /// Audio tags, without duplicates.
    pub audio: Vec<Audio>,
    /// HDR tags, without duplicates.
    pub hdr: Vec<Hdr>,
    /// Release group.
    pub release_group: Option<String>,
    /// Edition such as `IMAX` or `Extended`.
    pub edition: Option<String>,
    /// The name has a `PROPER` tag.
    pub proper: bool,
    /// The name has a `REPACK` or `RERIP` tag.
    pub repack: bool,
    /// Revision: 1 for a first release, 2 for a proper or repack, and so on.
    pub version: u8,
    /// Language tags; empty means unknown.
    pub languages: Vec<Language>,
    /// The name says the subtitles are burned in.
    pub hardcoded_subs: bool,
    /// The name has a `3D` tag.
    pub three_d: bool,
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("static regex is valid")
}

static WEBSITE: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)^\[?\s*www\.[\w-]+\.[a-z]{2,6}\s*\]?[-. ]+"));
static EXTENSION: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\.(?:mkv|mp4|avi|m4v|wmv|mov)$"));
static YEAR: LazyLock<Regex> = LazyLock::new(|| re(r"\b(?:1[89]|20)\d{2}\b"));
static RESOLUTION: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)\b(?:(?P<r480>480[pi]|640x480|848x480)|(?P<r576>576[pi])",
        r"|(?P<r720>720[pi]|1280x720|960p)|(?P<r1080>1080[pi]|1920x1080|1440p|fhd)",
        r"|(?P<r2160>2160p|3840x2160|4k|uhd))\b",
    ))
});
static SOURCE: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)\b(?:(?P<webrip>web[-. ]?rip|webmux|web[-. ]?cap)",
        r"|(?P<webdl>web[-. ]?dl(?:mux)?|webhd|web)",
        r"|(?P<bluray>blu[-. ]?ray|bdrip|brrip|bdmux|bdmv|bdiso|bd(?:25|50|66|100)?|uhdbd|hd[-. ]?dvd|br[-. ]?disk)",
        r"|(?P<rawhd>raw[-. ]?hd)",
        r"|(?P<tv>hdtv|pdtv|sdtv|dsr|tvrip)",
        r"|(?P<scr>(?:dvd|bd|hd)?[-. ]?scr(?:eener)?)",
        r"|(?P<regional>r\d|regional)",
        r"|(?P<dvdr>dvd[-. ]?r|dvd[59])",
        r"|(?P<dvd>dvd(?:rip|mux)?|xvidvd)",
        r"|(?P<ts>telesynch?|hd[-. ]?ts(?:rip)?|ts(?:rip)?|pdvd)",
        r"|(?P<tc>telecine|hd[-. ]?tc|tc)",
        r"|(?P<cam>camrip|(?:new|hq)?cam|hd[-. ]?cam(?:rip)?)",
        r"|(?P<wp>workprint|wp))\b",
    ))
});
/// Capture group of `SOURCE` and what it means. `dvdr` uses the remux
/// modifier because Radarr defines DVD-R as source DVD with modifier REMUX.
const SOURCES: &[(&str, Source, Modifier)] = &[
    ("webrip", Source::WebRip, Modifier::None),
    ("webdl", Source::WebDl, Modifier::None),
    ("bluray", Source::Bluray, Modifier::None),
    ("rawhd", Source::Tv, Modifier::RawHd),
    ("tv", Source::Tv, Modifier::None),
    ("scr", Source::Dvd, Modifier::Screener),
    ("regional", Source::Dvd, Modifier::Regional),
    ("dvdr", Source::Dvd, Modifier::Remux),
    ("dvd", Source::Dvd, Modifier::None),
    ("ts", Source::Telesync, Modifier::None),
    ("tc", Source::Telecine, Modifier::None),
    ("cam", Source::Cam, Modifier::None),
    ("wp", Source::Workprint, Modifier::None),
];
static REMUX: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(?:(?:bd|uhd)[-. ]?)?remux\b"));
static BR_DISK: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\b(?:complete|iso|bdiso|bdmv|bd(?:25|50|66|100)|br[-. ]?disk)\b"));
static CODEC: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)\b(?:(?P<x264>x264|h[. ]?264|avc)|(?P<x265>x265|h[. ]?265|hevc)",
        r"|(?P<av1>av1)|(?P<xvid>xvid))\b",
    ))
});
static DTS_HD_MA: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bdts[-. ]?(?:hd[-. ]?)?ma\b"));
static DTS: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bdts\b"));
static TRUE_HD: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\btrue[-. ]?hd\b"));
static EAC3: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\b(?:(?:e-?ac-?3|ddp|ddplus)(?:\d[. ]\d)?\b|dd\+)"));
static AC3: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(?:ac-?3|dd)(?:\d[. ]\d)?\b"));
static AAC: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\baac(?:\d[. ]\d)?\b"));
static ATMOS: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\batmos\b"));
static DOLBY_VISION: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\b(?:dv|dovi|dolby[. ]?vision)\b"));
static HDR10_PLUS: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bhdr10(?:\+|p\b|plus\b)"));
static HDR10: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bhdr(?:10)?\b"));
static EDITION: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)\b(?:(?:the[. ])?(?:(?:recut|extended|ultimate)[. ])?",
        r"(?:director.?s|collector.?s|theatrical|ultimate|extended|despecialized",
        r"|(?:special|rouge|final|assembly|imperial|diamond|signature|hunter|rekall)[. ](?:cut|edition|version)",
        r"|\d{2,3}(?:th)?[. ]anniversary)",
        r"(?:[. ](?:cut|edition|version))?",
        r"(?:[. ](?:extended|uncensored|remastered|unrated|uncut|open.?matte|imax|fan.?edit))?",
        r"|uncensored|remastered|unrated|uncut|open.?matte|imax|fan.?edit|restored|[234]in1)\b",
    ))
});
static REVISION: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(proper|repack|rerip)(\d)?\b"));
static HARDCODED_SUBS: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\b(?:hc|subbed|hardsubs?|hardcoded|korsub)\b"));
static THREE_D: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b3d\b"));
static HYPHEN_GROUP: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)-([a-z0-9]+)(?:\[\w+\])?\s*$"));
/// Text after a last hyphen that is the end of a tag, not a group.
static NOT_GROUP: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)^(?:dl|ray|rip|hd|x|ma|es|en|cat|eng|jap|ger|fra|fre|ita|hdrip|r|disk|\d{1,2}(?:bit)?|\d{3,4}[pi])$",
    )
});
static BRACKET_GROUP: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\[([a-z0-9.]+)\]\s*$"));

/// Lowercase language tag and its language.
const LANGUAGES: &[(&str, Language)] = &[
    ("english", Language::English),
    ("french", Language::French),
    ("truefrench", Language::French),
    ("vff", Language::French),
    ("vfq", Language::French),
    ("vostfr", Language::French),
    ("german", Language::German),
    ("spanish", Language::Spanish),
    ("italian", Language::Italian),
    ("dutch", Language::Dutch),
    ("portuguese", Language::Portuguese),
    ("russian", Language::Russian),
    ("japanese", Language::Japanese),
    ("korean", Language::Korean),
    ("chinese", Language::Chinese),
    ("swedish", Language::Swedish),
    ("norwegian", Language::Norwegian),
    ("danish", Language::Danish),
    ("finnish", Language::Finnish),
    ("polish", Language::Polish),
    ("hindi", Language::Hindi),
    ("multi", Language::Multi),
];
static LANGUAGE: LazyLock<Regex> = LazyLock::new(|| {
    let words: Vec<&str> = LANGUAGES.iter().map(|(word, _)| *word).collect();
    re(&format!(r"(?i)\b(?:{})\b", words.join("|")))
});

/// Parses a movie release name. Never fails and never panics.
pub fn parse(name: &str) -> ParsedRelease {
    // `_` is a word character for `\b`; `.` has the same length, so offsets hold.
    let name = name.trim().replace('_', ".");
    let name = WEBSITE.replace(&name, "");
    let name = EXTENSION.replace(&name, "");
    let name: &str = &name;

    // The year is the last year-like token that leaves a title before it,
    // so `2001.A.Space.Odyssey.1968` has the title `2001 A Space Odyssey`.
    let year = YEAR
        .find_iter(name)
        .filter(|m| !clean_title(&name[..m.start()]).is_empty())
        .last();
    let (title_end, tags_start) = match year {
        Some(m) => (m.start(), m.end()),
        None => {
            let first_tag = [&*RESOLUTION, &*SOURCE, &*CODEC, &*EDITION, &*REVISION]
                .iter()
                .filter_map(|regex| regex.find(name))
                .map(|m| m.start())
                .filter(|&start| start > 0)
                .min()
                .unwrap_or(name.len());
            (first_tag, first_tag)
        }
    };

    let rest = &name[tags_start..];
    let group = HYPHEN_GROUP
        .captures(rest)
        .filter(|c| !NOT_GROUP.is_match(&c[1]))
        .or_else(|| {
            BRACKET_GROUP.captures(rest).filter(|c| {
                !(RESOLUTION.is_match(&c[1]) || SOURCE.is_match(&c[1]) || CODEC.is_match(&c[1]))
            })
        });
    let tags = match &group {
        Some(c) => &rest[..c.get(0).map_or(rest.len(), |m| m.start())],
        None => rest,
    };

    let (source, modifier) = source(tags);
    let mut parsed = ParsedRelease {
        title: clean_title(&name[..title_end]),
        year: year.and_then(|m| m.as_str().parse().ok()),
        resolution: resolution(tags),
        source,
        modifier,
        codec: codec(tags),
        audio: audio(tags),
        hdr: hdr(tags),
        release_group: group.map(|c| c[1].to_string()),
        edition: EDITION.find(tags).map(|m| m.as_str().replace('.', " ")),
        proper: false,
        repack: false,
        version: 1,
        languages: languages(tags),
        hardcoded_subs: hardcoded_subs(tags),
        three_d: THREE_D.is_match(tags),
    };
    for c in REVISION.captures_iter(tags) {
        if c[1].eq_ignore_ascii_case("proper") {
            parsed.proper = true;
        } else {
            parsed.repack = true;
        }
        let number = c.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(1);
        parsed.version = parsed.version.max(1 + number);
    }
    parsed
}

/// Radarr quality name for parsed facts. Temporary: issue #40 adds the
/// quality enum with Radarr ids, which must replace this function.
pub fn quality_name(
    source: Source,
    modifier: Modifier,
    resolution: Option<Resolution>,
) -> &'static str {
    use Resolution::{R480p, R576p, R720p, R1080p, R2160p};
    match (source, modifier, resolution) {
        (_, Modifier::RawHd, _) => "Raw-HD",
        (_, Modifier::BrDisk, _) => "BR-DISK",
        (_, Modifier::Screener, _) => "DVDSCR",
        (_, Modifier::Regional, _) => "REGIONAL",
        (Source::Unknown, _, _) => "Unknown",
        (Source::Workprint, _, _) => "WORKPRINT",
        (Source::Cam, _, _) => "CAM",
        (Source::Telesync, _, _) => "TELESYNC",
        (Source::Telecine, _, _) => "TELECINE",
        (Source::Dvd, Modifier::Remux, _) => "DVD-R",
        (Source::Dvd, _, _) => "DVD",
        (Source::Tv, _, Some(R2160p)) => "HDTV-2160p",
        (Source::Tv, _, Some(R1080p)) => "HDTV-1080p",
        (Source::Tv, _, Some(R720p)) => "HDTV-720p",
        (Source::Tv, _, _) => "SDTV",
        (Source::WebDl, _, Some(R2160p)) => "WEBDL-2160p",
        (Source::WebDl, _, Some(R1080p)) => "WEBDL-1080p",
        (Source::WebDl, _, Some(R720p)) => "WEBDL-720p",
        (Source::WebDl, _, _) => "WEBDL-480p",
        (Source::WebRip, _, Some(R2160p)) => "WEBRip-2160p",
        (Source::WebRip, _, Some(R1080p)) => "WEBRip-1080p",
        (Source::WebRip, _, Some(R720p)) => "WEBRip-720p",
        (Source::WebRip, _, _) => "WEBRip-480p",
        (Source::Bluray, Modifier::Remux, Some(R2160p)) => "Remux-2160p",
        (Source::Bluray, Modifier::Remux, Some(R1080p) | None) => "Remux-1080p",
        (Source::Bluray, _, Some(R2160p)) => "Bluray-2160p",
        (Source::Bluray, _, Some(R1080p)) => "Bluray-1080p",
        (Source::Bluray, _, Some(R576p)) => "Bluray-576p",
        (Source::Bluray, _, Some(R480p)) => "Bluray-480p",
        (Source::Bluray, _, Some(R720p) | None) => "Bluray-720p",
    }
}

fn clean_title(raw: &str) -> String {
    let words: Vec<&str> = raw
        .split(|c: char| c == '.' || c.is_whitespace())
        .filter(|word| !word.is_empty())
        .collect();
    words
        .join(" ")
        .trim_start_matches([' ', '-'])
        .trim_end_matches([' ', '-', '(', '['])
        .to_string()
}

fn resolution(tags: &str) -> Option<Resolution> {
    let c = RESOLUTION.captures(tags)?;
    [
        ("r480", Resolution::R480p),
        ("r576", Resolution::R576p),
        ("r720", Resolution::R720p),
        ("r1080", Resolution::R1080p),
        ("r2160", Resolution::R2160p),
    ]
    .into_iter()
    .find(|(group, _)| c.name(group).is_some())
    .map(|(_, resolution)| resolution)
}

fn source(tags: &str) -> (Source, Modifier) {
    let (source, modifier) = SOURCE
        .captures(tags)
        .and_then(|c| SOURCES.iter().find(|(group, _, _)| c.name(group).is_some()))
        .map_or(
            (Source::Unknown, Modifier::None),
            |&(_, source, modifier)| (source, modifier),
        );
    // A remux or a disc image without another source is a Blu-ray.
    if matches!(source, Source::Bluray | Source::Unknown) {
        if REMUX.is_match(tags) {
            return (Source::Bluray, Modifier::Remux);
        }
        if BR_DISK.is_match(tags) {
            return (Source::Bluray, Modifier::BrDisk);
        }
    }
    (source, modifier)
}

pub(crate) fn codec(tags: &str) -> Option<Codec> {
    let c = CODEC.captures(tags)?;
    [
        ("x264", Codec::X264),
        ("x265", Codec::X265),
        ("av1", Codec::Av1),
        ("xvid", Codec::Xvid),
    ]
    .into_iter()
    .find(|(group, _)| c.name(group).is_some())
    .map(|(_, codec)| codec)
}

pub(crate) fn audio(tags: &str) -> Vec<Audio> {
    let mut audio = Vec::new();
    if AAC.is_match(tags) {
        audio.push(Audio::Aac);
    }
    // `DD+` also matches the AC3 pattern, so EAC3 wins.
    if EAC3.is_match(tags) {
        audio.push(Audio::Eac3);
    } else if AC3.is_match(tags) {
        audio.push(Audio::Ac3);
    }
    if DTS_HD_MA.is_match(tags) {
        audio.push(Audio::DtsHdMa);
    } else if DTS.is_match(tags) {
        audio.push(Audio::Dts);
    }
    if TRUE_HD.is_match(tags) {
        audio.push(Audio::TrueHd);
    }
    if ATMOS.is_match(tags) {
        audio.push(Audio::Atmos);
    }
    audio
}

pub(crate) fn hdr(tags: &str) -> Vec<Hdr> {
    let mut hdr = Vec::new();
    if HDR10_PLUS.is_match(tags) {
        hdr.push(Hdr::Hdr10Plus);
    } else if HDR10.is_match(tags) {
        hdr.push(Hdr::Hdr10);
    }
    if DOLBY_VISION.is_match(tags) {
        hdr.push(Hdr::DolbyVision);
    }
    hdr
}

pub(crate) fn languages(tags: &str) -> Vec<Language> {
    let mut found = Vec::new();
    for m in LANGUAGE.find_iter(tags) {
        let word = m.as_str().to_ascii_lowercase();
        if let Some((_, language)) = LANGUAGES.iter().find(|(tag, _)| *tag == word)
            && !found.contains(language)
        {
            found.push(*language);
        }
    }
    found
}

pub(crate) fn hardcoded_subs(tags: &str) -> bool {
    HARDCODED_SUBS.is_match(tags)
}
