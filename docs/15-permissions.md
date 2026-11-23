# 15 Accounts and access control

Status: reviewed (2026-11-16). Decisions: ADR 0006 (authentication), ADR 0007 (authorization).

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
| `library:read` | See movies, collections, files and tags |
| `library:add` | Add a movie; add a profile to a movie |
| `library:edit` | Edit a movie, its tags, monitored state; monitor a collection; create and rename tags |
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
| `profiles:read` | See quality profiles, qualities, quality definitions, custom formats and the TRaSH guide browser |
| `profiles:manage` | Create, edit and delete quality profiles and custom formats; edit quality definitions; apply TRaSH guide items |
| `settings:read` | See settings and root folders |
| `settings:manage` | Change settings and root folders |
| `users:manage` | Create, edit, disable and delete users; assign roles and direct grants |
| `roles:manage` | Create, edit and delete roles |
| `apikeys:manage` | Manage API keys of other users |
| `system:logs` | Read logs |
| `system:backup` | Create and download backups; restore |
| `system:tasks` | See scheduled tasks and run them now |
| `system:audit` | Read the audit log |

Every user manages their own API keys without a permission.

## Owner role

- Built in. Has every permission, including ones added in later versions.
- Cannot be edited or deleted.
- At least one enabled user must hold it. The last Owner cannot be disabled,
  deleted or lose the role.
- Only an Owner can modify an Owner or grant the Owner role.

- Ownership transfer: an Owner opens "Transfer ownership", picks an enabled
  user and confirms with their own password. In one transaction the target
  gets the Owner role and the current Owner loses it (and keeps or receives
  another role chosen in the dialog). The action is written to the audit log
  and both users' sessions reload their permissions. Granting the Owner role
  to a second user without giving it up stays possible.

## Custom roles

- A named set of permissions, stored in the database, managed in the UI.
- A user can have several roles. Effective permissions are the union of all
  roles and the user's direct grants.
- A user without a role or grant can log in and sees nothing.
- Templates on the roles page only pre-fill the form:

| Template | Permissions |
|---|---|
| Admin | Every permission in the catalog at the time the role is created, listed explicitly. Does not gain future permissions; the Owner-only rules still apply |
| Member | `library:read`, `library:add`, `search:run`, `queue:read` |
| Read-only | `library:read`, `queue:read` |

## Direct grants

A permission can be given to one user without creating a role, for example
`queue:manage` for one family member. Stored in `user_permissions`.

- Grants only add. There is no "deny" entry; a deny rule would make the
  effective set hard to predict.
- The users page shows roles and direct grants separately, and the effective
  set with the source of each permission.

## Escalation rules

- With `roles:manage`: a role may only contain permissions the editor holds.
  Editing a role that contains a permission the editor lacks is refused.
- With `users:manage`: only roles whose permissions the editor fully holds
  can be assigned or removed, and only permissions the editor holds can be
  granted directly or taken away.
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
| `GET /health`, `GET /setup` (status), `POST /setup`, `/api/auth/*` sign-in and sign-out | public |
| `GET /me`, `GET/POST/DELETE /me/api-keys` | authenticated, no permission |
| `GET /permissions`, `/roles` (all methods) | `roles:manage` |
| `/users` (all methods), `PUT /users/{id}/permissions` | `users:manage` |
| `POST /owner/transfer` | authenticated; the handler requires the Owner role and the password |
| `GET /audit-log` | `system:audit` |
| `/api-keys` (other users' keys) | `apikeys:manage` |
| `GET /movies`, `/movies/{id}`, `/movies/{id}/files`, `/collections`, `/tags` | `library:read` |
| `GET /movies/lookup`, `POST /movies`, `POST /movies/{id}/profiles` | `library:add` |
| `PATCH /movies/{id}`, `PATCH /collections/{id}`, `POST/PATCH/DELETE /tags` | `library:edit` |
| `DELETE /movies/{id}`, `DELETE /movies/{id}/profiles/{pid}`, `DELETE /movies/{id}/files/{fid}` | `library:delete` |
| `GET /quality-profiles`, `/qualities`, `/quality-definitions`, `/custom-formats`, `/trash-guides`, `POST /custom-formats/test` | `profiles:read` |
| `POST/PATCH/DELETE /quality-profiles`, `/custom-formats`; `PATCH /quality-definitions`; `POST /custom-formats/import`, `/trash-guides/apply` | `profiles:manage` |
| `POST /trash-guides/sync` | `system:tasks` |
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

## Scoping per library (after V1)

Decided direction, not built in V1: access is scoped per library, not per
root folder. A library is a named group of root folders ("Movies", "Kids").
A role assignment or a direct grant then applies to all libraries or to
chosen ones. In V1 every permission covers the whole server. Adding
libraries later is a migration that puts every root folder into one default
library, so V1 data needs no special preparation. Planned for M11.

## Decisions (2026-11-15)

- Scoping: per library, after V1.
- Direct per-user grants: yes, in V1.
- Ownership transfer: yes, in V1.
- Audit log: yes, in V1, with the permission `system:audit` (25 permissions
  in total).
- Request and approval flow: wanted, after V1. Seerr covers it until then. A
  design issue is in M11.
- Admin template: every permission in the catalog when the role is created.
  It is only a form pre-fill; the Owner can untick anything.

## Open questions

- Does a request flow need new permissions (`requests:create`,
  `requests:approve`)? Decide in its design.
