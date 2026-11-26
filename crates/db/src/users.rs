//! Users beyond what the auth library stores: the `disabled` flag, roles and
//! direct grants (docs/15-permissions.md).

use std::collections::{BTreeMap, BTreeSet};

use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait,
    QueryFilter, QueryOrder, Set, SqlErr, Statement, TransactionTrait,
};

use crate::auth_schema::{account, session, user};
use crate::entities::{
    roles, user_permission_libraries, user_permissions, user_role_libraries, user_roles,
};

/// A role assignment with its library scope (`None` = all libraries).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleAssignment {
    pub role_id: i32,
    pub library_ids: Option<BTreeSet<i32>>,
}

/// A direct permission grant with its library scope (`None` = all libraries).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionGrant {
    pub permission: String,
    pub library_ids: Option<BTreeSet<i32>>,
}

/// A user with the roles and direct grants stored for them.
#[derive(Debug, Clone)]
pub struct UserDetail {
    pub user: user::Model,
    pub disabled: bool,
    /// Ordered by name.
    pub roles: Vec<roles::Model>,
    /// Per-role library scope (`None` = all libraries, `Some(ids)` = those
    /// libraries sorted).
    pub role_scopes: BTreeMap<i32, Option<Vec<i32>>>,
    /// The direct grants, not the effective set.
    pub permissions: BTreeSet<String>,
    /// Per-permission library scope (`None` = all libraries, `Some(ids)` =
    /// those libraries sorted).
    pub permission_scopes: BTreeMap<String, Option<Vec<i32>>>,
}

#[derive(Debug)]
pub enum UserError {
    NotFound,
    EmailTaken,
    Db(DbErr),
}

impl From<DbErr> for UserError {
    fn from(error: DbErr) -> Self {
        if matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) {
            Self::EmailTaken
        } else {
            Self::Db(error)
        }
    }
}

/// Ids of the disabled users. The auth policy loads them at startup.
pub async fn disabled_ids(db: &DatabaseConnection) -> Result<BTreeSet<String>, DbErr> {
    let rows = db
        .query_all_raw(Statement::from_string(
            db.get_database_backend(),
            r#"SELECT id FROM "user" WHERE disabled = 1"#,
        ))
        .await?;
    rows.iter().map(|row| row.try_get("", "id")).collect()
}

pub async fn set_disabled(
    db: &impl ConnectionTrait,
    id: &str,
    disabled: bool,
) -> Result<(), DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        db.get_database_backend(),
        r#"UPDATE "user" SET disabled = ? WHERE id = ?"#,
        [disabled.into(), id.into()],
    ))
    .await?;
    Ok(())
}

