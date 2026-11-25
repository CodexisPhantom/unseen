//! Quality profiles: the ordered items, the cutoff and their validation
//! (docs/07-release-scoring.md, "Quality profile").

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::quality::Quality;

/// Quality groups have ids from this value up, as in Radarr, so a cutoff id
/// names either a quality or a group.
pub const GROUP_ID_MIN: i32 = 1000;

pub const LANGUAGE_ANY: &str = "any";
pub const LANGUAGE_ORIGINAL: &str = "original";

/// One entry of the ordered list, lowest quality first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Item {
    #[serde(rename_all = "camelCase")]
    Quality { quality_id: i32, allowed: bool },
    /// Qualities that count as equal, such as WEBDL-1080p and WEBRip-1080p.
    #[serde(rename_all = "camelCase")]
    Group {
        id: i32,
        name: String,
        quality_ids: Vec<i32>,
        allowed: bool,
    },
}

impl Item {
    fn allowed(&self) -> bool {
        match self {
            Self::Quality { allowed, .. } | Self::Group { allowed, .. } => *allowed,
        }
    }

    /// The id a cutoff uses for this item.
    fn cutoff_id(&self) -> i32 {
        match self {
            Self::Quality { quality_id, .. } => *quality_id,
            Self::Group { id, .. } => *id,
        }
    }
}

/// The fields of a profile that have rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub items: Vec<Item>,
    pub cutoff_quality_id: i32,
    pub language: String,
    pub min_upgrade_format_score: i32,
}

