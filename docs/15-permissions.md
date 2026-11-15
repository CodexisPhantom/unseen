# 15 Accounts and access control

Status: draft. Decisions: ADR 0006 (authentication), ADR 0007 (authorization).

## First run

1. While the `user` table is empty, the app serves only the setup page,
   `POST /api/v1/setup`, the health check and static files. Everything else
   returns 503 with code `setup_required`.
2. The setup endpoint creates the first user and assigns the Owner role in
   one transaction.
3. After that the setup endpoint returns 409 for good. Public sign-up never
   exists. Only users with `users:manage` create accounts.

## Permission catalog

Fixed in code as a Rust enum in `crates/core`. Format `resource:action`.

| Permission | Meaning |
|---|---|
| `library:read` | See movies, files and tags |
| `library:add` | Add a movie |
| `library:edit` | Edit a movie, its tags, monitored state; create and rename tags |
| `library:delete` | Remove a movie, with or without files |
| `search:run` | Start a manual or automatic search |
| `release:grab` | Send a chosen release to a download client |
| `queue:read` | See downloads and their progress |
| `queue:manage` | Remove, retry or blocklist downloads |
| `import:run` | Adopt a library folder; run an import from Radarr/Sonarr |
| `subtitles:manage` | Search, download and delete subtitles (M10) |
| `indexers:read` | See indexers without secrets |
| `indexers:manage` | Create, edit, delete and test indexers |
| `clients:read` | See download clients without secrets |
| `clients:manage` | Create, edit, delete and test download clients |
| `profiles:read` | See quality profiles and qualities |
| `profiles:manage` | Create, edit and delete quality profiles |
| `settings:read` | See settings and root folders |
| `settings:manage` | Change settings and root folders |
| `users:manage` | Create, edit, disable and delete users; assign roles |
| `roles:manage` | Create, edit and delete roles |
| `apikeys:manage` | Manage API keys of other users |
| `system:logs` | Read logs |
| `system:backup` | Create and download backups; restore |
| `system:tasks` | See scheduled tasks and run them now |

Every user manages their own API keys without a permission.

## Owner role

- Built in. Has every permission, including ones added in later versions.
- Cannot be edited or deleted.
- At least one enabled user must hold it. The last Owner cannot be disabled,
  deleted or lose the role.
- Only an Owner can modify an Owner or grant the Owner role.

## Custom roles

- A named set of permissions, stored in the database, managed in the UI.
- A user can have several roles. Effective permissions are the union.
- A user without a role can log in and sees nothing.
- Templates on the roles page only pre-fill the form:

| Template | Permissions |
|---|---|
| Admin | Every permission in the current catalog, listed explicitly. Does not gain future permissions; the Owner-only rules still apply |
| Member | `library:read`, `library:add`, `search:run`, `queue:read` |
| Read-only | `library:read`, `queue:read` |

## Escalation rules

- With `roles:manage`: a role may only contain permissions the editor holds.
  Editing a role that contains a permission the editor lacks is refused.
- With `users:manage`: only roles whose permissions the editor fully holds
  can be assigned or removed.
- Nobody but an Owner can change an Owner's account, roles or password.
- A user cannot remove their own last path to `roles:manage` by accident:
  the UI asks for confirmation. Open question: enforce or only warn.

## API keys

- Belong to a user. Limited to a chosen subset of that user's permissions.
- Effective permissions of a key = chosen subset ∩ the user's current
  permissions. Removing a role from the user narrows the key at once.
- Keys of a disabled user stop working.
- Shown once at creation. Stored hashed.
- Accepted as `X-Api-Key` header or `?apikey=` query on `/api/v1` and the
  compat APIs.

## Enforcement

- Server-side only. Each route declares exactly one permission or is public.
- An Axum extractor resolves the caller (session or API key), loads roles and
  permissions for this request, and checks. Nothing is cached in the session.
- 401 without valid credentials, 403 when the permission is missing.
- `x-permission` in OpenAPI is generated from the same declaration.
- `GET /api/v1/me` returns user, roles and effective permissions. The UI
  hides what the user cannot do; security never depends on that.
- SSE events are filtered by permission when sent.
- Secrets are write-only for callers without `*:manage` on that resource.

## Route-to-permission table

The generated OpenAPI document is authoritative. This table is the plan.

| Route | Permission |
|---|---|
| `GET /health`, `POST /setup`, `/api/auth/*` sign-in and sign-out | public |
| `GET /me`, `GET/POST/DELETE /me/api-keys` | authenticated, no permission |
| `GET /permissions`, `/roles` (all methods) | `roles:manage` |
| `/users` (all methods) | `users:manage` |
| `/api-keys` (other users' keys) | `apikeys:manage` |
| `GET /movies`, `/movies/{id}`, `/tags` | `library:read` |
| `GET /movies/lookup`, `POST /movies` | `library:add` |
| `PATCH /movies/{id}`, `POST/PATCH/DELETE /tags` | `library:edit` |
| `DELETE /movies/{id}` | `library:delete` |
| `GET /quality-profiles`, `/qualities` | `profiles:read` |
| `POST/PATCH/DELETE /quality-profiles` | `profiles:manage` |
| `GET /root-folders`, `/settings` | `settings:read` |
| `POST/DELETE /root-folders`, `PATCH /settings` | `settings:manage` |
| `POST /library/scan`, `/library/adopt`, `/import-sources/*` | `import:run` |
| `GET /indexers` | `indexers:read` |
| `POST/PATCH/DELETE /indexers`, `/indexers/{id}/test` | `indexers:manage` |
| `GET /movies/{id}/releases`, `POST /movies/{id}/search` | `search:run` |
| `POST /releases/grab` | `release:grab` |
| `GET /download-clients` | `clients:read` |
| `POST/PATCH/DELETE /download-clients` | `clients:manage` |
| `GET /queue`, `/history` | `queue:read` |
| `DELETE /queue/{id}`, `POST /queue/{id}/retry`, `/blocklist` | `queue:manage` |
| `GET /events` | authenticated; events filtered per permission |
| `GET /tasks`, `POST /tasks/{name}/run` | `system:tasks` |
| `GET /logs` | `system:logs` |
| `POST /backups`, `GET /backups/{id}` | `system:backup` |
| `/radarr/api/v3/*` | see [14-compatibility.md](14-compatibility.md) |

`GET /roles` for the user form: a user with `users:manage` but not
`roles:manage` must still list roles. Open question: a second read route, or
include assignable roles in the users API.

## Open questions

- Scoping per library or root folder ("kids can only see the kids folder").
- Direct per-user grants in addition to roles.
- Ownership transfer flow.
- Audit log.
- Request and approval flow.
- Exact content of the Admin template.
