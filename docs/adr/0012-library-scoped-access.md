# 0012 Scope access per library

Status: accepted (2026-11-26)

## Context

A household wants accounts that see only part of the media collection, such as
a "Kids" account that can browse and request animated or family movies without
seeing or modifying the rest of the server.

In V1 (ADR 0007), every permission covered the whole server. Scoping per movie
was ruled out as too granular and hard to reason about, and scoping directly
per root folder mixes storage layout with access control when a logical library
spans multiple disks or mount points.

## Decision

- Introduce `libraries` as named groups of root folders. Every `root_folders`
  row belongs to exactly one library via `root_folders.library_id` (foreign key
  with `ON DELETE RESTRICT`).
- The migration seeds a single `Default` library and assigns all existing root
  folders to it. Existing installations behave identically after migration.
- Role assignments (`user_roles`) and direct grants (`user_permissions`) gain
  an `all_libraries` boolean column (default `TRUE`, preserving existing
  grants). When `all_libraries` is `FALSE`, the assignment or grant applies
  only to the library ids listed in `user_role_libraries` or
  `user_permission_libraries`.
- Keeping an explicit `all_libraries` flag prevents privilege escalation if a
  scoped library is later deleted: cascading the link row leaves
  `all_libraries = FALSE` with an empty set (zero libraries), never reverting
  to "all libraries".
- The built-in `Owner` role always applies to all libraries and cannot be
  scoped.
- Effective permissions map each `Permission` to a `LibraryScope` (`All` or
  `Libraries(BTreeSet<i32>)`), computed as the union across all assigned roles
  and direct grants. API keys inherit the owning user's `LibraryScope` for each
  permission the key carries.
- Escalation checks require that an editor holds both the permission and a
  superset of the library scope being granted or removed.
- Items outside the caller's allowed libraries are omitted from lists, search
  results, collections, queue and SSE events. Direct access to an item or root
  folder outside the caller's libraries returns `404 Not Found` rather than
  `403 Forbidden`, avoiding existence leaks.

## Consequences

- A user scoped to one library cannot observe or mutate items in another
  library through any REST endpoint, compatibility endpoint (`/radarr/api/v3`,
  `/sonarr/api/v3`), or SSE event.
- Existing installs require no manual configuration after upgrading.
- Creating, renaming and deleting libraries is managed under `settings:read` /
  `settings:manage` (with `GET /api/v1/assignable-libraries` under
  `users:manage` for the user form), so no new permissions are added to the
  catalog.
