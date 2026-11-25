# 08 External integration contracts

Status: reviewed (2026-11-16). Verify every endpoint against the provider's current
documentation when its issue is implemented.

All clients: one shared `reqwest` client, timeouts (10 s connect, 30 s total),
a user agent `Unseen/<version>`, and typed errors that map to HTTP 502 in our
API. Every user-entered URL passes the outbound URL check in
[11-security.md](11-security.md). Tests use recorded fixtures served by a local mock HTTP server
(`wiremock`). No test calls the real service.

## Metadata providers

`core` defines a metadata provider trait: search, lookup by id, details,
collection details. TMDB is the V1 implementation. TVDB follows in M7. The
rest of the code depends on the trait only.

## TMDB (M2)

| | |
|---|---|
| Endpoints | `GET /3/search/movie`, `GET /3/movie/{id}?append_to_response=external_ids,release_dates`, `GET /3/find/{imdb_id}`, `GET /3/collection/{id}`, `GET /3/configuration` |
| Auth | API key or bearer token set by the Owner in settings |
| Rate limits | About 40 requests per second per IP; back off on 429 with `Retry-After` |
| Failure | Add fails with 502. Refresh keeps the stored metadata. |
| Mock | JSON fixtures per endpoint |

Open question: ship a shared TMDB key, or require the Owner's own key.

## Torznab (M3)

| | |
|---|---|
| Endpoints | `?t=caps`, `?t=movie&imdbid=&tmdbid=&q=`, `?t=search&q=`, RSS: `?t=movie` without query |
| Auth | `apikey` query parameter |
| Rate limits | Per indexer; configurable minimum delay; back off on 429 and 5xx |
| Failure | Skip the indexer for this search, record the error, disable temporarily after repeated failures with increasing back-off |
| Mock | XML fixtures: caps, results with `torznab:attr`, error response |

Use only the search modes and parameters that `caps` advertises.

Proxy: one optional proxy (HTTP, HTTPS or SOCKS5, with optional credentials)
in settings. It applies to indexer traffic only. Each indexer has a "use
proxy" switch, default on. Local addresses bypass the proxy by default.
TMDB, download clients and import sources are never proxied.

Details: the URL is `http://`, `https://`, `socks5://` or `socks5h://`, with
optional `user:password@`, stored encrypted in `proxy.url` and never returned
(the API shows the address without credentials). SOCKS5 always lets the proxy
resolve names. "Local" for the bypass means a private, loopback or
link-local address, `localhost`, a name without a dot (a container name) and
the suffixes `.local`, `.lan`, `.internal` and `.home.arpa`. The system proxy
variables (`HTTP_PROXY` and others) are ignored for indexer traffic. When a
proxy is set but cannot be used (unreadable or invalid), an indexer with the
switch on fails; it never falls back to a direct connection. Through a proxy
the target name is resolved by the proxy, so the check on resolved addresses
does not apply to it; the check on the URL itself still does.

## TRaSH guides (M5)

| | |
|---|---|
| Source | The JSON files in the public TRaSH-Guides GitHub repository (Radarr custom formats, custom format groups, quality profiles, quality sizes) |
| Auth | None |
| Rate limits | One download of the repository archive per sync; default once a day; conditional request so an unchanged guide costs nothing |
| Failure | Keep the stored copy; report the error on the task |
| Mock | A small copy of the guide folder as fixtures |

## Newznab (M9)

Same as Torznab with NZB enclosures and `newznab:attr`. Details in M9.

## qBittorrent (M4)

| | |
|---|---|
| Endpoints (WebUI API v2) | `POST /api/v2/auth/login`, `GET /api/v2/app/version`, `POST /api/v2/torrents/add`, `GET /api/v2/torrents/info?category=`, `GET /api/v2/torrents/properties`, `POST /api/v2/torrents/delete`, `POST /api/v2/torrents/createCategory`, `GET /api/v2/app/preferences` |
| Auth | API key (qBittorrent 5.2 or later): `Authorization: Bearer qbt_...` on every request, no login. Otherwise a cookie session from username and password; log in again on 403. When a client has a key, the key wins |
| Rate limits | None; poll every 5 s while downloads are active, 60 s when idle |
| Failure | See retries in [06-state-machines.md](06-state-machines.md) |
| Mock | JSON fixtures; one integration test may use a real qBittorrent container (not in default CI) |

Torrents are identified by info hash. The category is how Unseen finds its
own torrents.

## SABnzbd (M9)

`/api?mode=addurl|queue|history|version&output=json&apikey=`. Details in M9.

## OpenSubtitles (M10)

REST API with API key and user login, strict daily download quota. Details
in M10.

## Radarr as import source (M6)

| | |
|---|---|
| Endpoints (v3, read-only) | `GET /api/v3/system/status`, `/movie`, `/collection`, `/rootfolder`, `/qualityprofile`, `/qualitydefinition`, `/customformat`, `/tag`, `/indexer`, `/downloadclient` |
| Auth | `X-Api-Key` header |
| Rate limits | None |
| Failure | Import stops with a report; nothing partial is hidden; safe to re-run |
| Mock | Recorded responses from a real Radarr, shared with the contract tests |

Radarr masks secret fields in `/indexer` and `/downloadclient` responses.
Open question: import those entries disabled and ask the user to re-enter the
secret.

## Sonarr (M8), Prowlarr (M11)

Sonarr: v3, same pattern with `/series`. Prowlarr: API v1, `GET /api/v1/indexer`.

## Decisions (2026-11-15)

- Optional proxy for indexer traffic: yes, in V1.
- Bazarr settings import: not in V1. Decide in the subtitles design (M10).
