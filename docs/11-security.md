# 11 Security notes

Status: reviewed (2026-11-16). The permission model is in [15-permissions.md](15-permissions.md).

## Threat model

A household server on a home network, sometimes published through a reverse
proxy.

| Threat | Handling |
|---|---|
| A household member does more than their role allows | Server-side permission on every route; no privilege escalation |
| A leaked API key in a companion tool | Keys are scoped to chosen permissions and can be revoked |
| Internet exposure through a reverse proxy | Closed sign-up, strong password hashing, rate-limited login, secure cookies |
| Malicious indexer response or release name | Strict XML parsing; paths built only from sanitised templates; never execute downloaded content |
| Path traversal through folder names or templates | Every target path is checked to stay inside a root folder |
| Server-side request forgery through user-entered URLs | Only users with `*:manage` may enter URLs; every outbound URL passes the check below |
| Stolen database file | Password hashes only; API keys hashed; integration secrets encrypted with a key that is not in the database |
| A user denies an action, or an account is misused | Audit log |

Out of scope: a hostile Owner, a compromised host, attacks on the download
clients themselves.

## Sessions and cookies

Handled by Better Auth RS (ADR 0006). Required settings:

- Cookie: `HttpOnly`, `SameSite=Lax`, `Secure` when `UNSEEN_BASE_URL` is https.
- Session lifetime 30 days, sliding. Configurable.
- Sessions of a disabled user are deleted at once.
- Permissions are loaded per request, never stored in the session.

Measured on the spike server (library defaults, http base URL, 2026-11-16):

- Cookie `better-auth.session_token`, with `HttpOnly; SameSite=Lax; Path=/;
  Max-Age=604800`. No `Secure` flag, as expected on http.
- The default lifetime is 7 days, not 30.

