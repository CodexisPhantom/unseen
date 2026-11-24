//! API keys: the hash, the chosen permissions and the lookup that
//! authenticates a request (docs/15-permissions.md, "API keys").

use std::collections::{BTreeMap, BTreeSet};

use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbErr, QueryResult, Statement, TransactionTrait,
};

/// A key that is not revoked. The key itself is never stored, only its hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKey {
    pub id: i32,
    pub user_id: String,
    pub user_name: String,
    pub name: String,
    /// The first characters of the key, for display.
    pub prefix: String,
    /// The permissions chosen at creation. The effective set is smaller when
    /// the user holds fewer today.
    pub permissions: BTreeSet<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

/// What a valid key stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyOwner {
    pub key_id: i32,
    pub user_id: String,
    pub permissions: BTreeSet<String>,
}

const SELECT: &str = r#"SELECT k.id, k.user_id, u.name AS user_name, k.name, k.key_prefix,
       k.created_at, k.last_used_at
FROM api_keys k JOIN "user" u ON u.id = k.user_id
WHERE k.revoked_at IS NULL"#;

fn statement(db: &impl ConnectionTrait, sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(db.get_database_backend(), sql, values)
}

fn key_from(row: &QueryResult) -> Result<ApiKey, DbErr> {
    Ok(ApiKey {
        id: row.try_get("", "id")?,
        user_id: row.try_get("", "user_id")?,
        user_name: row.try_get("", "user_name")?,
        name: row.try_get("", "name")?,
        prefix: row.try_get("", "key_prefix")?,
        permissions: BTreeSet::new(),
        created_at: row.try_get("", "created_at")?,
        last_used_at: row.try_get("", "last_used_at")?,
    })
}

async fn with_permissions(
    db: &DatabaseConnection,
    mut keys: Vec<ApiKey>,
) -> Result<Vec<ApiKey>, DbErr> {
    let mut chosen: BTreeMap<i32, BTreeSet<String>> = BTreeMap::new();
    for row in db
        .query_all_raw(statement(
            db,
            "SELECT api_key_id, permission FROM api_key_permissions",
            vec![],
        ))
        .await?
    {
        chosen
            .entry(row.try_get("", "api_key_id")?)
            .or_default()
            .insert(row.try_get("", "permission")?);
    }
    for key in &mut keys {
        key.permissions = chosen.remove(&key.id).unwrap_or_default();
    }
    Ok(keys)
}

/// Stores a key by its hash. `hash` must be unique.
pub async fn create(
    db: &DatabaseConnection,
    user_id: &str,
    name: &str,
    hash: &str,
    prefix: &str,
    permissions: &BTreeSet<String>,
) -> Result<ApiKey, DbErr> {
    let txn = db.begin().await?;
    txn.execute_raw(statement(
        &txn,
        "INSERT INTO api_keys (user_id, name, key_hash, key_prefix, created_at) VALUES (?, ?, ?, ?, ?)",
        vec![
            user_id.into(),
            name.into(),
            hash.into(),
            prefix.into(),
            crate::now().into(),
        ],
    ))
    .await?;
    let id: i32 = txn
        .query_one_raw(statement(&txn, "SELECT last_insert_rowid() AS id", vec![]))
        .await?
        .ok_or_else(|| DbErr::RecordNotFound("api key".into()))?
        .try_get("", "id")?;
    for permission in permissions {
        txn.execute_raw(statement(
            &txn,
            "INSERT INTO api_key_permissions (api_key_id, permission) VALUES (?, ?)",
            vec![id.into(), permission.as_str().into()],
        ))
        .await?;
    }
    txn.commit().await?;
    find(db, id)
        .await?
        .ok_or_else(|| DbErr::RecordNotFound("api key".into()))
}

/// The keys that are not revoked, oldest first. `user_id` limits them to one
/// user.
pub async fn list(db: &DatabaseConnection, user_id: Option<&str>) -> Result<Vec<ApiKey>, DbErr> {
    let (sql, values) = match user_id {
        Some(user_id) => (
            format!("{SELECT} AND k.user_id = ? ORDER BY k.id"),
            vec![user_id.into()],
        ),
        None => (format!("{SELECT} ORDER BY k.id"), vec![]),
    };
    let keys = db
        .query_all_raw(statement(db, &sql, values))
        .await?
        .iter()
        .map(key_from)
        .collect::<Result<_, _>>()?;
    with_permissions(db, keys).await
}

pub async fn find(db: &DatabaseConnection, id: i32) -> Result<Option<ApiKey>, DbErr> {
    Ok(list(db, None).await?.into_iter().find(|key| key.id == id))
}

