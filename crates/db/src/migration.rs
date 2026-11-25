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
            Box::new(M0006Indexers),
            Box::new(M0007DownloadClients),
            Box::new(M0008DownloadClientApiKey),
            Box::new(M0009QualityDefinitions),
            Box::new(M0010Subtitles),
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

struct M0006Indexers;

impl MigrationName for M0006Indexers {
    fn name(&self) -> &str {
        "m0006_indexers"
    }
}

/// Indexers and their tags (docs/04-database.md, "Integration tables").
/// `api_key` holds the encrypted form (ADR 0011).
const M0006_UP: &str = r#"
CREATE TABLE indexers (
    id integer PRIMARY KEY AUTOINCREMENT,
    name text NOT NULL UNIQUE,
    kind text NOT NULL,
    url text NOT NULL,
    api_key text NOT NULL,
    categories json NOT NULL,
    priority integer NOT NULL DEFAULT 25,
    use_proxy boolean NOT NULL DEFAULT TRUE,
    enabled boolean NOT NULL,
    created_at text NOT NULL
);

CREATE TABLE indexer_tags (
    indexer_id integer NOT NULL REFERENCES indexers (id) ON DELETE CASCADE,
    tag_id integer NOT NULL REFERENCES tags (id) ON DELETE CASCADE,
    PRIMARY KEY (indexer_id, tag_id)
);
CREATE INDEX indexer_tags_tag_id_idx ON indexer_tags (tag_id);
"#;

#[async_trait::async_trait]
impl MigrationTrait for M0006Indexers {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(M0006_UP)
            .await?;
        Ok(())
    }
}

struct M0007DownloadClients;

impl MigrationName for M0007DownloadClients {
    fn name(&self) -> &str {
        "m0007_download_clients"
    }
}

/// Download clients and their tags (docs/04-database.md, "Integration
/// tables"). `password` holds the encrypted form (ADR 0011). `remote_path`
/// and `local_path` are the client's remote path mapping: both set or both
/// null.
const M0007_UP: &str = r#"
CREATE TABLE download_clients (
    id integer PRIMARY KEY AUTOINCREMENT,
    name text NOT NULL UNIQUE,
    kind text NOT NULL,
    url text NOT NULL,
    username text,
    password text,
    category text NOT NULL,
    remote_path text,
    local_path text,
    remove_completed boolean NOT NULL DEFAULT FALSE,
    enabled boolean NOT NULL,
    created_at text NOT NULL,
    CHECK ((remote_path IS NULL) = (local_path IS NULL))
);

CREATE TABLE download_client_tags (
    download_client_id integer NOT NULL REFERENCES download_clients (id) ON DELETE CASCADE,
    tag_id integer NOT NULL REFERENCES tags (id) ON DELETE CASCADE,
    PRIMARY KEY (download_client_id, tag_id)
);
CREATE INDEX download_client_tags_tag_id_idx ON download_client_tags (tag_id);
"#;

#[async_trait::async_trait]
impl MigrationTrait for M0007DownloadClients {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(M0007_UP)
            .await?;
        Ok(())
    }
}

struct M0008DownloadClientApiKey;

impl MigrationName for M0008DownloadClientApiKey {
    fn name(&self) -> &str {
        "m0008_download_client_api_key"
    }
}

/// The API key of qBittorrent 5.2 or later, as an alternative to the user
/// name and password. Encrypted like the password (ADR 0011).
const M0008_UP: &str = "ALTER TABLE download_clients ADD COLUMN api_key text;";

#[async_trait::async_trait]
impl MigrationTrait for M0008DownloadClientApiKey {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(M0008_UP)
            .await?;
        Ok(())
    }
}

struct M0009QualityDefinitions;

impl MigrationName for M0009QualityDefinitions {
    fn name(&self) -> &str {
        "m0009_quality_definitions"
    }
}

