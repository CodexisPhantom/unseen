//! The `settings` table (docs/10-configuration.md). The value column is JSON.
//! A secret key holds the encrypted form as a JSON string.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr, Statement};

use crate::secret::{self, Secret, SecretKey};

/// Setting keys that hold a secret. [`set_secret`] and [`get_secret`] refuse
/// every other key.
pub const SECRET_KEYS: &[&str] = &["tmdb.api_key", "proxy.url"];

/// What the table holds for a secret key.
#[derive(Debug, PartialEq, Eq)]
pub enum SecretSetting {
    Set(Secret),
    /// Stored, but the key does not match: the user must enter it again.
    MustReenter,
}

fn check(name: &str) -> Result<(), DbErr> {
    if SECRET_KEYS.contains(&name) {
        Ok(())
    } else {
        Err(DbErr::Custom(format!("{name} is not a secret setting")))
    }
}

async fn upsert(
    db: &DatabaseConnection,
    name: &str,
    value: serde_json::Value,
) -> Result<(), DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        db.get_database_backend(),
        "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, ?) \
         ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        [name.into(), value.into(), crate::now().into()],
    ))
    .await?;
    Ok(())
}

async fn fetch(db: &DatabaseConnection, name: &str) -> Result<Option<serde_json::Value>, DbErr> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            db.get_database_backend(),
            "SELECT value FROM settings WHERE key = ?",
            [name.into()],
        ))
        .await?;
    row.map(|row| row.try_get("", "value")).transpose()
}

/// Encrypts and stores a secret, replacing the previous value.
pub async fn set_secret(
    db: &DatabaseConnection,
    key: &SecretKey,
    name: &str,
    value: &Secret,
) -> Result<(), DbErr> {
    check(name)?;
    upsert(
        db,
        name,
        serde_json::Value::String(secret::encrypt(key, value)),
    )
    .await
}

/// Reads and decrypts a secret. `None` when the key has no row.
pub async fn get_secret(
    db: &DatabaseConnection,
    key: &SecretKey,
    name: &str,
) -> Result<Option<SecretSetting>, DbErr> {
    check(name)?;
    let Some(stored) = fetch(db, name).await? else {
        return Ok(None);
    };
    Ok(Some(
        match stored.as_str().map(|stored| secret::decrypt(key, stored)) {
            Some(Ok(secret)) => SecretSetting::Set(secret),
            _ => SecretSetting::MustReenter,
        },
    ))
}

/// Reads a plain text setting. `None` when the key has no row. A secret key
/// is refused: [`get_secret`] reads those.
pub async fn get_string(db: &DatabaseConnection, name: &str) -> Result<Option<String>, DbErr> {
    refuse_secret(name)?;
    Ok(fetch(db, name)
        .await?
        .and_then(|value| value.as_str().map(str::to_owned)))
}

/// Stores a plain text setting, replacing the previous value. A secret key
/// is refused: [`set_secret`] writes those.
pub async fn set_string(db: &DatabaseConnection, name: &str, value: &str) -> Result<(), DbErr> {
    refuse_secret(name)?;
    upsert(db, name, serde_json::Value::String(value.to_owned())).await
}

fn refuse_secret(name: &str) -> Result<(), DbErr> {
    if SECRET_KEYS.contains(&name) {
        Err(DbErr::Custom(format!("{name} is a secret setting")))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_db;

    #[tokio::test]
    async fn round_trip_and_wrong_key() {
        let (db, _dir) = temp_db().await;
        let key = SecretKey::derive("root");
        assert_eq!(get_secret(&db, &key, "tmdb.api_key").await.unwrap(), None);

        set_secret(&db, &key, "tmdb.api_key", &Secret::new("one"))
            .await
            .unwrap();
        set_secret(&db, &key, "tmdb.api_key", &Secret::new("two"))
            .await
            .unwrap();
        assert_eq!(
            get_secret(&db, &key, "tmdb.api_key").await.unwrap(),
            Some(SecretSetting::Set(Secret::new("two")))
        );
        assert_eq!(
            get_secret(&db, &SecretKey::derive("lost"), "tmdb.api_key")
                .await
                .unwrap(),
            Some(SecretSetting::MustReenter)
        );
    }

    #[tokio::test]
    async fn a_non_secret_key_is_refused() {
        let (db, _dir) = temp_db().await;
        let key = SecretKey::derive("root");
        assert!(
            set_secret(&db, &key, "tmdb.language", &Secret::new("x"))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn database_files_do_not_contain_the_plain_text() {
        let (db, dir) = temp_db().await;
        let key = SecretKey::derive("root");
        set_secret(
            &db,
            &key,
            "proxy.url",
            &Secret::new("http://user:needle-9f3a@proxy"),
        )
        .await
        .unwrap();
        db.close().await.unwrap();
        let mut checked = 0;
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let bytes = std::fs::read(entry.unwrap().path()).unwrap();
            checked += 1;
            assert!(!bytes.windows(11).any(|w| w == b"needle-9f3a"));
        }
        assert!(checked > 0);
    }

    #[tokio::test]
    async fn plain_text_round_trip() {
        let (db, _dir) = temp_db().await;
        assert_eq!(get_string(&db, "tmdb.language").await.unwrap(), None);
        set_string(&db, "tmdb.language", "fr-FR").await.unwrap();
        set_string(&db, "tmdb.language", "de-DE").await.unwrap();
        assert_eq!(
            get_string(&db, "tmdb.language").await.unwrap().as_deref(),
            Some("de-DE")
        );
    }

    #[tokio::test]
    async fn plain_text_helpers_refuse_secret_keys() {
        let (db, _dir) = temp_db().await;
        assert!(set_string(&db, "tmdb.api_key", "x").await.is_err());
        assert!(get_string(&db, "tmdb.api_key").await.is_err());
    }
}