Measured on the mounted router (#30, tests in `crates/api/tests/auth.rs`):

- http or no base URL: `better-auth.session_token`, with `Max-Age=2592000;
  Path=/; HttpOnly; SameSite=Lax`.
- https base URL: `__Secure-better-auth.session_token`, with the same
  attributes and `Secure`.
- The lifetime is a constant of 30 days until the setting
  `auth.session_days` exists. The library moves the expiry forward at most
  once a day.

## Passwords

Hashed by the auth library. Minimum length 10. No composition rules. Login
is rate limited per IP and per account.

Measured on the spike server (2026-11-16):

- The default hasher is scrypt (N = 2^14, r = 16, p = 1), stored as
  `salt:key` in hex. This is the format of Better Auth for TypeScript.
- The library also ships `Argon2PasswordHasher` (Argon2id with the `argon2`
  crate defaults, PHC string). It does not read scrypt hashes.
- The default minimum length is 8; the maximum is 128.

Decision (2026-11-16): Unseen uses `Argon2PasswordHasher` and a minimum
length of 10. Both are set in the auth issue in M1 (#30), before the first
user exists, because the hasher cannot be changed later without resetting
every password.

## CSRF

- Auth endpoints: the library's origin check. Measured on the spike server
  (2026-11-16): a sign-in with a foreign `Origin` returns 403
  `INVALID_ORIGIN`; a request without `Origin` is accepted (curl, companion
  tools); the body must be `application/json` or
  `application/x-www-form-urlencoded`, anything else returns 415.
- `/api/v1`: cookie requests that change state must carry `Origin` matching
  the base URL, and `Content-Type: application/json`. Kept to confirm in M1:
  the check is written with the permission extractor (#31), and no route
  that changes state exists before it.
- API-key requests have no cookie and need no CSRF check.

## Secrets at rest

| Secret | Storage |
|---|---|
| User passwords | Argon2id hash (auth library, see Passwords) |
| Unseen API keys | SHA-256 hash; shown once at creation; a short prefix is stored for display |
| Indexer keys, download client passwords, TMDB key, proxy credentials, import source keys | In SQLite, encrypted (ADR 0011) |

Encryption of integration secrets:

- An encryption key is derived from `UNSEEN_SECRET` with HKDF, separate from
  the auth signing key.
- Each value is encrypted with an AEAD cipher (XChaCha20-Poly1305) and a
  random nonce, and stored as `v1:<nonce>:<ciphertext>`. The version prefix
  allows a later key or cipher change.
- One helper in `crates/db` encrypts on write and decrypts on read. No other
  code sees ciphertext.
- A value that cannot be decrypted (wrong or lost `UNSEEN_SECRET`) is
  reported as "secret must be re-entered"; the app still starts.
- A backup needs both the database and the secret. A stolen database file
  alone does not reveal the secrets.

Secrets are write-only in the API for users without `*:manage` on that
resource, and never appear in logs or in the audit log.

## Outbound URL check

Indexers, download clients and import sources are often on the local
network, so private addresses cannot be blocked. The check is:

- Scheme `http` or `https` only (proxy: also `socks5`).
- No link-local addresses (`169.254.0.0/16`, `fe80::/10`), which include the
  cloud metadata address, and no unspecified, broadcast or multicast
  addresses.
- The check runs on the resolved IP address at connect time, not only on
  the text of the URL, so a DNS name cannot point at a blocked address.
- Redirects are followed only to the same host, at most 5.
- "Test" endpoints return a status and a short message, never the raw
  response body.

## Audit log

Append-only table `audit_log`. Read with `system:audit` at
`GET /api/v1/audit-log` and on a page in the system section.

Recorded: sign-in and failed sign-in, sign-out, setup, ownership transfer;
create, change, disable and delete of users; role and direct-grant changes;
API key creation and revocation; settings changes; create, change and delete
of indexers, download clients, import sources, root folders, quality
profiles and custom formats; movie deletion with files; backup and restore.

Each row has the time, the actor (user and, if used, API key), the action,
the target, changed field names with old and new values, and the IP address.
Secret values are never written, only the fact that a secret changed.

Action names are `area.verb`: `auth.sign_in`, `auth.sign_in_failed`,
`auth.sign_out`, `setup.completed`, `user.created`, `user.updated`,
`user.deleted`, `user.permissions_changed`, `role.created`, `role.updated`,
`role.deleted`, `api_key.created`, `api_key.revoked`. A failed sign-in names
the email that was tried and no user. The address is the peer of the
connection; `UNSEEN_TRUSTED_PROXIES` is not read yet. Nothing in the app
updates or deletes a row; retention is still open (docs/04).

Note: the compat API accepts `?apikey=` because Seerr sends it that way. Keys
in URLs can end up in proxy logs. Document this for users.

## Network exposure

- One port. No other listener.
- `/api/v1/health` is the only unauthenticated endpoint after setup, besides
  login and static UI files.
- While no user exists, only the setup page and its endpoint work. Run first
  setup before exposing the server.

## VPN boundary

Only qBittorrent runs inside the Gluetun network. Unseen is not behind the
VPN and reaches qBittorrent on the Gluetun container's published port.

## Reverse proxy

- Set `UNSEEN_BASE_URL` and `UNSEEN_TRUSTED_PROXIES`.
- The proxy must not buffer `/api/v1/events` (SSE).
- Sub-path hosting (`/unseen`) is not supported in V1. Use a subdomain.

## Backups

The SQLite file contains password hashes and encrypted integration secrets.
The secret file decrypts them. Treat a backup that holds both as sensitive. See [13-deployment.md](13-deployment.md).

## Decisions (2026-11-15)

- Integration secrets are encrypted at rest.
- An audit log exists in V1.
- Outbound URLs are restricted as described above.

## Open questions

- Audit log retention. Kept open on purpose: the audit log issue (#116)
  decides it; pruning needs no schema change.

Answered (2026-11-16):

- Rotation of `UNSEEN_SECRET`: no rotation command in V1. A changed or lost
  secret means the integration secrets are entered again, and the UI shows
  which ones cannot be read. The `v1:` prefix of the stored values leaves
  room for a re-encryption command later.