/// Revokes a key. Returns false when there is no such key or it was revoked.
pub async fn revoke(db: &DatabaseConnection, id: i32) -> Result<bool, DbErr> {
    let done = db
        .execute_raw(statement(
            db,
            "UPDATE api_keys SET revoked_at = ? WHERE id = ? AND revoked_at IS NULL",
            vec![crate::now().into(), id.into()],
        ))
        .await?;
    Ok(done.rows_affected() > 0)
}

/// Resolves a key by its hash. A revoked key, and a key of a disabled user,
/// resolve to nothing. The last use is stored at most once a minute, so a
/// busy client does not write on every request.
pub async fn authenticate(db: &DatabaseConnection, hash: &str) -> Result<Option<KeyOwner>, DbErr> {
    let Some(row) = db
        .query_one_raw(statement(
            db,
            r#"SELECT k.id, k.user_id FROM api_keys k JOIN "user" u ON u.id = k.user_id
               WHERE k.key_hash = ? AND k.revoked_at IS NULL AND u.disabled = 0"#,
            vec![hash.into()],
        ))
        .await?
    else {
        return Ok(None);
    };
    let key_id: i32 = row.try_get("", "id")?;
    db.execute_raw(statement(
        db,
        "UPDATE api_keys SET last_used_at = ? WHERE id = ?
           AND (last_used_at IS NULL OR last_used_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-1 minute'))",
        vec![crate::now().into(), key_id.into()],
    ))
    .await?;
    let permissions = db
        .query_all_raw(statement(
            db,
            "SELECT permission FROM api_key_permissions WHERE api_key_id = ?",
            vec![key_id.into()],
        ))
        .await?
        .iter()
        .map(|row| row.try_get("", "permission"))
        .collect::<Result<_, _>>()?;
    Ok(Some(KeyOwner {
        key_id,
        user_id: row.try_get("", "user_id")?,
        permissions,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{insert_user, temp_db};

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[tokio::test]
    async fn a_key_is_found_by_its_hash_until_it_is_revoked_or_its_user_is_disabled() {
        let (db, _dir) = temp_db().await;
        insert_user(&db, "u1").await;
        let key = create(
            &db,
            "u1",
            "Seerr",
            "h1",
            "abcd1234",
            &set(&["library:read"]),
        )
        .await
        .unwrap();
        assert_eq!(key.user_name, "u1");
        assert_eq!(key.permissions, set(&["library:read"]));
        assert_eq!(key.last_used_at, None);

        let owner = authenticate(&db, "h1").await.unwrap().unwrap();
        assert_eq!(owner.user_id, "u1");
        assert_eq!(owner.permissions, set(&["library:read"]));
        assert!(authenticate(&db, "other").await.unwrap().is_none());
        assert!(
            find(&db, key.id)
                .await
                .unwrap()
                .unwrap()
                .last_used_at
                .is_some()
        );

        crate::users::set_disabled(&db, "u1", true).await.unwrap();
        assert!(authenticate(&db, "h1").await.unwrap().is_none());
        crate::users::set_disabled(&db, "u1", false).await.unwrap();

        assert!(revoke(&db, key.id).await.unwrap());
        assert!(!revoke(&db, key.id).await.unwrap());
        assert!(authenticate(&db, "h1").await.unwrap().is_none());
        assert!(list(&db, None).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn last_use_is_not_written_twice_within_a_minute() {
        let (db, _dir) = temp_db().await;
        insert_user(&db, "u1").await;
        let key = create(&db, "u1", "Seerr", "h1", "abcd1234", &set(&[]))
            .await
            .unwrap();
        authenticate(&db, "h1").await.unwrap();
        let first = find(&db, key.id).await.unwrap().unwrap().last_used_at;
        authenticate(&db, "h1").await.unwrap();
        let second = find(&db, key.id).await.unwrap().unwrap().last_used_at;
        assert!(first.is_some());
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn keys_are_listed_per_user_and_deleted_with_the_user() {
        let (db, _dir) = temp_db().await;
        insert_user(&db, "u1").await;
        insert_user(&db, "u2").await;
        create(&db, "u1", "A", "h1", "p1", &set(&["queue:read"]))
            .await
            .unwrap();
        create(&db, "u2", "B", "h2", "p2", &set(&[])).await.unwrap();
        assert_eq!(list(&db, None).await.unwrap().len(), 2);
        let own = list(&db, Some("u1")).await.unwrap();
        assert_eq!(own.len(), 1);
        assert_eq!(own[0].name, "A");

        assert!(
            create(&db, "u1", "Dup", "h1", "p3", &set(&[]))
                .await
                .is_err()
        );

        crate::users::delete(&db, "u1").await.unwrap();
        assert!(authenticate(&db, "h1").await.unwrap().is_none());
        assert_eq!(list(&db, None).await.unwrap().len(), 1);
    }
}
