//! Custom roles and their permissions (docs/15-permissions.md, "Custom roles").

use std::collections::{BTreeMap, BTreeSet};

use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
    Set, SqlErr, TransactionTrait,
};

use crate::entities::{role_permissions, roles, user_roles};

/// A role with its stored permission strings and the number of users holding it.
/// The Owner role stores no permissions: it means the whole catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleDetail {
    pub role: roles::Model,
    pub permissions: BTreeSet<String>,
    pub users: u64,
}

#[derive(Debug)]
pub enum RoleError {
    NotFound,
    NameTaken,
    Db(DbErr),
}

impl From<DbErr> for RoleError {
    fn from(error: DbErr) -> Self {
        if matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) {
            Self::NameTaken
        } else {
            Self::Db(error)
        }
    }
}

/// Fields of a role to change; `None` leaves a field as it is.
#[derive(Debug, Default)]
pub struct RoleChanges {
    pub name: Option<String>,
    /// An empty description clears it.
    pub description: Option<String>,
    pub permissions: Option<BTreeSet<String>>,
}

/// Every role, the Owner role first, then by name.
pub async fn list(db: &DatabaseConnection) -> Result<Vec<RoleDetail>, DbErr> {
    let models = roles::Entity::find()
        .order_by_desc(roles::Column::IsOwner)
        .order_by_asc(roles::Column::Name)
        .all(db)
        .await?;
    let mut permissions: BTreeMap<i32, BTreeSet<String>> = BTreeMap::new();
    for row in role_permissions::Entity::find().all(db).await? {
        permissions
            .entry(row.role_id)
            .or_default()
            .insert(row.permission);
    }
    let mut users: BTreeMap<i32, u64> = BTreeMap::new();
    for row in user_roles::Entity::find().all(db).await? {
        *users.entry(row.role_id).or_default() += 1;
    }
    Ok(models
        .into_iter()
        .map(|role| RoleDetail {
            permissions: permissions.remove(&role.id).unwrap_or_default(),
            users: users.get(&role.id).copied().unwrap_or_default(),
            role,
        })
        .collect())
}

pub async fn find(db: &DatabaseConnection, id: i32) -> Result<Option<RoleDetail>, DbErr> {
    Ok(list(db)
        .await?
        .into_iter()
        .find(|detail| detail.role.id == id))
}

/// Creates a custom role. Fails with `NameTaken` on a duplicate name.
pub async fn create(
    db: &DatabaseConnection,
    name: &str,
    description: Option<&str>,
    permissions: &BTreeSet<String>,
) -> Result<RoleDetail, RoleError> {
    let txn = db.begin().await?;
    let now = crate::now();
    let role = roles::ActiveModel {
        name: Set(name.to_owned()),
        description: Set(description.map(str::to_owned)),
        is_owner: Set(false),
        created_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&txn)
    .await?;
    insert_permissions(&txn, role.id, permissions).await?;
    txn.commit().await?;
    Ok(RoleDetail {
        role,
        permissions: permissions.clone(),
        users: 0,
    })
}

/// Applies `changes` to a custom role. The permission set is replaced as a
/// whole. The caller refuses the Owner role before calling this.
pub async fn update(
    db: &DatabaseConnection,
    id: i32,
    changes: RoleChanges,
) -> Result<RoleDetail, RoleError> {
    let txn = db.begin().await?;
    let mut role: roles::ActiveModel = roles::Entity::find_by_id(id)
        .one(&txn)
        .await?
        .ok_or(RoleError::NotFound)?
        .into();
    if let Some(name) = changes.name {
        role.name = Set(name);
    }
    if let Some(description) = changes.description {
        role.description = Set(Some(description).filter(|text| !text.is_empty()));
    }
    role.updated_at = Set(crate::now());
    role.update(&txn).await?;
    if let Some(permissions) = &changes.permissions {
        role_permissions::Entity::delete_many()
            .filter(role_permissions::Column::RoleId.eq(id))
            .exec(&txn)
            .await?;
        insert_permissions(&txn, id, permissions).await?;
    }
    txn.commit().await?;
    find(db, id).await?.ok_or(RoleError::NotFound)
}

