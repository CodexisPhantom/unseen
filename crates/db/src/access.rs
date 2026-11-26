//! Who may do what: the roles and grants of a user (docs/15-permissions.md).

use std::collections::{BTreeMap, BTreeSet};

use sea_orm::{ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder};
use unseen_core::permission::LibraryScope;

use crate::entities::{
    role_permissions, roles, user_permission_libraries, user_permissions, user_role_libraries,
    user_roles,
};

/// A role assigned to a user together with its library scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignedRole {
    pub role: roles::Model,
    /// `None` means all libraries; `Some(ids)` restricts the role to those
    /// libraries (sorted).
    pub library_ids: Option<Vec<i32>>,
}

/// What the database grants to one user. Permissions are the stored strings;
/// `core` turns them into the catalog and ignores the ones it does not know.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Grants {
    /// The user holds the Owner role, which means every permission across all
    /// libraries.
    pub is_owner: bool,
    /// Union of the permissions of all roles and of the direct grants.
    pub permissions: BTreeSet<String>,
    /// Scoped permission grants from all roles and direct grants.
    pub scoped_permissions: Vec<(String, LibraryScope)>,
}

/// The roles of a user with their library scopes, ordered by role name.
pub async fn assigned_roles_of_user(
    db: &DatabaseConnection,
    user_id: &str,
) -> Result<Vec<AssignedRole>, DbErr> {
    let assignments = user_roles::Entity::find()
        .filter(user_roles::Column::UserId.eq(user_id))
        .all(db)
        .await?;
    if assignments.is_empty() {
        return Ok(Vec::new());
    }
    let role_ids: Vec<i32> = assignments.iter().map(|row| row.role_id).collect();
    let roles_by_id: BTreeMap<i32, roles::Model> = roles::Entity::find()
        .filter(roles::Column::Id.is_in(role_ids))
        .all(db)
        .await?
        .into_iter()
        .map(|role| (role.id, role))
        .collect();
    let mut libs_by_role: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
    for row in user_role_libraries::Entity::find()
        .filter(user_role_libraries::Column::UserId.eq(user_id))
        .order_by_asc(user_role_libraries::Column::LibraryId)
        .all(db)
        .await?
    {
        libs_by_role
            .entry(row.role_id)
            .or_default()
            .push(row.library_id);
    }
    let mut assigned: Vec<AssignedRole> = assignments
        .into_iter()
        .filter_map(|assignment| {
            let role = roles_by_id.get(&assignment.role_id)?.clone();
            let library_ids = if role.is_owner || assignment.all_libraries {
                None
            } else {
                Some(libs_by_role.remove(&assignment.role_id).unwrap_or_default())
            };
            Some(AssignedRole { role, library_ids })
        })
        .collect();
    assigned.sort_by(|a, b| a.role.name.cmp(&b.role.name));
    Ok(assigned)
}

/// The roles of a user, ordered by name.
pub async fn roles_of_user(
    db: &DatabaseConnection,
    user_id: &str,
) -> Result<Vec<roles::Model>, DbErr> {
    Ok(assigned_roles_of_user(db, user_id)
        .await?
        .into_iter()
        .map(|assigned| assigned.role)
        .collect())
}

/// The ids of the users holding `role_id`, ordered by user id.
pub async fn users_of_role(db: &DatabaseConnection, role_id: i32) -> Result<Vec<String>, DbErr> {
    Ok(user_roles::Entity::find()
        .filter(user_roles::Column::RoleId.eq(role_id))
        .order_by_asc(user_roles::Column::UserId)
        .all(db)
        .await?
        .into_iter()
        .map(|row| row.user_id)
        .collect())
}

/// The effective grants of a user: all roles plus the direct grants, each with
/// its library scope.
pub async fn grants_of_user(db: &DatabaseConnection, user_id: &str) -> Result<Grants, DbErr> {
    let assigned = assigned_roles_of_user(db, user_id).await?;
    let is_owner = assigned.iter().any(|entry| entry.role.is_owner);
    let role_scopes: BTreeMap<i32, LibraryScope> = assigned
        .iter()
        .map(|entry| {
            let scope = match &entry.library_ids {
                None => LibraryScope::All,
                Some(ids) => LibraryScope::Libraries(ids.iter().copied().collect()),
            };
            (entry.role.id, scope)
        })
        .collect();

    let mut scoped_permissions = Vec::new();
    let mut permissions = BTreeSet::new();

    if !role_scopes.is_empty() {
        for row in role_permissions::Entity::find()
            .filter(role_permissions::Column::RoleId.is_in(role_scopes.keys().copied()))
            .all(db)
            .await?
        {
            if let Some(scope) = role_scopes.get(&row.role_id) {
                permissions.insert(row.permission.clone());
                scoped_permissions.push((row.permission, scope.clone()));
            }
        }
    }

    let direct_rows = user_permissions::Entity::find()
        .filter(user_permissions::Column::UserId.eq(user_id))
        .all(db)
        .await?;
    let mut direct_libs: BTreeMap<String, BTreeSet<i32>> = BTreeMap::new();
    if direct_rows.iter().any(|row| !row.all_libraries) {
        for row in user_permission_libraries::Entity::find()
            .filter(user_permission_libraries::Column::UserId.eq(user_id))
            .all(db)
            .await?
        {
            direct_libs
                .entry(row.permission)
                .or_default()
                .insert(row.library_id);
        }
    }
    for row in direct_rows {
        let scope = if row.all_libraries {
            LibraryScope::All
        } else {
            LibraryScope::Libraries(direct_libs.remove(&row.permission).unwrap_or_default())
        };
        permissions.insert(row.permission.clone());
        scoped_permissions.push((row.permission, scope));
    }

    Ok(Grants {
        is_owner,
        permissions,
        scoped_permissions,
    })
}

