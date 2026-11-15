# 0002 SQLite with SeaORM 2

Status: accepted (2026-11-15)

## Context

A household library has thousands of rows, not millions. Users expect one
container and one folder to back up. The authentication library (ADR 0006)
runs on the application's SeaORM 2 entities.

## Decision

SQLite in WAL mode, accessed through SeaORM 2 (stable 2.0.x; the auth
library requires `^2.0.0-rc.37`). Schema changes are explicit, versioned
migrations with `sea-orm-migration`, run at startup. No automatic schema
sync outside local development. Auth and app tables share one migrator.

## Consequences

- No database server to run. Backup is one file.
- One writer at a time. Long write transactions must be avoided; bulk import
  works in batches.
- PostgreSQL is not supported. SeaORM keeps that door open, but nothing is
  tested against it.
- The auth library's SeaORM integration enables the `sqlx-postgres` and
  `sqlx-mysql` features as well, which adds compile time and binary size.
- Raw SQL is allowed inside `crates/db` where entities do not fit
  ([04-database.md](../04-database.md)).