/// Size limits per quality, in megabytes per minute of runtime, seeded with
/// Radarr's defaults (`GET /api/v3/qualitydefinition` of a new Radarr 7.0.3;
/// the same values as `Quality.DefaultQualityDefinitions` in its source). A
/// null `max_size` is no limit; a null `preferred_size` is no preference.
const M0009_UP: &str = r#"
CREATE TABLE quality_definitions (
    quality_id integer PRIMARY KEY,
    title text NOT NULL,
    min_size real NOT NULL,
    preferred_size real,
    max_size real
);

INSERT INTO quality_definitions (quality_id, title, min_size, preferred_size, max_size) VALUES
    (0, 'Unknown', 0, 95, 100),
    (24, 'WORKPRINT', 0, 95, 100),
    (25, 'CAM', 0, 95, 100),
    (26, 'TELESYNC', 0, 95, 100),
    (27, 'TELECINE', 0, 95, 100),
    (29, 'REGIONAL', 0, 95, 100),
    (28, 'DVDSCR', 0, 95, 100),
    (1, 'SDTV', 0, 95, 100),
    (2, 'DVD', 0, 95, 100),
    (23, 'DVD-R', 0, 95, 100),
    (8, 'WEBDL-480p', 0, 95, 100),
    (12, 'WEBRip-480p', 0, 95, 100),
    (20, 'Bluray-480p', 0, 95, 100),
    (21, 'Bluray-576p', 0, 95, 100),
    (4, 'HDTV-720p', 0, 95, 100),
    (5, 'WEBDL-720p', 0, 95, 100),
    (14, 'WEBRip-720p', 0, 95, 100),
    (6, 'Bluray-720p', 0, 95, 100),
    (9, 'HDTV-1080p', 0, 95, 100),
    (3, 'WEBDL-1080p', 0, 95, 100),
    (15, 'WEBRip-1080p', 0, 95, 100),
    (7, 'Bluray-1080p', 0, NULL, NULL),
    (30, 'Remux-1080p', 0, NULL, NULL),
    (16, 'HDTV-2160p', 0, NULL, NULL),
    (18, 'WEBDL-2160p', 0, NULL, NULL),
    (17, 'WEBRip-2160p', 0, NULL, NULL),
    (19, 'Bluray-2160p', 0, NULL, NULL),
    (31, 'Remux-2160p', 0, NULL, NULL),
    (22, 'BR-DISK', 0, NULL, NULL),
    (10, 'Raw-HD', 0, NULL, NULL);
"#;

#[async_trait::async_trait]
impl MigrationTrait for M0009QualityDefinitions {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(M0009_UP)
            .await?;
        Ok(())
    }
}

struct M0010Subtitles;

impl MigrationName for M0010Subtitles {
    fn name(&self) -> &str {
        "m0010_subtitles"
    }
}

/// Subtitle tables (docs/04-database.md, "Subtitle tables (M10)"). A movie
/// has at most one language profile; a profile that a movie uses cannot be
/// deleted. A file has at most one subtitle per language and kind.
const M0010_UP: &str = r#"
CREATE TABLE language_profiles (
    id integer PRIMARY KEY AUTOINCREMENT,
    name text NOT NULL UNIQUE,
    items json NOT NULL,
    cutoff integer,
    created_at text NOT NULL,
    updated_at text NOT NULL
);

ALTER TABLE movies ADD COLUMN language_profile_id integer
    REFERENCES language_profiles (id) ON DELETE RESTRICT;
CREATE INDEX movies_language_profile_id_idx ON movies (language_profile_id);

CREATE TABLE subtitles (
    id integer PRIMARY KEY AUTOINCREMENT,
    movie_file_id integer NOT NULL REFERENCES movie_files (id) ON DELETE CASCADE,
    language text NOT NULL,
    kind text NOT NULL CHECK (kind IN ('normal', 'hi', 'forced')),
    relative_path text NOT NULL,
    provider text,
    provider_id text,
    score integer,
    added_at text NOT NULL,
    UNIQUE (movie_file_id, language, kind)
);
CREATE INDEX subtitles_movie_file_id_idx ON subtitles (movie_file_id);
"#;

#[async_trait::async_trait]
impl MigrationTrait for M0010Subtitles {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(M0010_UP)
            .await?;
        Ok(())
    }
}
