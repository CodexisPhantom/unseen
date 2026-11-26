//! Radarr custom formats and their matching rules.
//!
//! A specification matches a release on its own, and a format combines its
//! specifications: every required one must match, and each implementation
//! group needs at least one match.

use fancy_regex::Regex;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::release::{Language, Modifier, ParsedRelease, Resolution, Source};

#[cfg(test)]
mod tests;

const BYTES_PER_GIB: f64 = 1024.0 * 1024.0 * 1024.0;

/// Radarr's language id for a movie's original language.
pub const ORIGINAL_LANGUAGE_ID: i32 = -2;

/// A custom format paired with the score a quality profile assigns to it.
#[derive(Debug, Clone)]
pub struct ScoredFormat {
    pub format: CustomFormat,
    pub score: i32,
}

/// A custom format as Radarr and the TRaSH guides export it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CustomFormat {
    #[serde(default, rename = "includeCustomFormatWhenRenaming")]
    pub include_custom_format_when_renaming: bool,
    pub name: String,
    #[serde(default)]
    pub specifications: Vec<Specification>,
}

/// One condition of a custom format.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Specification {
    #[serde(default)]
    pub name: String,
    pub implementation: String,
    #[serde(default)]
    pub negate: bool,
    #[serde(default)]
    pub required: bool,
    #[serde(default, deserialize_with = "deserialize_fields")]
    pub fields: Vec<Field>,
}

/// One field of a specification.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Field {
    pub name: String,
    pub value: Value,
}

/// The facts a custom format matches against.
#[derive(Debug, Clone, Copy)]
pub struct ReleaseInfo<'a> {
    pub title: &'a str,
    pub release_group: Option<&'a str>,
    pub edition: Option<&'a str>,
    pub languages: &'a [i32],
    pub source: Option<i32>,
    pub resolution: Option<i32>,
    pub quality_modifier: Option<i32>,
    pub size_bytes: i64,
    pub indexer_flags: &'a [i32],
    pub year: Option<i32>,
    pub movie_year: Option<i32>,
}

impl CustomFormat {
    /// Evaluates this format against a release.
    pub fn matches(&self, release: &ReleaseInfo<'_>) -> bool {
        let mut groups: Vec<(&str, bool)> = Vec::new();
        for specification in &self.specifications {
            let result = specification.matches(release);
            if specification.required && !result {
                return false;
            }
            match groups.iter_mut().find(|(implementation, _)| {
                *implementation == specification.implementation.as_str()
            }) {
                Some((_, matched)) => *matched |= result,
                None => groups.push((&specification.implementation, result)),
            }
        }
        groups.iter().all(|(_, matched)| *matched)
    }

    /// Parses one format object or an array of them.
    pub fn from_json(text: &str) -> Result<Vec<CustomFormat>, serde_json::Error> {
        Self::from_value(serde_json::from_str(text)?)
    }

    /// Parses one format value or an array of them.
    pub fn from_value(value: Value) -> Result<Vec<CustomFormat>, serde_json::Error> {
        match value {
            Value::Array(values) => values.into_iter().map(serde_json::from_value).collect(),
            value => Ok(vec![serde_json::from_value(value)?]),
        }
    }
}

impl Specification {
    fn matches(&self, release: &ReleaseInfo<'_>) -> bool {
        let result = match self.implementation.as_str() {
            "ReleaseTitleSpecification" => {
                regex_matches(text_field(self, "value"), Some(release.title))
            }
            "ReleaseGroupSpecification" => {
                regex_matches(text_field(self, "value"), release.release_group)
            }
            "EditionSpecification" => regex_matches(text_field(self, "value"), release.edition),
            "LanguageSpecification" => {
                let value = integer_field(self, "value").unwrap_or(0) as i32;
                if boolean_field(self, "exceptLanguage").unwrap_or(false) {
                    !release.languages.contains(&value)
                } else {
                    release.languages.contains(&value)
                }
            }
            "SourceSpecification" => {
                integer_field(self, "value").unwrap_or(0) as i32 == release.source.unwrap_or(0)
            }
            "ResolutionSpecification" => {
                integer_field(self, "value").unwrap_or(0) as i32 == release.resolution.unwrap_or(0)
            }
            "QualityModifierSpecification" => {
                integer_field(self, "value").unwrap_or(0) as i32
                    == release.quality_modifier.unwrap_or(0)
            }
            "SizeSpecification" => {
                let min = number_field(self, "min").unwrap_or(0.0) * BYTES_PER_GIB;
                let max = number_field(self, "max").unwrap_or(0.0) * BYTES_PER_GIB;
                let size = release.size_bytes as f64;
                size > min && size <= max
            }
            "IndexerFlagSpecification" => {
                let value = integer_field(self, "value").unwrap_or(0) as i32;
                release.indexer_flags.contains(&value)
            }
            "YearSpecification" => {
                let year = release.year.filter(|year| *year > 0).or(release.movie_year);
                let min = integer_field(self, "min").unwrap_or(0);
                let max = integer_field(self, "max").unwrap_or(0);
                year.is_some_and(|year| i64::from(year) >= min && i64::from(year) <= max)
            }
            _ => return false,
        };
        result != self.negate
    }
}