#[cfg(test)]
mod tests {
    use sea_orm::{ActiveModelTrait, Set};

    use super::*;
    use crate::testing::{insert_user, temp_db};

    async fn insert_role(db: &DatabaseConnection, name: &str, permissions: &[&str]) -> i32 {
        let role = roles::ActiveModel {
            name: Set(name.into()),
            is_owner: Set(false),
            created_at: Set(crate::now()),
            updated_at: Set(crate::now()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        for permission in permissions {
            role_permissions::ActiveModel {
                role_id: Set(role.id),
                permission: Set((*permission).into()),
            }
            .insert(db)
            .await
            .unwrap();
        }
        role.id
    }

    async fn assign(db: &DatabaseConnection, user_id: &str, role_id: i32) {
        user_roles::ActiveModel {
            user_id: Set(user_id.into()),
            role_id: Set(role_id),
            all_libraries: Set(true),
        }
        .insert(db)
        .await
        .unwrap();
    }

    fn set(permissions: &[&str]) -> BTreeSet<String> {
        permissions.iter().map(|p| (*p).to_owned()).collect()
    }

    #[tokio::test]
    async fn two_roles_and_a_direct_grant_are_united() {
        let (db, _dir) = temp_db().await;
        insert_user(&db, "u1").await;
        insert_user(&db, "u2").await;
        let adder = insert_role(&db, "Adder", &["library:read", "library:add"]).await;
        let searcher = insert_role(&db, "Searcher", &["library:read", "search:run"]).await;
        assign(&db, "u1", adder).await;
        assign(&db, "u1", searcher).await;
        user_permissions::ActiveModel {
            user_id: Set("u1".into()),
            permission: Set("queue:read".into()),
            all_libraries: Set(true),
        }
        .insert(&db)
        .await
        .unwrap();

        let grants = grants_of_user(&db, "u1").await.unwrap();
        assert!(!grants.is_owner);
        assert_eq!(
            grants.permissions,
            set(&["library:add", "library:read", "queue:read", "search:run"])
        );
        let names: Vec<_> = roles_of_user(&db, "u1").await.unwrap();
        assert_eq!(
            names.iter().map(|r| &*r.name).collect::<Vec<_>>(),
            ["Adder", "Searcher"]
        );

        // Another user gets nothing from u1's roles and grants.
        assert_eq!(grants_of_user(&db, "u2").await.unwrap(), Grants::default());
    }

    #[tokio::test]
    async fn the_migration_creates_one_owner_role_and_refuses_a_second() {
        let (db, _dir) = temp_db().await;
        let owners = roles::Entity::find()
            .filter(roles::Column::IsOwner.eq(true))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(owners.len(), 1);
        assert_eq!(owners[0].name, "Owner");

        insert_user(&db, "u1").await;
        assign(&db, "u1", owners[0].id).await;
        let grants = grants_of_user(&db, "u1").await.unwrap();
        assert!(grants.is_owner);
        assert!(
            grants.permissions.is_empty(),
            "the Owner role stores no rows"
        );

        let second = roles::ActiveModel {
            name: Set("Second owner".into()),
            is_owner: Set(true),
            created_at: Set(crate::now()),
            updated_at: Set(crate::now()),
            ..Default::default()
        }
        .insert(&db)
        .await;
        assert!(second.is_err(), "a second Owner role must be refused");
    }

    #[tokio::test]
    async fn users_of_role_lists_every_holder() {
        let (db, _dir) = temp_db().await;
        insert_user(&db, "u1").await;
        insert_user(&db, "u2").await;
        insert_user(&db, "u3").await;
        let role = insert_role(&db, "Adder", &["library:add"]).await;
        assign(&db, "u2", role).await;
        assign(&db, "u1", role).await;

        assert_eq!(
            users_of_role(&db, role).await.unwrap(),
            ["u1".to_owned(), "u2".to_owned()]
        );
        assert!(users_of_role(&db, role + 1).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn deleting_a_role_removes_its_assignments() {
        let (db, _dir) = temp_db().await;
        insert_user(&db, "u1").await;
        let role = insert_role(&db, "Adder", &["library:add"]).await;
        assign(&db, "u1", role).await;
        roles::Entity::delete_by_id(role).exec(&db).await.unwrap();
        assert_eq!(grants_of_user(&db, "u1").await.unwrap(), Grants::default());
    }
}
