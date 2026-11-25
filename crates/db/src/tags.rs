//! Tags (docs/04-database.md, "Library tables").

use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, Set, SqlErr,
};

use crate::entities::{movie_tags, tags};

#[derive(Debug)]
pub enum TagError {
    NotFound,
    LabelTaken,
    /// A movie carries the tag.
    InUse,
    Db(DbErr),
}

impl From<DbErr> for TagError {
    fn from(error: DbErr) -> Self {
        if matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) {
            Self::LabelTaken
        } else {
            Self::Db(error)
        }
    }
}

/// Every tag, by label.
pub async fn list(db: &DatabaseConnection) -> Result<Vec<tags::Model>, DbErr> {
    tags::Entity::find()
        .order_by_asc(tags::Column::Label)
        .all(db)
        .await
}

pub async fn create(db: &DatabaseConnection, label: String) -> Result<tags::Model, TagError> {
    Ok(tags::ActiveModel {
        label: Set(label),
        ..Default::default()
    }
    .insert(db)
    .await?)
}

pub async fn rename(
    db: &DatabaseConnection,
    id: i32,
    label: String,
) -> Result<tags::Model, TagError> {
    let mut tag: tags::ActiveModel = tags::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or(TagError::NotFound)?
        .into();
    tag.label = Set(label);
    Ok(tag.update(db).await?)
}

pub async fn find(db: &DatabaseConnection, id: i32) -> Result<Option<tags::Model>, DbErr> {
    tags::Entity::find_by_id(id).one(db).await
}

/// Deletes a tag that no movie carries.
pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<(), TagError> {
    let used = movie_tags::Entity::find()
        .filter(movie_tags::Column::TagId.eq(id))
        .count(db)
        .await?;
    if used > 0 {
        return Err(TagError::InUse);
    }
    if tags::Entity::delete_by_id(id).exec(db).await?.rows_affected == 0 {
        return Err(TagError::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{movies, root_folders};
    use crate::testing::temp_db;

    #[tokio::test]
    async fn create_rename_list_and_delete() {
        let (db, _dir) = temp_db().await;
        let b = create(&db, "b".into()).await.unwrap();
        create(&db, "a".into()).await.unwrap();
        assert!(matches!(
            create(&db, "a".into()).await,
            Err(TagError::LabelTaken)
        ));
        assert!(matches!(
            rename(&db, b.id, "a".into()).await,
            Err(TagError::LabelTaken)
        ));
        assert_eq!(rename(&db, b.id, "c".into()).await.unwrap().label, "c");
        assert!(matches!(
            rename(&db, 999, "x".into()).await,
            Err(TagError::NotFound)
        ));
        let labels: Vec<_> = list(&db)
            .await
            .unwrap()
            .into_iter()
            .map(|t| t.label)
            .collect();
        assert_eq!(labels, ["a", "c"]);

        delete(&db, b.id).await.unwrap();
        assert!(matches!(delete(&db, b.id).await, Err(TagError::NotFound)));
    }

    #[tokio::test]
    async fn a_tag_in_use_is_not_deleted() {
        let (db, _dir) = temp_db().await;
        let tag = create(&db, "4k".into()).await.unwrap();
        let root = root_folders::ActiveModel {
            path: Set("/m".into()),
            created_at: Set(crate::now()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let movie = movies::ActiveModel {
            tmdb_id: Set(1),
            title: Set("M".into()),
            sort_title: Set("m".into()),
            year: Set(2020),
            status: Set("released".into()),
            root_folder_id: Set(root.id),
            folder_name: Set("M (2020)".into()),
            monitored: Set(true),
            minimum_availability: Set("released".into()),
            added_at: Set(crate::now()),
            updated_at: Set(crate::now()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        movie_tags::ActiveModel {
            movie_id: Set(movie.id),
            tag_id: Set(tag.id),
        }
        .insert(&db)
        .await
        .unwrap();
        assert!(matches!(delete(&db, tag.id).await, Err(TagError::InUse)));
    }
}
