//! Quality profiles (docs/04-database.md, "Library tables"). The items are
//! stored as one JSON list; `core::profile` defines and validates them.

use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait,
    PaginatorTrait, QueryFilter, QueryOrder, Set, SqlErr, Statement,
};
use unseen_core::profile::{LANGUAGE_ANY, default_profiles};

use crate::entities::{movie_profiles, quality_profiles};

/// Settings key that records that the default profiles were created, so that
/// deleting them all does not bring them back at the next start.
const SEEDED_KEY: &str = "seeded.quality_profiles";

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
}

#[derive(Debug)]
pub enum ProfileError {
    NotFound,
    NameTaken,
    /// A movie uses the profile.
    InUse,
    Db(DbErr),
}

impl From<DbErr> for ProfileError {
    fn from(error: DbErr) -> Self {
        if matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) {
            Self::NameTaken
        } else {
            Self::Db(error)
        }
    }
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

pub async fn create(
    db: &DatabaseConnection,
    values: Values,
) -> Result<quality_profiles::Model, ProfileError> {
    let now = crate::now();
    Ok(quality_profiles::ActiveModel {
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
    .insert(db)
    .await?)
}

/// Replaces every editable field.
pub async fn update(
    db: &DatabaseConnection,
    id: i32,
    values: Values,
) -> Result<quality_profiles::Model, ProfileError> {
    let mut profile: quality_profiles::ActiveModel =
        find(db, id).await?.ok_or(ProfileError::NotFound)?.into();
    profile.name = Set(values.name);
    profile.upgrade_allowed = Set(values.upgrade_allowed);
    profile.cutoff_quality_id = Set(values.cutoff_quality_id);
    profile.items = Set(values.items);
    profile.language = Set(values.language);
    profile.min_format_score = Set(values.min_format_score);
    profile.cutoff_format_score = Set(values.cutoff_format_score);
    profile.min_upgrade_format_score = Set(values.min_upgrade_format_score);
    profile.updated_at = Set(crate::now());
    Ok(profile.update(db).await?)
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
}
