//! Subtitles next to a video (docs/03-domain-model.md, "Subtitles (M10)";
//! docs/09-filesystem.md, "Sidecar subtitles"): language profiles, the
//! language and kind that a file name says. No disk access here.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// The language of a subtitle file whose name has no language.
pub const LANGUAGE_UNDETERMINED: &str = "und";

/// Extensions of subtitle files (`import.extra_file_extensions`).
pub const EXTENSIONS: [&str; 5] = ["srt", "ass", "ssa", "sub", "idx"];

/// What a subtitle is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Normal,
    /// Hearing impaired (SDH).
    Hi,
    /// Foreign parts only.
    Forced,
}

impl Kind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Hi => "hi",
            Self::Forced => "forced",
        }
    }

    /// The kind as it is stored, or `None` for any other text.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "normal" => Some(Self::Normal),
            "hi" => Some(Self::Hi),
            "forced" => Some(Self::Forced),
            _ => None,
        }
    }
}

/// One wanted subtitle of a language profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub language: String,
    pub kind: Kind,
}

/// The fields of a language profile that have rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub items: Vec<Item>,
    /// Index into `items`.
    pub cutoff: Option<usize>,
}

/// The first rule a profile breaks: the field and the message.
pub type Invalid = (&'static str, String);

/// A language as stored: two lower case letters, with an upper case region
/// where it matters (`en`, `pt-BR`).
#[must_use]
pub fn is_language_code(text: &str) -> bool {
    let (language, region) = match text.split_once('-') {
        Some((language, region)) => (language, Some(region)),
        None => (text, None),
    };
    language.len() == 2
        && language.bytes().all(|b| b.is_ascii_lowercase())
        && region.is_none_or(|r| r.len() == 2 && r.bytes().all(|b| b.is_ascii_uppercase()))
}

/// Checks a language profile: a name, at least one item, a valid and unique
/// language and kind in each item, and a cutoff that points at an item.
pub fn validate(profile: &Profile) -> Result<(), Invalid> {
    if profile.name.trim().is_empty() {
        return Err(("name", "must not be empty".to_owned()));
    }
    if profile.items.is_empty() {
        return Err(("items", "must hold at least one item".to_owned()));
    }
    let mut seen = BTreeSet::new();
    for item in &profile.items {
        if !is_language_code(&item.language) {
            return Err((
                "items",
                format!(
                    "`{}` is not a language code such as `en` or `pt-BR`",
                    item.language
                ),
            ));
        }
        if !seen.insert((&item.language, item.kind)) {
            return Err((
                "items",
                format!("lists {} ({}) twice", item.language, item.kind.as_str()),
            ));
        }
    }
    if profile
        .cutoff
        .is_some_and(|cutoff| cutoff >= profile.items.len())
    {
        return Err(("cutoff", "must be the index of an item".to_owned()));
    }
    Ok(())
}

/// ISO 639-2 codes (both the bibliographic and the terminology form) of the
/// common languages, with the two-letter code.
const THREE_LETTER: &[(&str, &str)] = &[
    ("ara", "ar"),
    ("bul", "bg"),
    ("cat", "ca"),
    ("ces", "cs"),
    ("cze", "cs"),
    ("dan", "da"),
    ("deu", "de"),
    ("ger", "de"),
    ("ell", "el"),
    ("gre", "el"),
    ("eng", "en"),
    ("spa", "es"),
    ("est", "et"),
    ("eus", "eu"),
    ("baq", "eu"),
    ("fas", "fa"),
    ("per", "fa"),
    ("fin", "fi"),
    ("fra", "fr"),
    ("fre", "fr"),
    ("heb", "he"),
    ("hin", "hi"),
    ("hrv", "hr"),
    ("hun", "hu"),
    ("ind", "id"),
    ("isl", "is"),
    ("ice", "is"),
    ("ita", "it"),
    ("jpn", "ja"),
    ("kor", "ko"),
    ("lit", "lt"),
    ("lav", "lv"),
    ("msa", "ms"),
    ("may", "ms"),
    ("nld", "nl"),
    ("dut", "nl"),
    ("nor", "no"),
    ("nob", "nb"),
    ("pol", "pl"),
    ("por", "pt"),
    ("ron", "ro"),
    ("rum", "ro"),
    ("rus", "ru"),
    ("slk", "sk"),
    ("slo", "sk"),
    ("slv", "sl"),
    ("srp", "sr"),
    ("swe", "sv"),
    ("tha", "th"),
    ("tur", "tr"),
    ("ukr", "uk"),
    ("vie", "vi"),
    ("zho", "zh"),
    ("chi", "zh"),
];

/// A language token of a file name as the stored code: `en`, `eng`, `EN`,
/// `pt-br` and `pt_BR` give `en`, `en`, `en`, `pt-BR` and `pt-BR`. `None`
/// for any other text.
fn language_token(token: &str) -> Option<String> {
    let token = token.to_ascii_lowercase();
    let (language, region) = match token.split_once(['-', '_']) {
        Some((language, region)) => (language, Some(region)),
        None => (token.as_str(), None),
    };
    let language = if language.len() == 3 {
        THREE_LETTER.iter().find(|(three, _)| *three == language)?.1
    } else {
        language
    };
    let code = match region {
        None => language.to_owned(),
        Some(region) => format!("{language}-{}", region.to_ascii_uppercase()),
    };
    is_language_code(&code).then_some(code)
}

fn kind_token(token: &str) -> Option<Kind> {
    match token.to_ascii_lowercase().as_str() {
        "sdh" | "cc" => Some(Kind::Hi),
        "forced" | "foreign" => Some(Kind::Forced),
        _ => None,
    }
}

/// A subtitle file next to a video.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sidecar {
    pub language: String,
    pub kind: Kind,
}

