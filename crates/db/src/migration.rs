//! Versioned migrations. Never edit a released migration; add a new one.

use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(M0001Init),
            Box::new(M0002AccessControl),
            Box::new(M0003ApiKeys),
            Box::new(M0004Movies),
            Box::new(M0005AuditLog),
        ]
    }
}

struct M0001Init;

impl MigrationName for M0001Init {
    fn name(&self) -> &str {
        "m0001_init"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for M0001Init {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        crate::auth_schema::create_auth_tables(manager.get_connection()).await?;
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("settings"))
                    .col(
                        ColumnDef::new(Alias::new("key"))
                            .text()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("value")).json().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .to_owned(),
            )
            .await
    }
}

struct M0002AccessControl;

impl MigrationName for M0002AccessControl {
    fn name(&self) -> &str {
        "m0002_access_control"
    }
}

/// Roles, grants and the `disabled` flag (docs/04-database.md, "Access
/// control tables"). Plain SQL: the partial unique index has no builder.
///
/// `user.disabled` is our column on a table of the auth library. It has a
/// default, so the library's own inserts, which do not name it, still work.
const M0002_UP: &str = r#"
ALTER TABLE "user" ADD COLUMN disabled boolean NOT NULL DEFAULT FALSE;

CREATE TABLE roles (
    id integer PRIMARY KEY AUTOINCREMENT,
    name text NOT NULL UNIQUE,
    description text,
    is_owner boolean NOT NULL DEFAULT FALSE,
    created_at text NOT NULL,
    updated_at text NOT NULL
);
-- At most one row can be the Owner role.
CREATE UNIQUE INDEX roles_single_owner ON roles (is_owner) WHERE is_owner = 1;

CREATE TABLE role_permissions (
    role_id integer NOT NULL REFERENCES roles (id) ON DELETE CASCADE,
    permission text NOT NULL,
    PRIMARY KEY (role_id, permission)
);

CREATE TABLE user_roles (
    user_id text NOT NULL REFERENCES "user" (id) ON DELETE CASCADE,
    role_id integer NOT NULL REFERENCES roles (id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, role_id)
);
CREATE INDEX user_roles_role_id_idx ON user_roles (role_id);

CREATE TABLE user_permissions (
    user_id text NOT NULL REFERENCES "user" (id) ON DELETE CASCADE,
    permission text NOT NULL,
    PRIMARY KEY (user_id, permission)
);

INSERT INTO roles (name, description, is_owner, created_at, updated_at)
VALUES ('Owner', 'Every permission. Built in.', TRUE,
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
"#;

#[async_trait::async_trait]
impl MigrationTrait for M0002AccessControl {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(M0002_UP)
            .await?;
        Ok(())
    }
}

struct M0003ApiKeys;

impl MigrationName for M0003ApiKeys {
    fn name(&self) -> &str {
        "m0003_api_keys"
    }
}

/// API keys (docs/04-database.md). Only the SHA-256 of a key is stored. A
/// revoked key keeps its row, so the audit log can still name it.
const M0003_UP: &str = r#"
CREATE TABLE api_keys (
    id integer PRIMARY KEY AUTOINCREMENT,
    user_id text NOT NULL REFERENCES "user" (id) ON DELETE CASCADE,
    name text NOT NULL,
    key_hash text NOT NULL UNIQUE,
    key_prefix text NOT NULL,
    last_used_at text,
    created_at text NOT NULL,
    revoked_at text
);
CREATE INDEX api_keys_user_id_idx ON api_keys (user_id);

CREATE TABLE api_key_permissions (
    api_key_id integer NOT NULL REFERENCES api_keys (id) ON DELETE CASCADE,
    permission text NOT NULL,
    PRIMARY KEY (api_key_id, permission)
);
"#;

#[async_trait::async_trait]
impl MigrationTrait for M0003ApiKeys {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(M0003_UP)
            .await?;
        Ok(())
    }
}

struct M0004Movies;

impl MigrationName for M0004Movies {
    fn name(&self) -> &str {
        "m0004_movies"
    }
}

/// Library tables (docs/04-database.md, "Library tables"), without
/// `collections`: the issue that adds that table also adds
/// `movies.collection_id`.
///
/// A movie has one `movie_profiles` row per attached quality profile and at
/// most one primary (ADR 0010). A file with `movie_profile_id` set is the
/// managed file of that profile; a file without is unmanaged.
const M0004_UP: &str = r#"
CREATE TABLE root_folders (
    id integer PRIMARY KEY AUTOINCREMENT,
    path text NOT NULL UNIQUE,
    created_at text NOT NULL
);

