# 11 Security notes

Status: draft. The permission model is in [15-permissions.md](15-permissions.md).

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

## Passwords

Hashed by the auth library (Argon2; confirm parameters in the spike). Minimum
length 10. No composition rules. Login is rate limited per IP and per account.

## CSRF

- Auth endpoints: the library's origin check.
- `/api/v1`: cookie requests that change state must carry `Origin` matching
  the base URL, and `Content-Type: application/json`. Confirm this is enough
  in M1.
- API-key requests have no cookie and need no CSRF check.

## Secrets at rest

| Secret | Storage |
|---|---|
| User passwords | Argon2 hash (auth library) |
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

- Audit log retention.
- Rotation of `UNSEEN_SECRET` (re-encrypt all secrets): needed, or document
  "re-enter the secrets".
