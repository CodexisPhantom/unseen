//! Root folders (docs/04-database.md, "Library tables").

use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, Set, SqlErr,
};

use crate::entities::{movies, root_folders};

#[derive(Debug)]
pub enum RootFolderError {
    NotFound,
    PathTaken,
    /// A movie lives in the root folder.
    InUse,
    Db(DbErr),
}

impl From<DbErr> for RootFolderError {
    fn from(error: DbErr) -> Self {
        match error.sql_err() {
            Some(SqlErr::UniqueConstraintViolation(_)) => Self::PathTaken,
            Some(SqlErr::ForeignKeyConstraintViolation(_)) => Self::InUse,
            _ => Self::Db(error),
        }
    }
}

/// Every root folder, by path.
pub async fn list(db: &DatabaseConnection) -> Result<Vec<root_folders::Model>, DbErr> {
    root_folders::Entity::find()
        .order_by_asc(root_folders::Column::Path)
        .all(db)
        .await
}

pub async fn find(db: &DatabaseConnection, id: i32) -> Result<Option<root_folders::Model>, DbErr> {
    root_folders::Entity::find_by_id(id).one(db).await
}

pub async fn create(
    db: &DatabaseConnection,
    path: String,
) -> Result<root_folders::Model, RootFolderError> {
    let library_id = crate::libraries::default_id(db)
        .await?
        .ok_or_else(|| RootFolderError::Db(DbErr::Custom("no library exists".into())))?;
    create_in_library(db, path, library_id).await
}

pub async fn create_in_library(
    db: &DatabaseConnection,
    path: String,
    library_id: i32,
) -> Result<root_folders::Model, RootFolderError> {
    Ok(root_folders::ActiveModel {
        path: Set(path),
        library_id: Set(library_id),
        created_at: Set(crate::now()),
        ..Default::default()
    }
    .insert(db)
    .await?)
}

/// Moves a root folder to another library.
pub async fn set_library(
    db: &DatabaseConnection,
    id: i32,
    library_id: i32,
) -> Result<root_folders::Model, RootFolderError> {
    let current = root_folders::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or(RootFolderError::NotFound)?;
    let mut active: root_folders::ActiveModel = current.into();
    active.library_id = Set(library_id);
    Ok(active.update(db).await?)
}

/// Deletes a root folder that no movie uses.
pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<(), RootFolderError> {
    let used = movies::Entity::find()
        .filter(movies::Column::RootFolderId.eq(id))
        .count(db)
        .await?;
    if used > 0 {
        return Err(RootFolderError::InUse);
    }
    if root_folders::Entity::delete_by_id(id)
        .exec(db)
        .await?
        .rows_affected
        == 0
    {
        return Err(RootFolderError::NotFound);
    }
    Ok(())
}

/// The folder names of the movies in a root folder.
pub async fn movie_folder_names(db: &DatabaseConnection, id: i32) -> Result<Vec<String>, DbErr> {
    movies::Entity::find()
        .select_only()
        .column(movies::Column::FolderName)
        .filter(movies::Column::RootFolderId.eq(id))
        .into_tuple()
        .all(db)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_db;

    #[tokio::test]
    async fn create_list_and_delete() {
        let (db, _dir) = temp_db().await;
        let b = create(&db, "/b".into()).await.unwrap();
        create(&db, "/a".into()).await.unwrap();
        assert!(matches!(
            create(&db, "/a".into()).await,
            Err(RootFolderError::PathTaken)
        ));
        let paths: Vec<_> = list(&db)
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.path)
            .collect();
        assert_eq!(paths, ["/a", "/b"]);
        assert_eq!(find(&db, b.id).await.unwrap().unwrap().path, "/b");
        assert!(movie_folder_names(&db, b.id).await.unwrap().is_empty());

        let movie = movies::ActiveModel {
            tmdb_id: Set(603),
            title: Set("The Matrix".into()),
            sort_title: Set("matrix".into()),
            year: Set(1999),
            status: Set("released".into()),
            root_folder_id: Set(b.id),
            folder_name: Set("The Matrix (1999)".into()),
            monitored: Set(true),
            minimum_availability: Set("released".into()),
            added_at: Set(crate::now()),
            updated_at: Set(crate::now()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        assert_eq!(
            movie_folder_names(&db, b.id).await.unwrap(),
            ["The Matrix (1999)"]
        );
        assert!(matches!(
            delete(&db, b.id).await,
            Err(RootFolderError::InUse)
        ));
        movies::Entity::delete_by_id(movie.id)
            .exec(&db)
            .await
            .unwrap();

        delete(&db, b.id).await.unwrap();
        assert!(matches!(
            delete(&db, b.id).await,
            Err(RootFolderError::NotFound)
        ));
        assert!(find(&db, b.id).await.unwrap().is_none());
    }
}