CREATE TABLE quality_profiles (
    id integer PRIMARY KEY AUTOINCREMENT,
    name text NOT NULL UNIQUE,
    upgrade_allowed boolean NOT NULL,
    cutoff_quality_id integer NOT NULL,
    items json NOT NULL,
    language text NOT NULL DEFAULT 'any',
    min_format_score integer NOT NULL DEFAULT 0,
    cutoff_format_score integer NOT NULL DEFAULT 0,
    min_upgrade_format_score integer NOT NULL DEFAULT 1,
    trash_id text,
    created_at text NOT NULL,
    updated_at text NOT NULL
);

CREATE TABLE tags (
    id integer PRIMARY KEY AUTOINCREMENT,
    label text NOT NULL UNIQUE
);

CREATE TABLE movies (
    id integer PRIMARY KEY AUTOINCREMENT,
    tmdb_id integer NOT NULL UNIQUE,
    imdb_id text,
    title text NOT NULL,
    original_title text,
    sort_title text NOT NULL,
    year integer NOT NULL,
    overview text,
    runtime integer,
    status text NOT NULL,
    in_cinemas text,
    digital_release text,
    physical_release text,
    poster_path text,
    root_folder_id integer NOT NULL REFERENCES root_folders (id),
    folder_name text NOT NULL,
    monitored boolean NOT NULL,
    minimum_availability text NOT NULL,
    added_at text NOT NULL,
    updated_at text NOT NULL
);
CREATE INDEX movies_root_folder_id_idx ON movies (root_folder_id);

CREATE TABLE movie_profiles (
    id integer PRIMARY KEY AUTOINCREMENT,
    movie_id integer NOT NULL REFERENCES movies (id) ON DELETE CASCADE,
    quality_profile_id integer NOT NULL REFERENCES quality_profiles (id),
    is_primary boolean NOT NULL,
    created_at text NOT NULL,
    UNIQUE (movie_id, quality_profile_id)
);
-- At most one primary profile per movie.
CREATE UNIQUE INDEX movie_profiles_single_primary ON movie_profiles (movie_id) WHERE is_primary = 1;
CREATE INDEX movie_profiles_quality_profile_id_idx ON movie_profiles (quality_profile_id);

CREATE TABLE movie_tags (
    movie_id integer NOT NULL REFERENCES movies (id) ON DELETE CASCADE,
    tag_id integer NOT NULL REFERENCES tags (id) ON DELETE CASCADE,
    PRIMARY KEY (movie_id, tag_id)
);
CREATE INDEX movie_tags_tag_id_idx ON movie_tags (tag_id);

CREATE TABLE movie_files (
    id integer PRIMARY KEY AUTOINCREMENT,
    movie_id integer NOT NULL REFERENCES movies (id) ON DELETE CASCADE,
    relative_path text NOT NULL,
    size integer NOT NULL,
    quality_id integer NOT NULL,
    release_group text,
    edition text,
    scene_name text,
    media_info json,
    languages json,
    movie_profile_id integer UNIQUE REFERENCES movie_profiles (id) ON DELETE SET NULL,
    added_at text NOT NULL,
    UNIQUE (movie_id, relative_path)
);
"#;

#[async_trait::async_trait]
impl MigrationTrait for M0004Movies {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(M0004_UP)
            .await?;
        Ok(())
    }
}

struct M0005AuditLog;

impl MigrationName for M0005AuditLog {
    fn name(&self) -> &str {
        "m0005_audit_log"
    }
}

/// The audit log (docs/04-database.md). Append-only: nothing in the app
/// updates or deletes a row. The actor columns are set to null when the user
/// or key is deleted; `actor_name` was copied at write time, so the row still
/// says who acted.
const M0005_UP: &str = r#"
CREATE TABLE audit_log (
    id integer PRIMARY KEY AUTOINCREMENT,
    created_at text NOT NULL,
    actor_user_id text REFERENCES "user" (id) ON DELETE SET NULL,
    actor_name text NOT NULL,
    api_key_id integer REFERENCES api_keys (id) ON DELETE SET NULL,
    action text NOT NULL,
    target_kind text,
    target_id text,
    data json,
    ip_address text
);
CREATE INDEX audit_log_created_at_idx ON audit_log (created_at);
CREATE INDEX audit_log_actor_user_id_idx ON audit_log (actor_user_id);
CREATE INDEX audit_log_target_idx ON audit_log (target_kind, target_id);
"#;

#[async_trait::async_trait]
impl MigrationTrait for M0005AuditLog {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(M0005_UP)
            .await?;
        Ok(())
    }
}
