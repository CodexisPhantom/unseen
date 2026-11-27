//! Quality profiles (docs/04-database.md, "Library tables"). The items are
//! stored as one JSON list; `core::profile` defines and validates them.

use std::collections::{BTreeSet, HashMap};

use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait,
    PaginatorTrait, QueryFilter, QueryOrder, Set, SqlErr, Statement, TransactionTrait,
};
use unseenarr_core::profile::{LANGUAGE_ANY, default_profiles};

use crate::entities::{
    custom_formats, movie_profiles, quality_profile_custom_formats, quality_profiles,
};

/// Settings key that records that the default profiles were created, so that
/// deleting them all does not bring them back at the next start.
const SEEDED_KEY: &str = "seeded.quality_profiles";

/// A custom format score entry on a quality profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatScore {
    pub custom_format_id: i32,
    pub score: i32,
}

/// One custom format with its name and the score a profile assigns to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileFormatScore {
    pub custom_format_id: i32,
    pub name: String,
    pub score: i32,
}

/// A stored custom format paired with the score a quality profile assigns to it.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoredCustomFormat {
    pub custom_format: custom_formats::Model,
    pub score: i32,
}

/// What a caller sets on a profile.
#[derive(Debug, Clone, PartialEq)]
pub struct Values {
    pub name: String,
    pub upgrade_allowed: bool,
    pub cutoff_quality_id: i32,
    pub items: serde_json::Value,
    pub language: String,
    pub min_format_score: i32,
    pub cutoff_format_score: i32,
    pub min_upgrade_format_score: i32,
    pub format_scores: Vec<FormatScore>,
}

#[derive(Debug)]
pub enum ProfileError {
    NotFound,
    NameTaken,
    /// A movie uses the profile.
    InUse,
    /// A `format_scores` entry references a custom format id that does not exist.
    UnknownCustomFormat,
    Db(DbErr),
}

impl From<DbErr> for ProfileError {
    fn from(error: DbErr) -> Self {
        match error.sql_err() {
            Some(SqlErr::UniqueConstraintViolation(_)) => Self::NameTaken,
            Some(SqlErr::ForeignKeyConstraintViolation(_)) => Self::UnknownCustomFormat,
            _ => Self::Db(error),
        }
    }
}

async fn replace_format_scores(
    txn: &impl ConnectionTrait,
    profile_id: i32,
    scores: &[FormatScore],
) -> Result<(), ProfileError> {
    if !scores.is_empty() {
        let ids: BTreeSet<i32> = scores.iter().map(|entry| entry.custom_format_id).collect();
        let found = custom_formats::Entity::find()
            .filter(custom_formats::Column::Id.is_in(ids.iter().copied()))
            .count(txn)
            .await?;
        if found as usize != ids.len() {
            return Err(ProfileError::UnknownCustomFormat);
        }
    }
    quality_profile_custom_formats::Entity::delete_many()
        .filter(quality_profile_custom_formats::Column::QualityProfileId.eq(profile_id))
        .exec(txn)
        .await?;
    for entry in scores {
        if entry.score == 0 {
            continue;
        }
        quality_profile_custom_formats::ActiveModel {
            quality_profile_id: Set(profile_id),
            custom_format_id: Set(entry.custom_format_id),
            score: Set(entry.score),
        }
        .insert(txn)
        .await?;
    }
    Ok(())
}

/// Every profile, by name.
pub async fn list(db: &DatabaseConnection) -> Result<Vec<quality_profiles::Model>, DbErr> {
    quality_profiles::Entity::find()
        .order_by_asc(quality_profiles::Column::Name)
        .all(db)
        .await
}