/// Every user, ordered by name.
pub async fn list(db: &DatabaseConnection) -> Result<Vec<UserDetail>, DbErr> {
    let users = user::Entity::find()
        .order_by_asc(user::Column::Name)
        .all(db)
        .await?;
    let disabled = disabled_ids(db).await?;
    let all_roles: BTreeMap<i32, roles::Model> = roles::Entity::find()
        .order_by_asc(roles::Column::Name)
        .all(db)
        .await?
        .into_iter()
        .map(|role| (role.id, role))
        .collect();
    let mut role_rows: BTreeMap<String, Vec<user_roles::Model>> = BTreeMap::new();
    for row in user_roles::Entity::find().all(db).await? {
        role_rows.entry(row.user_id.clone()).or_default().push(row);
    }
    let mut role_libs: BTreeMap<(String, i32), Vec<i32>> = BTreeMap::new();
    for row in user_role_libraries::Entity::find()
        .order_by_asc(user_role_libraries::Column::LibraryId)
        .all(db)
        .await?
    {
        role_libs
            .entry((row.user_id, row.role_id))
            .or_default()
            .push(row.library_id);
    }
    let mut grant_rows: BTreeMap<String, Vec<user_permissions::Model>> = BTreeMap::new();
    for row in user_permissions::Entity::find().all(db).await? {
        grant_rows.entry(row.user_id.clone()).or_default().push(row);
    }
    let mut grant_libs: BTreeMap<(String, String), Vec<i32>> = BTreeMap::new();
    for row in user_permission_libraries::Entity::find()
        .order_by_asc(user_permission_libraries::Column::LibraryId)
        .all(db)
        .await?
    {
        grant_libs
            .entry((row.user_id, row.permission))
            .or_default()
            .push(row.library_id);
    }
    Ok(users
        .into_iter()
        .map(|user| {
            let mut roles = Vec::new();
            let mut role_scopes = BTreeMap::new();
            for assignment in role_rows.remove(&user.id).unwrap_or_default() {
                if let Some(role) = all_roles.get(&assignment.role_id) {
                    let scope = if role.is_owner || assignment.all_libraries {
                        None
                    } else {
                        Some(
                            role_libs
                                .remove(&(user.id.clone(), assignment.role_id))
                                .unwrap_or_default(),
                        )
                    };
                    role_scopes.insert(role.id, scope);
                    roles.push(role.clone());
                }
            }
            roles.sort_by(|a, b| a.name.cmp(&b.name));

            let mut permissions = BTreeSet::new();
            let mut permission_scopes = BTreeMap::new();
            for grant in grant_rows.remove(&user.id).unwrap_or_default() {
                let scope = if grant.all_libraries {
                    None
                } else {
                    Some(
                        grant_libs
                            .remove(&(user.id.clone(), grant.permission.clone()))
                            .unwrap_or_default(),
                    )
                };
                permission_scopes.insert(grant.permission.clone(), scope);
                permissions.insert(grant.permission);
            }

            UserDetail {
                disabled: disabled.contains(&user.id),
                roles,
                role_scopes,
                permissions,
                permission_scopes,
                user,
            }
        })
        .collect())
}

pub async fn find(db: &DatabaseConnection, id: &str) -> Result<Option<UserDetail>, DbErr> {
    Ok(list(db)
        .await?
        .into_iter()
        .find(|detail| detail.user.id == id))
}

/// Enabled users who hold the Owner role.
pub async fn enabled_owners(db: &DatabaseConnection) -> Result<Vec<String>, DbErr> {
    Ok(list(db)
        .await?
        .into_iter()
        .filter(|detail| !detail.disabled && detail.roles.iter().any(|role| role.is_owner))
        .map(|detail| detail.user.id)
        .collect())
}

/// Changes name and email. Fails with `EmailTaken` on a duplicate email.
pub async fn update_profile(
    db: &DatabaseConnection,
    id: &str,
    name: Option<String>,
    email: Option<String>,
) -> Result<(), UserError> {
    let mut model: user::ActiveModel = user::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or(UserError::NotFound)?
        .into();
    if let Some(name) = name {
        model.name = Set(name);
    }
    if let Some(email) = email {
        model.email = Set(Some(email));
    }
    model.updated_at = Set(sea_orm::prelude::ChronoUtc::now());
    model.update(db).await?;
    Ok(())
}

/// Replaces the roles of a user, each scoped to all libraries.
pub async fn set_roles(
    db: &DatabaseConnection,
    id: &str,
    role_ids: &BTreeSet<i32>,
) -> Result<(), DbErr> {
    let assignments: Vec<RoleAssignment> = role_ids
        .iter()
        .map(|&role_id| RoleAssignment {
            role_id,
            library_ids: None,
        })
        .collect();
    set_role_assignments(db, id, &assignments).await
}

/// Replaces the roles of a user with their per-library scopes.
pub async fn set_role_assignments(
    db: &DatabaseConnection,
    id: &str,
    assignments: &[RoleAssignment],
) -> Result<(), DbErr> {
    let txn = db.begin().await?;
    user_roles::Entity::delete_many()
        .filter(user_roles::Column::UserId.eq(id))
        .exec(&txn)
        .await?;
    if !assignments.is_empty() {
        user_roles::Entity::insert_many(assignments.iter().map(|entry| user_roles::ActiveModel {
            user_id: Set(id.to_owned()),
            role_id: Set(entry.role_id),
            all_libraries: Set(entry.library_ids.is_none()),
        }))
        .exec(&txn)
        .await?;
        let mut library_rows = Vec::new();
        for entry in assignments {
            if let Some(libs) = &entry.library_ids {
                for &library_id in libs {
                    library_rows.push(user_role_libraries::ActiveModel {
                        user_id: Set(id.to_owned()),
                        role_id: Set(entry.role_id),
                        library_id: Set(library_id),
                    });
                }
            }
        }
        if !library_rows.is_empty() {
            user_role_libraries::Entity::insert_many(library_rows)
                .exec(&txn)
                .await?;
        }
    }
    txn.commit().await
}

