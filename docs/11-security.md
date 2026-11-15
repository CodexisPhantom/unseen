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
| Server-side request forgery through user-entered URLs | Only users with `*:manage` may enter URLs; open question: block link-local and metadata addresses |
| Stolen database file | Password hashes only; API keys hashed; integration secrets: open question below |

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
| Indexer keys, download client passwords, TMDB key, import source keys | In SQLite. Open question: plain text (as Sonarr/Radarr do) or encrypted with a key derived from `UNSEEN_SECRET` |

Secrets are write-only in the API for users without `*:manage` on that
resource, and never appear in logs.

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
- Sub-path hosting (`/unseen`) is an open question.

## Backups

The SQLite file contains password hashes and integration secrets. Treat
backups as sensitive. See [13-deployment.md](13-deployment.md).

## Open questions

- Encrypt integration secrets at rest.
- Audit log.
- SSRF restrictions on indexer and client URLs.
