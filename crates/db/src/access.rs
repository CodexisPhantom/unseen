//! Who may do what: the roles and grants of a user (docs/15-permissions.md).

use std::collections::BTreeSet;

use sea_orm::{ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder};

use crate::entities::{role_permissions, roles, user_permissions, user_roles};

/// What the database grants to one user. Permissions are the stored strings;
/// `core` turns them into the catalog and ignores the ones it does not know.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Grants {
    /// The user holds the Owner role, which means every permission.
    pub is_owner: bool,
    /// Union of the permissions of all roles and of the direct grants.
    pub permissions: BTreeSet<String>,
}

/// The roles of a user, ordered by name.
pub async fn roles_of_user(
    db: &DatabaseConnection,
    user_id: &str,
) -> Result<Vec<roles::Model>, DbErr> {
    let role_ids: Vec<i32> = user_roles::Entity::find()
        .filter(user_roles::Column::UserId.eq(user_id))
        .all(db)
        .await?
        .into_iter()
        .map(|row| row.role_id)
        .collect();
    roles::Entity::find()
        .filter(roles::Column::Id.is_in(role_ids))
        .order_by_asc(roles::Column::Name)
        .all(db)
        .await
}

/// The effective grants of a user: all roles plus the direct grants.
pub async fn grants_of_user(db: &DatabaseConnection, user_id: &str) -> Result<Grants, DbErr> {
    let roles = roles_of_user(db, user_id).await?;
    let from_roles = role_permissions::Entity::find()
        .filter(role_permissions::Column::RoleId.is_in(roles.iter().map(|role| role.id)))
        .all(db)
        .await?
        .into_iter()
        .map(|row| row.permission);
    let direct = user_permissions::Entity::find()
        .filter(user_permissions::Column::UserId.eq(user_id))
        .all(db)
        .await?
        .into_iter()
        .map(|row| row.permission);
    Ok(Grants {
        is_owner: roles.iter().any(|role| role.is_owner),
        permissions: from_roles.chain(direct).collect(),
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
    async fn deleting_a_role_removes_its_assignments() {
        let (db, _dir) = temp_db().await;
        insert_user(&db, "u1").await;
        let role = insert_role(&db, "Adder", &["library:add"]).await;
        assign(&db, "u1", role).await;
        roles::Entity::delete_by_id(role).exec(&db).await.unwrap();
        assert_eq!(grants_of_user(&db, "u1").await.unwrap(), Grants::default());
    }
}
