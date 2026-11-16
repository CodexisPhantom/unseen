//! Helpers for tests in this crate and in the crates that depend on it.

use sea_orm::DatabaseConnection;
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