/// Deletes a role. Its assignments and permission rows go with it.
pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<(), RoleError> {
    let deleted = roles::Entity::delete_by_id(id).exec(db).await?;
    if deleted.rows_affected == 0 {
        return Err(RoleError::NotFound);
    }
    Ok(())
}

async fn insert_permissions(
    txn: &impl sea_orm::ConnectionTrait,
    role_id: i32,
    permissions: &BTreeSet<String>,
) -> Result<(), DbErr> {
    if permissions.is_empty() {
        return Ok(());
    }
    role_permissions::Entity::insert_many(permissions.iter().map(|permission| {
        role_permissions::ActiveModel {
            role_id: Set(role_id),
            permission: Set(permission.clone()),
        }
    }))
    .exec(txn)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{insert_user, temp_db};

    fn set(permissions: &[&str]) -> BTreeSet<String> {
        permissions.iter().map(|p| (*p).to_owned()).collect()
    }

    #[tokio::test]
    async fn create_update_and_delete_a_role() {
        let (db, _dir) = temp_db().await;
        let created = create(&db, "Member", Some("Family"), &set(&["library:read"]))
            .await
            .unwrap();
        assert_eq!(created.permissions, set(&["library:read"]));

        let changes = RoleChanges {
            name: Some("Viewer".into()),
            description: Some(String::new()),
            permissions: Some(set(&["queue:read", "library:read"])),
        };
        let updated = update(&db, created.role.id, changes).await.unwrap();
        assert_eq!(updated.role.name, "Viewer");
        assert_eq!(updated.role.description, None);
        assert_eq!(updated.permissions, set(&["library:read", "queue:read"]));

        // The Owner role comes first and stores no permissions.
        let roles = list(&db).await.unwrap();
        assert_eq!(
            roles.iter().map(|r| &*r.role.name).collect::<Vec<_>>(),
            ["Owner", "Viewer"]
        );
        assert!(roles[0].permissions.is_empty());

        delete(&db, created.role.id).await.unwrap();
        assert!(find(&db, created.role.id).await.unwrap().is_none());
        assert!(matches!(
            delete(&db, created.role.id).await,
            Err(RoleError::NotFound)
        ));
    }

    #[tokio::test]
    async fn a_duplicate_name_is_refused_and_nothing_is_stored() {
        let (db, _dir) = temp_db().await;
        create(&db, "Member", None, &set(&[])).await.unwrap();
        let again = create(&db, "Member", None, &set(&["library:read"])).await;
        assert!(matches!(again, Err(RoleError::NameTaken)));
        assert_eq!(list(&db).await.unwrap().len(), 2);

        let other = create(&db, "Other", None, &set(&["library:read"]))
            .await
            .unwrap();
        let changes = RoleChanges {
            name: Some("Member".into()),
            permissions: Some(set(&[])),
            ..Default::default()
        };
        assert!(matches!(
            update(&db, other.role.id, changes).await,
            Err(RoleError::NameTaken)
        ));
        // The failed update rolled back, permissions included.
        let kept = find(&db, other.role.id).await.unwrap().unwrap();
        assert_eq!(kept.permissions, set(&["library:read"]));
    }

    #[tokio::test]
    async fn counts_the_users_of_a_role() {
        let (db, _dir) = temp_db().await;
        insert_user(&db, "u1").await;
        let role = create(&db, "Member", None, &set(&[])).await.unwrap();
        user_roles::ActiveModel {
            user_id: Set("u1".into()),
            role_id: Set(role.role.id),
            all_libraries: Set(true),
        }
        .insert(&db)
        .await
        .unwrap();
        assert_eq!(find(&db, role.role.id).await.unwrap().unwrap().users, 1);
        assert!(matches!(
            update(&db, 999, RoleChanges::default()).await,
            Err(RoleError::NotFound)
        ));
    }
}
