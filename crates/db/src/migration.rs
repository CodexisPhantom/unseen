//! Versioned migrations. Never edit a released migration; add a new one.

use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(M0001Init), Box::new(M0002AccessControl)]
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