pub async fn find(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<quality_profiles::Model>, DbErr> {
    quality_profiles::Entity::find_by_id(id).one(db).await
}

/// Every custom format ordered by name, with the score assigned by `profile_id`
/// (defaulting to `0`).
pub async fn format_scores(
    db: &DatabaseConnection,
    profile_id: i32,
) -> Result<Vec<ProfileFormatScore>, DbErr> {
    let mut map = format_scores_for_profiles(db, &[profile_id]).await?;
    Ok(map.remove(&profile_id).unwrap_or_default())
}

/// Loads [`format_scores`] for many profiles in two queries (all custom formats
/// and all score rows for the given profiles).
pub async fn format_scores_for_profiles(
    db: &DatabaseConnection,
    profile_ids: &[i32],
) -> Result<HashMap<i32, Vec<ProfileFormatScore>>, DbErr> {
    if profile_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let formats = custom_formats::Entity::find()
        .order_by_asc(custom_formats::Column::Name)
        .all(db)
        .await?;
    if formats.is_empty() {
        return Ok(profile_ids.iter().map(|&id| (id, Vec::new())).collect());
    }
    let rows = quality_profile_custom_formats::Entity::find()
        .filter(
            quality_profile_custom_formats::Column::QualityProfileId
                .is_in(profile_ids.iter().copied()),
        )
        .all(db)
        .await?;
    let mut by_profile: HashMap<i32, HashMap<i32, i32>> = HashMap::new();
    for row in rows {
        by_profile
            .entry(row.quality_profile_id)
            .or_default()
            .insert(row.custom_format_id, row.score);
    }
    let mut result = HashMap::with_capacity(profile_ids.len());
    for &profile_id in profile_ids {
        let scores = by_profile.get(&profile_id);
        let items = formats
            .iter()
            .map(|cf| ProfileFormatScore {
                custom_format_id: cf.id,
                name: cf.name.clone(),
                score: scores.and_then(|map| map.get(&cf.id).copied()).unwrap_or(0),
            })
            .collect();
        result.insert(profile_id, items);
    }
    Ok(result)
}

/// Loads custom formats that have a non-zero score on `profile_id`.
pub async fn scored_custom_formats(
    db: &DatabaseConnection,
    profile_id: i32,
) -> Result<Vec<ScoredCustomFormat>, DbErr> {
    let score_rows = quality_profile_custom_formats::Entity::find()
        .filter(quality_profile_custom_formats::Column::QualityProfileId.eq(profile_id))
        .all(db)
        .await?;
    let score_map: HashMap<i32, i32> = score_rows
        .into_iter()
        .filter(|row| row.score != 0)
        .map(|row| (row.custom_format_id, row.score))
        .collect();
    if score_map.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<i32> = score_map.keys().copied().collect();
    let models = custom_formats::Entity::find()
        .filter(custom_formats::Column::Id.is_in(ids))
        .all(db)
        .await?;
    Ok(models
        .into_iter()
        .filter_map(|custom_format| {
            let score = score_map.get(&custom_format.id).copied()?;
            Some(ScoredCustomFormat {
                custom_format,
                score,
            })
        })
        .collect())
}

pub async fn create(
    db: &DatabaseConnection,
    values: Values,
) -> Result<quality_profiles::Model, ProfileError> {
    let txn = db.begin().await?;
    let now = crate::now();
    let created = quality_profiles::ActiveModel {
        name: Set(values.name),
        upgrade_allowed: Set(values.upgrade_allowed),
        cutoff_quality_id: Set(values.cutoff_quality_id),
        items: Set(values.items),
        language: Set(values.language),
        min_format_score: Set(values.min_format_score),
        cutoff_format_score: Set(values.cutoff_format_score),
        min_upgrade_format_score: Set(values.min_upgrade_format_score),
        created_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&txn)
    .await?;
    replace_format_scores(&txn, created.id, &values.format_scores).await?;
    txn.commit().await?;
    Ok(created)
}

/// Replaces every editable field.
pub async fn update(
    db: &DatabaseConnection,
    id: i32,
    values: Values,
) -> Result<quality_profiles::Model, ProfileError> {
    let txn = db.begin().await?;
    let mut profile: quality_profiles::ActiveModel = quality_profiles::Entity::find_by_id(id)
        .one(&txn)
        .await?
        .ok_or(ProfileError::NotFound)?
        .into();
    profile.name = Set(values.name);
    profile.upgrade_allowed = Set(values.upgrade_allowed);
    profile.cutoff_quality_id = Set(values.cutoff_quality_id);
    profile.items = Set(values.items);
    profile.language = Set(values.language);
    profile.min_format_score = Set(values.min_format_score);
    profile.cutoff_format_score = Set(values.cutoff_format_score);
    profile.min_upgrade_format_score = Set(values.min_upgrade_format_score);
    profile.updated_at = Set(crate::now());
    let updated = profile.update(&txn).await?;
    replace_format_scores(&txn, id, &values.format_scores).await?;
    txn.commit().await?;
    Ok(updated)
}

/// Deletes a profile that no movie uses.
pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<(), ProfileError> {
    let used = movie_profiles::Entity::find()
        .filter(movie_profiles::Column::QualityProfileId.eq(id))
        .count(db)
        .await?;
    if used > 0 {
        return Err(ProfileError::InUse);
    }
    if quality_profiles::Entity::delete_by_id(id)
        .exec(db)
        .await?
        .rows_affected
        == 0
    {
        return Err(ProfileError::NotFound);
    }
    Ok(())
}

/// How many movies use the profile.
pub async fn movie_count(db: &DatabaseConnection, id: i32) -> Result<u64, DbErr> {
    movie_profiles::Entity::find()
        .filter(movie_profiles::Column::QualityProfileId.eq(id))
        .count(db)
        .await
}

/// Creates Radarr's default profiles once, on a database that has none.
pub(crate) async fn seed_defaults(db: &DatabaseConnection) -> Result<(), DbErr> {
    let backend = db.get_database_backend();
    let done = db
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            "SELECT 1 AS done FROM settings WHERE key = ?",
            [SEEDED_KEY.into()],
        ))
        .await?;
    if done.is_some() {
        return Ok(());
    }
    if quality_profiles::Entity::find().count(db).await? == 0 {
        for default in default_profiles() {
            create(
                db,
                Values {
                    name: default.name.to_owned(),
                    upgrade_allowed: true,
                    cutoff_quality_id: default.cutoff_quality_id,
                    items: serde_json::to_value(default.items)
                        .map_err(|error| DbErr::Custom(error.to_string()))?,
                    language: LANGUAGE_ANY.to_owned(),
                    min_format_score: 0,
                    cutoff_format_score: 0,
                    min_upgrade_format_score: 1,
                    format_scores: Vec::new(),
                },
            )
            .await
            .map_err(|error| match error {
                ProfileError::Db(error) => error,
                other => DbErr::Custom(format!("seed default profiles: {other:?}")),
            })?;
        }
    }
    db.execute_raw(Statement::from_sql_and_values(
        backend,
        "INSERT INTO settings (key, value, updated_at) VALUES (?, 'true', ?)",
        [SEEDED_KEY.into(), crate::now().into()],
    ))
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_db;

    fn values(name: &str) -> Values {
        Values {
            name: name.into(),
            upgrade_allowed: false,
            cutoff_quality_id: 7,
            items: serde_json::json!([{ "type": "quality", "qualityId": 7, "allowed": true }]),
            language: "any".into(),
            min_format_score: 0,
            cutoff_format_score: 0,
            min_upgrade_format_score: 1,
            format_scores: Vec::new(),
        }
    }

    #[tokio::test]
    async fn a_new_database_has_the_default_profiles_once() {
        let (db, dir) = temp_db().await;
        let names: Vec<_> = list(&db)
            .await
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(
            names,
            [
                "Any",
                "HD - 720p/1080p",
                "HD-1080p",
                "HD-720p",
                "SD",
                "Ultra-HD"
            ]
        );

        // Deleting them all and restarting does not bring them back.
        for profile in list(&db).await.unwrap() {
            delete(&db, profile.id).await.unwrap();
        }
        db.close().await.unwrap();
        let db = crate::connect(dir.path()).await.unwrap();
        assert!(list(&db).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_update_and_delete() {
        let (db, _dir) = temp_db().await;
        let created = create(&db, values("Mine")).await.unwrap();
        assert!(matches!(
            create(&db, values("Mine")).await,
            Err(ProfileError::NameTaken)
        ));
        let mut changed = values("Mine 2");
        changed.upgrade_allowed = true;
        let updated = update(&db, created.id, changed).await.unwrap();
        assert_eq!(
            (updated.name.as_str(), updated.upgrade_allowed),
            ("Mine 2", true)
        );
        assert!(matches!(
            update(&db, 9999, values("x")).await,
            Err(ProfileError::NotFound)
        ));
        delete(&db, created.id).await.unwrap();
        assert!(matches!(
            delete(&db, created.id).await,
            Err(ProfileError::NotFound)
        ));
    }

    #[tokio::test]
    async fn stores_reads_and_validates_custom_format_scores() {
        let (db, _dir) = temp_db().await;
        let now = crate::now();
        let hdr = custom_formats::ActiveModel {
            name: Set("HDR".into()),
            include_when_renaming: Set(false),
            specifications: Set(serde_json::json!([])),
            trash_id: Set(None),
            trash_synced: Set(false),
            created_at: Set(now.clone()),
            updated_at: Set(now.clone()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let atmos = custom_formats::ActiveModel {
            name: Set("Atmos".into()),
            include_when_renaming: Set(false),
            specifications: Set(serde_json::json!([])),
            trash_id: Set(None),
            trash_synced: Set(false),
            created_at: Set(now.clone()),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let mut v = values("Scored");
        v.min_format_score = 100;
        v.cutoff_format_score = 500;
        v.format_scores = vec![
            FormatScore {
                custom_format_id: hdr.id,
                score: 300,
            },
            FormatScore {
                custom_format_id: atmos.id,
                score: 0,
            },
        ];
        let created = create(&db, v).await.unwrap();
        let scores = format_scores(&db, created.id).await.unwrap();
        assert_eq!(
            scores,
            vec![
                ProfileFormatScore {
                    custom_format_id: atmos.id,
                    name: "Atmos".into(),
                    score: 0,
                },
                ProfileFormatScore {
                    custom_format_id: hdr.id,
                    name: "HDR".into(),
                    score: 300,
                },
            ]
        );
        let scored = scored_custom_formats(&db, created.id).await.unwrap();
        assert_eq!(scored.len(), 1);
        assert_eq!(scored[0].custom_format.id, hdr.id);
        assert_eq!(scored[0].score, 300);

        // Unknown custom format id is rejected.
        let mut bad = values("Bad");
        bad.format_scores = vec![FormatScore {
            custom_format_id: 9999,
            score: 10,
        }];
        assert!(matches!(
            create(&db, bad).await,
            Err(ProfileError::UnknownCustomFormat)
        ));
    }
}
