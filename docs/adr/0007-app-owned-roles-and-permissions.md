# 0007 App-owned roles and permissions with a built-in Owner

Status: accepted (2026-11-15)

## Context

The Owner must be able to create roles at runtime, for example "kids" or
"friends". The auth library's admin and organization plugins define roles in
code or configuration. Sonarr and Radarr have a single user.

## Decision

- Permissions are a fixed catalog in code: a Rust enum in `crates/core`,
  serialised as `resource:action`.
- Roles are rows in our tables (`roles`, `role_permissions`, `user_roles`),
  managed from the UI. A user's effective permissions are the union of their
  roles.
- One built-in Owner role: every permission including future ones, not
  editable, not deletable, at least one holder at all times.
- Every route declares exactly one permission or is marked public. An Axum
  extractor enforces it. A test fails when a route declares neither.
- Permissions are loaded on each request. Nothing is cached in the session.
- No privilege escalation: you can only grant what you hold, and only an
  Owner can modify an Owner.
- API keys belong to a user and carry a subset of that user's permissions.

## Consequences

- The permission model is independent of the auth library.
- One or two extra queries per request. Acceptable on SQLite; cache per
  request only.
- Adding a permission is a code change. Custom roles do not receive new
  permissions automatically; the Owner does.
- Finer scoping (per root folder, per user grants) is not possible yet. It is
  listed as an open question in [15-permissions.md](../15-permissions.md).
