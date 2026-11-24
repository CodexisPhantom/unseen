//! Users beyond what the auth library stores: the `disabled` flag, roles and
//! direct grants (docs/15-permissions.md).

use std::collections::{BTreeMap, BTreeSet};

use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait,
    QueryFilter, QueryOrder, Set, SqlErr, Statement, TransactionTrait,
};

use crate::auth_schema::{account, session, user};
use crate::entities::{roles, user_permissions, user_roles};

/// A user with the roles and direct grants stored for them.
#[derive(Debug, Clone)]
pub struct UserDetail {
    pub user: user::Model,
    pub disabled: bool,
    /// Ordered by name.
    pub roles: Vec<roles::Model>,
    /// The direct grants, not the effective set.
    pub permissions: BTreeSet<String>,
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
    let mut role_ids: BTreeMap<String, Vec<i32>> = BTreeMap::new();
    for row in user_roles::Entity::find().all(db).await? {
        role_ids.entry(row.user_id).or_default().push(row.role_id);
    }
    let mut grants: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for row in user_permissions::Entity::find().all(db).await? {
        grants
            .entry(row.user_id)
            .or_default()
            .insert(row.permission);
    }
    Ok(users
        .into_iter()
        .map(|user| {
            let mut roles: Vec<_> = role_ids
                .remove(&user.id)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|id| all_roles.get(&id).cloned())
                .collect();
            roles.sort_by(|a, b| a.name.cmp(&b.name));
            UserDetail {
                disabled: disabled.contains(&user.id),
                permissions: grants.remove(&user.id).unwrap_or_default(),
                roles,
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

/// Replaces the roles of a user.
pub async fn set_roles(
    db: &DatabaseConnection,
    id: &str,
    role_ids: &BTreeSet<i32>,
) -> Result<(), DbErr> {
    let txn = db.begin().await?;
    user_roles::Entity::delete_many()
        .filter(user_roles::Column::UserId.eq(id))
        .exec(&txn)
        .await?;
    if !role_ids.is_empty() {
        user_roles::Entity::insert_many(role_ids.iter().map(|role_id| user_roles::ActiveModel {
            user_id: Set(id.to_owned()),
            role_id: Set(*role_id),
        }))
        .exec(&txn)
        .await?;
    }
    txn.commit().await
}

/// Replaces the direct grants of a user.
pub async fn set_permissions(
    db: &DatabaseConnection,
    id: &str,
    permissions: &BTreeSet<String>,
) -> Result<(), DbErr> {
    let txn = db.begin().await?;
    user_permissions::Entity::delete_many()
        .filter(user_permissions::Column::UserId.eq(id))
        .exec(&txn)
        .await?;
    if !permissions.is_empty() {
        user_permissions::Entity::insert_many(permissions.iter().map(|permission| {
            user_permissions::ActiveModel {
                user_id: Set(id.to_owned()),
                permission: Set(permission.clone()),
            }
        }))
        .exec(&txn)
        .await?;
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