/// Reads the name of a file that may be a subtitle of `video` (both are file
/// names, without a folder). The subtitle must start with the video's base
/// name and end in a subtitle extension; what lies between them is the
/// language and the kind, in either order (`.en.hi`, `.eng`, `.forced.fr`).
///
/// A name without a language is `und`. The token `hi` is Hindi when no other
/// token is a language, and hearing impaired otherwise. Unknown tokens are
/// ignored. `None` when the file is not a subtitle of this video.
#[must_use]
pub fn parse_sidecar(video: &str, file: &str) -> Option<Sidecar> {
    let base = video.rsplit_once('.').map_or(video, |(base, _)| base);
    let (stem, extension) = file.rsplit_once('.')?;
    if !EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        || !stem.is_char_boundary(base.len())
    {
        return None;
    }
    let (head, rest) = stem.split_at_checked(base.len())?;
    if !head.eq_ignore_ascii_case(base) {
        return None;
    }
    let tokens: Vec<&str> = if rest.is_empty() {
        Vec::new()
    } else {
        rest.strip_prefix('.')?.split('.').collect()
    };

    let mut language = None;
    let mut kind = None;
    let mut hi = false;
    for token in &tokens {
        if token.eq_ignore_ascii_case("hi") {
            hi = true;
        } else if let Some(found) = kind_token(token) {
            kind.get_or_insert(found);
        } else if let Some(found) = language_token(token) {
            language.get_or_insert(found);
        }
    }
    // `hi` is the kind next to a language, and Hindi alone.
    if hi {
        if language.is_some() {
            kind.get_or_insert(Kind::Hi);
        } else {
            language = Some("hi".to_owned());
        }
    }
    Some(Sidecar {
        language: language.unwrap_or_else(|| LANGUAGE_UNDETERMINED.to_owned()),
        kind: kind.unwrap_or(Kind::Normal),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIDEO: &str = "Movie (2020) Bluray-1080p.mkv";

    fn sidecar(file: &str) -> Option<(String, Kind)> {
        parse_sidecar(VIDEO, file).map(|s| (s.language, s.kind))
    }

    fn found(language: &str, kind: Kind) -> Option<(String, Kind)> {
        Some((language.to_owned(), kind))
    }

    #[test]
    fn the_names_of_the_design_doc_are_read() {
        assert_eq!(
            sidecar("Movie (2020) Bluray-1080p.en.srt"),
            found("en", Kind::Normal)
        );
        assert_eq!(
            sidecar("Movie (2020) Bluray-1080p.en.hi.srt"),
            found("en", Kind::Hi)
        );
        assert_eq!(
            sidecar("Movie (2020) Bluray-1080p.fr.forced.srt"),
            found("fr", Kind::Forced)
        );
        assert_eq!(
            sidecar("Movie (2020) Bluray-1080p.pt-BR.srt"),
            found("pt-BR", Kind::Normal)
        );
    }

    #[test]
    fn aliases_codes_and_order_are_accepted() {
        assert_eq!(
            sidecar("Movie (2020) Bluray-1080p.eng.sdh.srt"),
            found("en", Kind::Hi)
        );
        assert_eq!(
            sidecar("Movie (2020) Bluray-1080p.cc.en.ass"),
            found("en", Kind::Hi)
        );
        assert_eq!(
            sidecar("Movie (2020) Bluray-1080p.foreign.fra.SRT"),
            found("fr", Kind::Forced)
        );
        assert_eq!(
            sidecar("Movie (2020) Bluray-1080p.pt_br.srt"),
            found("pt-BR", Kind::Normal)
        );
        assert_eq!(
            sidecar("movie (2020) bluray-1080p.EN.srt"),
            found("en", Kind::Normal)
        );
    }

    #[test]
    fn a_name_without_a_language_is_undetermined() {
        assert_eq!(
            sidecar("Movie (2020) Bluray-1080p.srt"),
            found("und", Kind::Normal)
        );
        assert_eq!(
            sidecar("Movie (2020) Bluray-1080p.forced.srt"),
            found("und", Kind::Forced)
        );
        assert_eq!(
            sidecar("Movie (2020) Bluray-1080p.zzz.srt"),
            found("und", Kind::Normal)
        );
    }

    #[test]
    fn hi_is_hindi_only_without_another_language() {
        assert_eq!(
            sidecar("Movie (2020) Bluray-1080p.hi.srt"),
            found("hi", Kind::Normal)
        );
        assert_eq!(
            sidecar("Movie (2020) Bluray-1080p.hin.srt"),
            found("hi", Kind::Normal)
        );
        assert_eq!(
            sidecar("Movie (2020) Bluray-1080p.hi.forced.srt"),
            found("hi", Kind::Forced)
        );
    }

    #[test]
    fn other_files_are_not_subtitles_of_the_video() {
        for no in [
            "Other.en.srt",
            "Movie (2020) Bluray-1080p.mkv",
            "Movie (2020) Bluray-1080p.en.txt",
            "Movie (2020) Bluray-1080p.nfo",
            "Movie (2020) Bluray-1080p",
            "Movie (2020) Bluray-1080p-extra.en.srt",
            "Movie (2020) Bluray-1080",
            "srt",
        ] {
            assert_eq!(sidecar(no), None, "{no}");
        }
    }

    #[test]
    fn release_names_with_dots_keep_their_language() {
        assert_eq!(
            parse_sidecar(
                "Dune.2021.1080p.WEB-DL.mkv",
                "Dune.2021.1080p.WEB-DL.nl.srt"
            )
            .map(|s| s.language),
            Some("nl".to_owned())
        );
    }

    fn profile(items: &[(&str, Kind)], cutoff: Option<usize>) -> Profile {
        Profile {
            name: "English".to_owned(),
            items: items
                .iter()
                .map(|(language, kind)| Item {
                    language: (*language).to_owned(),
                    kind: *kind,
                })
                .collect(),
            cutoff,
        }
    }

    #[test]
    fn a_valid_profile_passes() {
        let ok = profile(&[("en", Kind::Normal), ("pt-BR", Kind::Hi)], Some(1));
        assert_eq!(validate(&ok), Ok(()));
        // The same language twice is fine with another kind.
        let ok = profile(&[("en", Kind::Normal), ("en", Kind::Forced)], None);
        assert_eq!(validate(&ok), Ok(()));
    }

    #[test]
    fn a_broken_profile_names_its_field() {
        let field = |p: &Profile| validate(p).unwrap_err().0;
        let mut blank = profile(&[("en", Kind::Normal)], None);
        blank.name = "  ".to_owned();
        assert_eq!(field(&blank), "name");
        assert_eq!(field(&profile(&[], None)), "items");
        assert_eq!(field(&profile(&[("english", Kind::Normal)], None)), "items");
        assert_eq!(field(&profile(&[("EN", Kind::Normal)], None)), "items");
        assert_eq!(field(&profile(&[("pt-br", Kind::Normal)], None)), "items");
        assert_eq!(
            field(&profile(&[("en", Kind::Hi), ("en", Kind::Hi)], None)),
            "items"
        );
        assert_eq!(field(&profile(&[("en", Kind::Normal)], Some(1))), "cutoff");
    }

    #[test]
    fn kinds_round_trip() {
        for kind in [Kind::Normal, Kind::Hi, Kind::Forced] {
            assert_eq!(Kind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(Kind::parse("sdh"), None);
    }
}
