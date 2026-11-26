//! Custom formats and their quality profile scores (docs/04-database.md).

use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
    Set, SqlErr, TransactionTrait,
};

use crate::entities::{custom_formats, quality_profile_custom_formats};

/// What a caller sets on a custom format.
#[derive(Debug, Clone, PartialEq)]
pub struct Values {
    pub name: String,
    pub include_when_renaming: bool,
    pub specifications: serde_json::Value,
    pub trash_id: Option<String>,
    pub trash_synced: bool,
}

#[derive(Debug)]
pub enum CustomFormatError {
    NotFound,
    NameTaken,
    Db(DbErr),
}

impl From<DbErr> for CustomFormatError {
    fn from(error: DbErr) -> Self {
        if matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) {
            Self::NameTaken
        } else {
            Self::Db(error)
        }
    }
}

/// Every custom format, ordered by name.
pub async fn list(db: &DatabaseConnection) -> Result<Vec<custom_formats::Model>, DbErr> {
    custom_formats::Entity::find()
        .order_by_asc(custom_formats::Column::Name)
        .all(db)
        .await
}

pub async fn find(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<custom_formats::Model>, DbErr> {
    custom_formats::Entity::find_by_id(id).one(db).await
}

pub async fn find_by_name(
    db: &DatabaseConnection,
    name: &str,
) -> Result<Option<custom_formats::Model>, DbErr> {
    custom_formats::Entity::find()
        .filter(custom_formats::Column::Name.eq(name))
        .one(db)
        .await
}

pub async fn create(
    db: &DatabaseConnection,
    values: Values,
) -> Result<custom_formats::Model, CustomFormatError> {
    let now = crate::now();
    Ok(custom_formats::ActiveModel {
        name: Set(values.name),
        include_when_renaming: Set(values.include_when_renaming),
        specifications: Set(values.specifications),
        trash_id: Set(values.trash_id),
        trash_synced: Set(values.trash_synced),
        created_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?)
}

/// Creates multiple custom formats atomically in a single transaction.
pub async fn create_many(
    db: &DatabaseConnection,
    items: Vec<Values>,
) -> Result<Vec<custom_formats::Model>, CustomFormatError> {
    let tx = db.begin().await?;
    let mut created = Vec::with_capacity(items.len());
    for values in items {
        let now = crate::now();
        let model = custom_formats::ActiveModel {
            name: Set(values.name),
            include_when_renaming: Set(values.include_when_renaming),
            specifications: Set(values.specifications),
            trash_id: Set(values.trash_id),
            trash_synced: Set(values.trash_synced),
            created_at: Set(now.clone()),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&tx)
        .await?;
        created.push(model);
    }
    tx.commit().await?;
    Ok(created)
}

/// Replaces every editable field of a custom format.
pub async fn update(
    db: &DatabaseConnection,
    id: i32,
    values: Values,
) -> Result<custom_formats::Model, CustomFormatError> {
    let mut format: custom_formats::ActiveModel = find(db, id)
        .await?
        .ok_or(CustomFormatError::NotFound)?
        .into();
    format.name = Set(values.name);
    format.include_when_renaming = Set(values.include_when_renaming);
    format.specifications = Set(values.specifications);
    format.trash_id = Set(values.trash_id);
    format.trash_synced = Set(values.trash_synced);
    format.updated_at = Set(crate::now());
    Ok(format.update(db).await?)
}

/// Deletes a custom format. Its scores in `quality_profile_custom_formats` are
/// removed by `ON DELETE CASCADE`.
pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<(), CustomFormatError> {
    if custom_formats::Entity::delete_by_id(id)
        .exec(db)
        .await?
        .rows_affected
        == 0
    {
        return Err(CustomFormatError::NotFound);
    }
    Ok(())
}

/// Every custom format score assigned across all quality profiles.
pub async fn list_profile_scores(
    db: &DatabaseConnection,
) -> Result<Vec<quality_profile_custom_formats::Model>, DbErr> {
    quality_profile_custom_formats::Entity::find().all(db).await
}

#[cfg(test)]
mod tests {
    use sea_orm::{ActiveModelTrait, Set};
    use serde_json::json;

    use super::*;
    use crate::entities::quality_profile_custom_formats;
    use crate::testing::temp_db;

    fn sample(name: &str) -> Values {
        Values {
            name: name.into(),
            include_when_renaming: false,
            specifications: json!([]),
            trash_id: None,
            trash_synced: false,
        }
    }

    #[tokio::test]
    async fn create_update_and_delete_cascades_profile_scores() {
        let (db, _dir) = temp_db().await;
        let created = create(&db, sample("HDR")).await.unwrap();
        assert!(matches!(
            create(&db, sample("HDR")).await,
            Err(CustomFormatError::NameTaken)
        ));

        let mut changed = sample("HDR10");
        changed.include_when_renaming = true;
        let updated = update(&db, created.id, changed).await.unwrap();
        assert_eq!(updated.name, "HDR10");
        assert!(updated.include_when_renaming);

        let profiles = crate::profiles::list(&db).await.unwrap();
        quality_profile_custom_formats::ActiveModel {
            quality_profile_id: Set(profiles[0].id),
            custom_format_id: Set(created.id),
            score: Set(500),
        }
        .insert(&db)
        .await
        .unwrap();
        assert_eq!(list_profile_scores(&db).await.unwrap().len(), 1);

        delete(&db, created.id).await.unwrap();
        assert!(list_profile_scores(&db).await.unwrap().is_empty());
        assert!(matches!(
            delete(&db, created.id).await,
            Err(CustomFormatError::NotFound)
        ));
    }
}