/// The first rule a profile breaks: the field and the message.
pub type Invalid = (&'static str, String);

fn invalid<T>(field: &'static str, message: impl Into<String>) -> Result<T, Invalid> {
    Err((field, message.into()))
}

/// Checks a profile. The cutoff must be an allowed quality or group, every
/// quality appears once, groups have a unique id and name and at least two
/// qualities, and the language is `any`, `original` or a language code.
pub fn validate(profile: &Profile) -> Result<(), Invalid> {
    if profile.name.trim().is_empty() {
        return invalid("name", "must not be empty");
    }
    validate_items(&profile.items)?;
    if !profile
        .items
        .iter()
        .any(|item| item.allowed() && item.cutoff_id() == profile.cutoff_quality_id)
    {
        return invalid(
            "cutoff",
            "must be an allowed quality or group of the profile",
        );
    }
    let language = profile.language.as_str();
    let code =
        (2..=3).contains(&language.len()) && language.bytes().all(|b| b.is_ascii_lowercase());
    if language != LANGUAGE_ANY && language != LANGUAGE_ORIGINAL && !code {
        return invalid(
            "language",
            "must be `any`, `original` or a language code such as `en`",
        );
    }
    if profile.min_upgrade_format_score < 1 {
        return invalid("minUpgradeFormatScore", "must be at least 1");
    }
    Ok(())
}

fn validate_items(items: &[Item]) -> Result<(), Invalid> {
    let mut qualities = BTreeSet::new();
    let mut groups = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut check_quality = |id: i32| -> Result<(), Invalid> {
        if Quality::from_id(id).is_none() {
            return invalid("items", format!("unknown quality {id}"));
        }
        if !qualities.insert(id) {
            return invalid("items", format!("quality {id} appears twice"));
        }
        Ok(())
    };
    for item in items {
        match item {
            Item::Quality { quality_id, .. } => check_quality(*quality_id)?,
            Item::Group {
                id,
                name,
                quality_ids,
                ..
            } => {
                if *id < GROUP_ID_MIN || !groups.insert(*id) {
                    return invalid(
                        "items",
                        format!("group id {id} must be unique and at least {GROUP_ID_MIN}"),
                    );
                }
                if name.trim().is_empty() || !names.insert(name.trim().to_lowercase()) {
                    return invalid("items", "group names must be unique and not empty");
                }
                if quality_ids.len() < 2 {
                    return invalid(
                        "items",
                        format!("group {name} needs at least two qualities"),
                    );
                }
                for quality_id in quality_ids {
                    check_quality(*quality_id)?;
                }
            }
        }
    }
    if !items.iter().any(Item::allowed) {
        return invalid("items", "at least one quality must be allowed");
    }
    Ok(())
}

/// A profile that a new install starts with.
pub struct DefaultProfile {
    pub name: &'static str,
    pub cutoff_quality_id: i32,
    pub items: Vec<Item>,
}

/// Radarr's default profiles: `Any`, `SD`, `HD-720p`, `HD-1080p`,
/// `Ultra-HD` and `HD - 720p/1080p`. Every profile lists all qualities from
/// lowest to highest, with the WEBDL and WEBRip pair of a resolution as one
/// group, and allows the ones Radarr allows.
pub fn default_profiles() -> Vec<DefaultProfile> {
    use Quality as Q;
    let web = [
        (1000, "WEB 480p", Q::WebDl480p, Q::WebRip480p),
        (1001, "WEB 720p", Q::WebDl720p, Q::WebRip720p),
        (1002, "WEB 1080p", Q::WebDl1080p, Q::WebRip1080p),
        (1003, "WEB 2160p", Q::WebDl2160p, Q::WebRip2160p),
    ];
    let items = |allowed: &[Quality]| -> Vec<Item> {
        let mut items = Vec::new();
        for quality in Quality::ALL {
            match web
                .iter()
                .find(|(_, _, first, second)| quality == *first || quality == *second)
            {
                Some((id, name, first, second)) if quality == *first => items.push(Item::Group {
                    id: *id,
                    name: (*name).to_owned(),
                    quality_ids: vec![first.id(), second.id()],
                    allowed: allowed.contains(first),
                }),
                // The second quality of a group is in the group.
                Some(_) => {}
                None => items.push(Item::Quality {
                    quality_id: quality.id(),
                    allowed: allowed.contains(&quality),
                }),
            }
        }
        items
    };
    let profile = |name, cutoff: Quality, allowed: &[Quality]| DefaultProfile {
        name,
        cutoff_quality_id: cutoff.id(),
        items: items(allowed),
    };
    vec![
        profile(
            "Any",
            Q::Bluray1080p,
            &[
                Q::Workprint,
                Q::Cam,
                Q::Telesync,
                Q::Telecine,
                Q::Regional,
                Q::DvdScr,
                Q::Sdtv,
                Q::Dvd,
                Q::DvdR,
                Q::WebDl480p,
                Q::Bluray480p,
                Q::Bluray576p,
                Q::Hdtv720p,
                Q::WebDl720p,
                Q::Bluray720p,
                Q::Hdtv1080p,
                Q::WebDl1080p,
                Q::Bluray1080p,
                Q::Remux1080p,
                Q::Hdtv2160p,
                Q::WebDl2160p,
                Q::Bluray2160p,
                Q::Remux2160p,
            ],
        ),
        profile(
            "SD",
            Q::Bluray480p,
            &[Q::WebDl480p, Q::Bluray480p, Q::Bluray576p],
        ),
        profile(
            "HD-720p",
            Q::Bluray720p,
            &[Q::Hdtv720p, Q::WebDl720p, Q::Bluray720p],
        ),
        profile(
            "HD-1080p",
            Q::Bluray1080p,
            &[Q::Hdtv1080p, Q::WebDl1080p, Q::Bluray1080p, Q::Remux1080p],
        ),
        profile(
            "Ultra-HD",
            Q::Remux2160p,
            &[Q::Hdtv2160p, Q::WebDl2160p, Q::Bluray2160p, Q::Remux2160p],
        ),
        profile(
            "HD - 720p/1080p",
            Q::Bluray1080p,
            &[
                Q::Hdtv720p,
                Q::Hdtv1080p,
                Q::WebDl720p,
                Q::WebDl1080p,
                Q::Bluray720p,
                Q::Bluray1080p,
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quality(id: Quality, allowed: bool) -> Item {
        Item::Quality {
            quality_id: id.id(),
            allowed,
        }
    }

    fn profile() -> Profile {
        Profile {
            name: "HD".into(),
            items: vec![
                quality(Quality::Hdtv1080p, true),
                Item::Group {
                    id: 1002,
                    name: "WEB 1080p".into(),
                    quality_ids: vec![Quality::WebDl1080p.id(), Quality::WebRip1080p.id()],
                    allowed: true,
                },
                quality(Quality::Bluray1080p, false),
            ],
            cutoff_quality_id: 1002,
            language: "any".into(),
            min_upgrade_format_score: 1,
        }
    }

    fn broken(change: impl FnOnce(&mut Profile)) -> &'static str {
        let mut profile = profile();
        change(&mut profile);
        validate(&profile).unwrap_err().0
    }

    #[test]
    fn a_valid_profile_passes() {
        assert_eq!(validate(&profile()), Ok(()));
        for language in ["original", "en", "fra"] {
            let mut p = profile();
            p.language = language.into();
            assert_eq!(validate(&p), Ok(()), "{language}");
        }
    }

    #[test]
    fn the_cutoff_must_be_an_allowed_item() {
        assert_eq!(
            broken(|p| p.cutoff_quality_id = Quality::Bluray1080p.id()),
            "cutoff"
        );
        assert_eq!(broken(|p| p.cutoff_quality_id = 9999), "cutoff");
        // A quality inside a group is not an item of its own.
        assert_eq!(
            broken(|p| p.cutoff_quality_id = Quality::WebDl1080p.id()),
            "cutoff"
        );
    }

    #[test]
    fn items_are_checked() {
        assert_eq!(broken(|p| p.items.clear()), "items");
        assert_eq!(
            broken(|p| p.items.push(quality(Quality::Hdtv1080p, true))),
            "items"
        );
        assert_eq!(
            broken(|p| p.items.push(Item::Quality {
                quality_id: 99,
                allowed: true
            })),
            "items"
        );
        assert_eq!(
            broken(|p| {
                if let Item::Group { quality_ids, .. } = &mut p.items[1] {
                    quality_ids.pop();
                }
            }),
            "items"
        );
        assert_eq!(
            broken(|p| {
                if let Item::Group { id, .. } = &mut p.items[1] {
                    *id = 5;
                }
            }),
            "items"
        );
        // Nothing allowed.
        assert_eq!(
            broken(|p| p.items.iter_mut().for_each(|item| match item {
                Item::Quality { allowed, .. } | Item::Group { allowed, .. } => *allowed = false,
            })),
            "items"
        );
    }

    #[test]
    fn name_language_and_upgrade_score_are_checked() {
        assert_eq!(broken(|p| p.name = "  ".into()), "name");
        assert_eq!(broken(|p| p.language = "English".into()), "language");
        assert_eq!(broken(|p| p.language = String::new()), "language");
        assert_eq!(
            broken(|p| p.min_upgrade_format_score = 0),
            "minUpgradeFormatScore"
        );
    }

    #[test]
    fn the_default_profiles_are_valid_and_use_every_quality_once() {
        let defaults = default_profiles();
        assert_eq!(
            defaults.iter().map(|p| p.name).collect::<Vec<_>>(),
            [
                "Any",
                "SD",
                "HD-720p",
                "HD-1080p",
                "Ultra-HD",
                "HD - 720p/1080p"
            ]
        );
        for default in defaults {
            let profile = Profile {
                name: default.name.into(),
                items: default.items.clone(),
                cutoff_quality_id: default.cutoff_quality_id,
                language: LANGUAGE_ANY.into(),
                min_upgrade_format_score: 1,
            };
            assert_eq!(validate(&profile), Ok(()), "{}", default.name);
            let count: usize = default
                .items
                .iter()
                .map(|item| match item {
                    Item::Quality { .. } => 1,
                    Item::Group { quality_ids, .. } => quality_ids.len(),
                })
                .sum();
            assert_eq!(count, Quality::ALL.len(), "{}", default.name);
        }
    }

    #[test]
    fn items_serialize_with_camel_case_fields() {
        let json = serde_json::to_value(quality(Quality::Dvd, true)).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "type": "quality", "qualityId": 2, "allowed": true })
        );
    }
}
