//! Libraries as named groups of root folders (docs/04-database.md, ADR 0012).

use std::collections::{BTreeMap, BTreeSet};

use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, Set, SqlErr,
};

use crate::entities::{libraries, root_folders, user_permission_libraries, user_role_libraries};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryDetail {
    pub library: libraries::Model,
    pub root_folder_ids: Vec<i32>,
}

#[derive(Debug)]
pub enum LibraryError {
    NotFound,
    NameTaken,
    /// A root folder still belongs to this library.
    InUse,
    Db(DbErr),
}

impl From<DbErr> for LibraryError {
    fn from(error: DbErr) -> Self {
        match error.sql_err() {
            Some(SqlErr::UniqueConstraintViolation(_)) => Self::NameTaken,
            Some(SqlErr::ForeignKeyConstraintViolation(_)) => Self::InUse,
            _ => Self::Db(error),
        }
    }
}

/// Every library, ordered by name, with its root folder ids.
pub async fn list(db: &DatabaseConnection) -> Result<Vec<LibraryDetail>, DbErr> {
    let rows = libraries::Entity::find()
        .order_by_asc(libraries::Column::Name)
        .all(db)
        .await?;
    let mut folders_by_library: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
    for folder in root_folders::Entity::find()
        .order_by_asc(root_folders::Column::Path)
        .all(db)
        .await?
    {
        folders_by_library
            .entry(folder.library_id)
            .or_default()
            .push(folder.id);
    }
    Ok(rows
        .into_iter()
        .map(|library| {
            let root_folder_ids = folders_by_library.remove(&library.id).unwrap_or_default();
            LibraryDetail {
                library,
                root_folder_ids,
            }
        })
        .collect())
}

/// One library by id, with its root folder ids.
pub async fn find(db: &DatabaseConnection, id: i32) -> Result<Option<LibraryDetail>, DbErr> {
    let Some(library) = libraries::Entity::find_by_id(id).one(db).await? else {
        return Ok(None);
    };
    let root_folder_ids = root_folders::Entity::find()
        .filter(root_folders::Column::LibraryId.eq(id))
        .order_by_asc(root_folders::Column::Path)
        .all(db)
        .await?
        .into_iter()
        .map(|folder| folder.id)
        .collect();
    Ok(Some(LibraryDetail {
        library,
        root_folder_ids,
    }))
}

/// The default library id: the lowest-id library (seeded as `Default` by
/// `m0015_libraries`).
pub async fn default_id(db: &DatabaseConnection) -> Result<Option<i32>, DbErr> {
    Ok(libraries::Entity::find()
        .order_by_asc(libraries::Column::Id)
        .one(db)
        .await?
        .map(|library| library.id))
}

/// Creates a new library with the given unique name.
pub async fn create(db: &DatabaseConnection, name: String) -> Result<LibraryDetail, LibraryError> {
    let now = crate::now();
    let library = libraries::ActiveModel {
        name: Set(name),
        created_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(LibraryDetail {
        library,
        root_folder_ids: Vec::new(),
    })
}

/// Renames a library.
pub async fn rename(
    db: &DatabaseConnection,
    id: i32,
    name: String,
) -> Result<LibraryDetail, LibraryError> {
    let current = libraries::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or(LibraryError::NotFound)?;
    let mut active: libraries::ActiveModel = current.into();
    active.name = Set(name);
    active.updated_at = Set(crate::now());
    let library = active.update(db).await?;
    let root_folder_ids = root_folders::Entity::find()
        .filter(root_folders::Column::LibraryId.eq(id))
        .order_by_asc(root_folders::Column::Path)
        .all(db)
        .await?
        .into_iter()
        .map(|folder| folder.id)
        .collect();
    Ok(LibraryDetail {
        library,
        root_folder_ids,
    })
}

/// Deletes a library that holds no root folders.
pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<(), LibraryError> {
    let used = root_folders::Entity::find()
        .filter(root_folders::Column::LibraryId.eq(id))
        .count(db)
        .await?;
    if used > 0 {
        return Err(LibraryError::InUse);
    }
    if libraries::Entity::delete_by_id(id)
        .exec(db)
        .await?
        .rows_affected
        == 0
    {
        return Err(LibraryError::NotFound);
    }
    Ok(())
}

/// User ids whose scoped role assignments or direct grants reference `library_id`.
pub async fn users_scoped_to_library(
    db: &DatabaseConnection,
    library_id: i32,
) -> Result<BTreeSet<String>, DbErr> {
    let mut users = BTreeSet::new();
    for row in user_role_libraries::Entity::find()
        .filter(user_role_libraries::Column::LibraryId.eq(library_id))
        .all(db)
        .await?
    {
        users.insert(row.user_id);
    }
    for row in user_permission_libraries::Entity::find()
        .filter(user_permission_libraries::Column::LibraryId.eq(library_id))
        .all(db)
        .await?
    {
        users.insert(row.user_id);
    }
    Ok(users)
}

/// Map from root folder id to its `library_id`.
pub async fn root_folder_library_map(db: &DatabaseConnection) -> Result<BTreeMap<i32, i32>, DbErr> {
    Ok(root_folders::Entity::find()
        .all(db)
        .await?
        .into_iter()
        .map(|folder| (folder.id, folder.library_id))
        .collect())
}

/// The `library_id` of a root folder, if the root folder exists.
pub async fn library_of_root_folder(
    db: &DatabaseConnection,
    root_folder_id: i32,
) -> Result<Option<i32>, DbErr> {
    Ok(root_folders::Entity::find_by_id(root_folder_id)
        .one(db)
        .await?
        .map(|folder| folder.library_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_db;

    #[tokio::test]
    async fn default_library_exists_and_crud_enforces_uniqueness_and_in_use() {
        let (db, _dir) = temp_db().await;
        let initial = list(&db).await.unwrap();
        assert_eq!(initial.len(), 1);
        assert_eq!(initial[0].library.name, "Default");
        assert_eq!(default_id(&db).await.unwrap(), Some(initial[0].library.id));

        let kids = create(&db, "Kids".into()).await.unwrap();
        assert_eq!(kids.library.name, "Kids");
        assert!(matches!(
            create(&db, "Kids".into()).await,
            Err(LibraryError::NameTaken)
        ));

        let renamed = rename(&db, kids.library.id, "Family".into()).await.unwrap();
        assert_eq!(renamed.library.name, "Family");

        let folder = crate::root_folders::create_in_library(&db, "/kids".into(), kids.library.id)
            .await
            .unwrap();
        assert_eq!(
            library_of_root_folder(&db, folder.id).await.unwrap(),
            Some(kids.library.id)
        );
        assert!(matches!(
            delete(&db, kids.library.id).await,
            Err(LibraryError::InUse)
        ));

        crate::root_folders::delete(&db, folder.id).await.unwrap();
        delete(&db, kids.library.id).await.unwrap();
        assert!(find(&db, kids.library.id).await.unwrap().is_none());
    }
}