/// The hash of the password of the credential account, `None` when the user
/// has no password.
pub async fn password_hash(db: &DatabaseConnection, id: &str) -> Result<Option<String>, DbErr> {
    Ok(account::Entity::find()
        .filter(account::Column::UserId.eq(id))
        .filter(account::Column::ProviderId.eq("credential"))
        .one(db)
        .await?
        .and_then(|account| account.password))
}

/// Gives the Owner role to `to` and takes it from `from`, which keeps
/// `keep_role` instead if one is given. One transaction: there is never a
/// moment without an Owner or with an unintended second one.
pub async fn transfer_owner(
    db: &DatabaseConnection,
    from: &str,
    to: &str,
    keep_role: Option<i32>,
) -> Result<(), DbErr> {
    let txn = db.begin().await?;
    let owner_role = roles::Entity::find()
        .filter(roles::Column::IsOwner.eq(true))
        .one(&txn)
        .await?
        .ok_or_else(|| DbErr::Custom("the Owner role is missing".into()))?;
    user_roles::Entity::delete_by_id((from.to_owned(), owner_role.id))
        .exec(&txn)
        .await?;
    if user_roles::Entity::find_by_id((to.to_owned(), owner_role.id))
        .one(&txn)
        .await?
        .is_none()
    {
        user_roles::ActiveModel {
            user_id: Set(to.to_owned()),
            role_id: Set(owner_role.id),
            all_libraries: Set(true),
        }
        .insert(&txn)
        .await?;
    }
    if let Some(role_id) = keep_role
        && user_roles::Entity::find_by_id((from.to_owned(), role_id))
            .one(&txn)
            .await?
            .is_none()
    {
        user_roles::ActiveModel {
            user_id: Set(from.to_owned()),
            role_id: Set(role_id),
            all_libraries: Set(true),
        }
        .insert(&txn)
        .await?;
    }
    txn.commit().await
}

/// Replaces the direct grants of a user, each scoped to all libraries.
pub async fn set_permissions(
    db: &DatabaseConnection,
    id: &str,
    permissions: &BTreeSet<String>,
) -> Result<(), DbErr> {
    let grants: Vec<PermissionGrant> = permissions
        .iter()
        .map(|permission| PermissionGrant {
            permission: permission.clone(),
            library_ids: None,
        })
        .collect();
    set_scoped_permissions(db, id, &grants).await
}

/// Replaces the direct grants of a user with their per-library scopes.
pub async fn set_scoped_permissions(
    db: &DatabaseConnection,
    id: &str,
    grants: &[PermissionGrant],
) -> Result<(), DbErr> {
    let txn = db.begin().await?;
    user_permissions::Entity::delete_many()
        .filter(user_permissions::Column::UserId.eq(id))
        .exec(&txn)
        .await?;
    if !grants.is_empty() {
        user_permissions::Entity::insert_many(grants.iter().map(|entry| {
            user_permissions::ActiveModel {
                user_id: Set(id.to_owned()),
                permission: Set(entry.permission.clone()),
                all_libraries: Set(entry.library_ids.is_none()),
            }
        }))
        .exec(&txn)
        .await?;
        let mut library_rows = Vec::new();
        for entry in grants {
            if let Some(libs) = &entry.library_ids {
                for &library_id in libs {
                    library_rows.push(user_permission_libraries::ActiveModel {
                        user_id: Set(id.to_owned()),
                        permission: Set(entry.permission.clone()),
                        library_id: Set(library_id),
                    });
                }
            }
        }
        if !library_rows.is_empty() {
            user_permission_libraries::Entity::insert_many(library_rows)
                .exec(&txn)
                .await?;
        }
    }
    txn.commit().await
}

