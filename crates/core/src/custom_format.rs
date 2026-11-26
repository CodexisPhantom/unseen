//! Radarr custom formats and their matching rules.
//!
//! A specification matches a release on its own, and a format combines its
//! specifications: every required one must match, and each implementation
//! group needs at least one match.

use fancy_regex::Regex;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

#[cfg(test)]
mod tests;

const BYTES_PER_GIB: f64 = 1024.0 * 1024.0 * 1024.0;

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
                    release.languages.iter().any(|language| *language != value)
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