fn field<'a>(specification: &'a Specification, name: &str) -> Option<&'a Value> {
    specification
        .fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| &field.value)
}

fn text_field<'a>(specification: &'a Specification, name: &str) -> Option<&'a str> {
    field(specification, name).and_then(Value::as_str)
}

fn integer_field(specification: &Specification, name: &str) -> Option<i64> {
    field(specification, name).and_then(Value::as_i64)
}

fn number_field(specification: &Specification, name: &str) -> Option<f64> {
    field(specification, name).and_then(Value::as_f64)
}

fn boolean_field(specification: &Specification, name: &str) -> Option<bool> {
    field(specification, name).and_then(Value::as_bool)
}

fn regex_matches(pattern: Option<&str>, text: Option<&str>) -> bool {
    let Some(pattern) = pattern.map(str::trim).filter(|pattern| !pattern.is_empty()) else {
        return false;
    };
    let Some(text) = text else {
        return false;
    };
    Regex::new(&format!("(?i){pattern}"))
        .ok()
        .and_then(|regex| regex.is_match(text).ok())
        .unwrap_or(false)
}

/// Radarr's numeric source id for custom format specifications.
#[must_use]
pub const fn source_id(source: Source) -> i32 {
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

/// Radarr's numeric resolution for custom format specifications.
#[must_use]
pub const fn resolution_id(resolution: Resolution) -> i32 {
    match resolution {
        Resolution::R480p => 480,
        Resolution::R576p => 576,
        Resolution::R720p => 720,
        Resolution::R1080p => 1080,
        Resolution::R2160p => 2160,
    }
}

/// Radarr's numeric quality modifier id for custom format specifications.
#[must_use]
pub const fn modifier_id(modifier: Modifier) -> i32 {
    match modifier {
        Modifier::None => 0,
        Modifier::Regional => 1,
        Modifier::Screener => 2,
        Modifier::RawHd => 3,
        Modifier::BrDisk => 4,
        Modifier::Remux => 5,
    }
}

/// Radarr's numeric language id for an ISO 639-1 code (or `"original"` / `"any"`).
#[must_use]
pub fn language_code_id(code: &str) -> Option<i32> {
    Some(match code.trim().to_ascii_lowercase().as_str() {
        "original" => ORIGINAL_LANGUAGE_ID,
        "any" => -1,
        "en" => 1,
        "fr" => 2,
        "es" => 3,
        "de" => 4,
        "it" => 5,
        "da" => 6,
        "nl" => 7,
        "ja" => 8,
        "is" => 9,
        "zh" => 10,
        "ru" => 11,
        "pl" => 12,
        "vi" => 13,
        "sv" => 14,
        "no" | "nb" => 15,
        "fi" => 16,
        "tr" => 17,
        "pt" => 18,
        "el" => 20,
        "ko" => 21,
        "hu" => 22,
        "he" => 23,
        "lt" => 24,
        "cs" => 25,
        "hi" => 26,
        "ro" => 27,
        "th" => 28,
        "bg" => 29,
        "ar" => 31,
        "uk" => 32,
        "fa" => 33,
        "bn" => 34,
        "sk" => 35,
        "lv" => 36,
        "ca" => 38,
        "hr" => 39,
        "sr" => 40,
        "bs" => 41,
        "et" => 42,
        "ta" => 43,
        "id" => 44,
        "te" => 45,
        "mk" => 46,
        "sl" => 47,
        "ml" => 48,
        "kn" => 49,
        "sq" => 50,
        "af" => 51,
        "mr" => 52,
        "tl" => 53,
        "ur" => 54,
        "rm" => 55,
        "mn" => 56,
        "ka" => 57,
        "ga" => 58,
        _ => return None,
    })
}

/// Radarr's numeric language id for a parsed release [`Language`].
#[must_use]
pub const fn language_id(language: Language) -> Option<i32> {
    Some(match language {
        Language::English => 1,
        Language::French => 2,
        Language::Spanish => 3,
        Language::German => 4,
        Language::Italian => 5,
        Language::Danish => 6,
        Language::Dutch => 7,
        Language::Japanese => 8,
        Language::Chinese => 10,
        Language::Russian => 11,
        Language::Polish => 12,
        Language::Swedish => 14,
        Language::Norwegian => 15,
        Language::Finnish => 16,
        Language::Portuguese => 18,
        Language::Korean => 21,
        Language::Hindi => 26,
        Language::Multi => return None,
    })
}

/// Maps parsed release languages to Radarr language ids. An untagged release
/// counts as the movie's original language (including [`ORIGINAL_LANGUAGE_ID`]);
/// a tagged release that includes the movie's original language also includes
/// [`ORIGINAL_LANGUAGE_ID`].
#[must_use]
pub fn release_language_ids(
    parsed_languages: &[Language],
    original_language: Option<&str>,
) -> Vec<i32> {
    let orig_id = original_language.and_then(language_code_id);
    let mut ids = Vec::new();
    if parsed_languages.is_empty() {
        ids.push(ORIGINAL_LANGUAGE_ID);
        if let Some(id) = orig_id {
            ids.push(id);
        }
        return ids;
    }
    for &lang in parsed_languages {
        if let Some(id) = language_id(lang)
            && !ids.contains(&id)
        {
            ids.push(id);
        }
    }
    if let Some(id) = orig_id
        && ids.contains(&id)
        && !ids.contains(&ORIGINAL_LANGUAGE_ID)
    {
        ids.push(ORIGINAL_LANGUAGE_ID);
    }
    ids
}

/// Maps stored ISO 639-1 language codes (as kept on `movie_files.languages`) to
/// Radarr language ids, with the same original-language rule as
/// [`release_language_ids`].
#[must_use]
pub fn code_language_ids(language_codes: &[String], original_language: Option<&str>) -> Vec<i32> {
    let orig_id = original_language.and_then(language_code_id);
    let mut ids = Vec::new();
    if language_codes.is_empty() {
        ids.push(ORIGINAL_LANGUAGE_ID);
        if let Some(id) = orig_id {
            ids.push(id);
        }
        return ids;
    }
    for code in language_codes {
        if let Some(id) = language_code_id(code)
            && !ids.contains(&id)
        {
            ids.push(id);
        }
    }
    if let Some(id) = orig_id
        && ids.contains(&id)
        && !ids.contains(&ORIGINAL_LANGUAGE_ID)
    {
        ids.push(ORIGINAL_LANGUAGE_ID);
    }
    ids
}

/// Builds a [`ReleaseInfo`] from a release title, its parsed fields, Radarr
/// language ids, size in bytes, indexer flags, and the movie's year.
#[must_use]
pub fn release_info_from_parsed<'a>(
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

/// Sums the scores of every custom format that matches `release`.
#[must_use]
pub fn score_release(formats: &[ScoredFormat], release: &ReleaseInfo<'_>) -> i32 {
    formats
        .iter()
        .filter(|entry| entry.format.matches(release))
        .fold(0i32, |total, entry| total.saturating_add(entry.score))
}

/// Reads Radarr's field list or the TRaSH guide's field map.
fn deserialize_fields<'de, D>(deserializer: D) -> Result<Vec<Field>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Array(entries) => entries
            .into_iter()
            .map(|entry| serde_json::from_value(entry).map_err(serde::de::Error::custom))
            .collect(),
        Value::Object(entries) => Ok(entries
            .into_iter()
            .map(|(name, value)| Field { name, value })
            .collect()),
        _ => Err(serde::de::Error::custom(
            "fields must be a list of field objects or a map",
        )),
    }
}