/// Stores a new password hash on the credential account. Returns false when
/// the user has none.
pub async fn set_password(db: &DatabaseConnection, id: &str, hash: &str) -> Result<bool, DbErr> {
    let updated = account::Entity::update_many()
        .col_expr(account::Column::Password, hash.into())
        .col_expr(
            account::Column::UpdatedAt,
            sea_orm::prelude::ChronoUtc::now().into(),
        )
        .filter(account::Column::UserId.eq(id))
        .filter(account::Column::ProviderId.eq("credential"))
        .exec(db)
        .await?;
    Ok(updated.rows_affected > 0)
}

/// Signs the user out everywhere.
pub async fn delete_sessions(db: &DatabaseConnection, id: &str) -> Result<(), DbErr> {
    session::Entity::delete_many()
        .filter(session::Column::UserId.eq(id))
        .exec(db)
        .await?;
    Ok(())
}

/// Deletes the user. Sessions, accounts, roles and grants go with it.
pub async fn delete(db: &DatabaseConnection, id: &str) -> Result<bool, DbErr> {
    Ok(user::Entity::delete_by_id(id).exec(db).await?.rows_affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::create;
    use crate::testing::{insert_user, temp_db};

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[tokio::test]
    async fn roles_grants_and_the_disabled_flag_round_trip() {
        let (db, _dir) = temp_db().await;
        insert_user(&db, "u1").await;
        insert_user(&db, "u2").await;
        let member = create(&db, "Member", None, &set(&[]))
            .await
            .unwrap()
            .role
            .id;
        let viewer = create(&db, "Viewer", None, &set(&[]))
            .await
            .unwrap()
            .role
            .id;

        set_roles(&db, "u1", &[member, viewer].into())
            .await
            .unwrap();
        set_permissions(&db, "u1", &set(&["queue:manage"]))
            .await
            .unwrap();
        set_disabled(&db, "u1", true).await.unwrap();

        let found = find(&db, "u1").await.unwrap().unwrap();
        assert!(found.disabled);
        assert_eq!(found.permissions, set(&["queue:manage"]));
        assert_eq!(
            found.roles.iter().map(|r| &*r.name).collect::<Vec<_>>(),
            ["Member", "Viewer"]
        );
        assert_eq!(disabled_ids(&db).await.unwrap(), set(&["u1"]));

        // Replacing, not adding.
        set_roles(&db, "u1", &[viewer].into()).await.unwrap();
        set_permissions(&db, "u1", &set(&[])).await.unwrap();
        set_disabled(&db, "u1", false).await.unwrap();
        let found = find(&db, "u1").await.unwrap().unwrap();
        assert_eq!(found.roles.len(), 1);
        assert!(found.permissions.is_empty() && !found.disabled);
        assert!(find(&db, "u2").await.unwrap().unwrap().roles.is_empty());
    }

    #[tokio::test]
    async fn owners_count_only_when_enabled() {
        let (db, _dir) = temp_db().await;
        insert_user(&db, "u1").await;
        let owner = crate::roles::list(&db).await.unwrap()[0].role.id;
        set_roles(&db, "u1", &[owner].into()).await.unwrap();
        assert_eq!(enabled_owners(&db).await.unwrap(), ["u1"]);
        set_disabled(&db, "u1", true).await.unwrap();
        assert!(enabled_owners(&db).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn profile_email_must_stay_unique_and_delete_cascades() {
        let (db, _dir) = temp_db().await;
        insert_user(&db, "u1").await;
        insert_user(&db, "u2").await;
        let again = update_profile(&db, "u2", None, Some("u1@example.com".into())).await;
        assert!(matches!(again, Err(UserError::EmailTaken)));
        update_profile(
            &db,
            "u2",
            Some("Two".into()),
            Some("two@example.com".into()),
        )
        .await
        .unwrap();
        let found = find(&db, "u2").await.unwrap().unwrap();
        assert_eq!(found.user.name, "Two");
        assert!(matches!(
            update_profile(&db, "nope", None, None).await,
            Err(UserError::NotFound)
        ));

        set_permissions(&db, "u2", &set(&["queue:read"]))
            .await
            .unwrap();
        assert!(delete(&db, "u2").await.unwrap());
        assert!(!delete(&db, "u2").await.unwrap());
        assert!(
            user_permissions::Entity::find()
                .all(&db)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
