//! Helpers for tests in this crate and in the crates that depend on it.

use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};
use tempfile::TempDir;

/// Creates a migrated database in a new temporary directory. The directory
/// is deleted when the returned `TempDir` is dropped.
pub async fn temp_db() -> (DatabaseConnection, TempDir) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db = crate::connect(dir.path())
        .await
        .expect("migrate temp database");
    (db, dir)
}

/// Inserts a user the way the auth library does: without our app columns.
pub async fn insert_user(db: &DatabaseConnection, id: &str) {
    let now = sea_orm::prelude::ChronoUtc::now();
    crate::auth_schema::user::ActiveModel {
        id: Set(id.into()),
        name: Set(id.into()),
        email: Set(Some(format!("{id}@example.com"))),
        email_verified: Set(false),
        image: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
        two_factor_enabled: sea_orm::ActiveValue::NotSet,
    }
    .insert(db)
    .await
    .expect("insert user");
}
