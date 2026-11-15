# 08 External integration contracts

Status: draft. Verify every endpoint against the provider's current
documentation when its issue is implemented.

All clients: one shared `reqwest` client, timeouts (10 s connect, 30 s total),
a user agent `Unseen/<version>`, and typed errors that map to HTTP 502 in our
API. Tests use recorded fixtures served by a local mock HTTP server
(`wiremock`). No test calls the real service.

## TMDB (M2)

| | |
|---|---|
| Endpoints | `GET /3/search/movie`, `GET /3/movie/{id}?append_to_response=external_ids,release_dates`, `GET /3/find/{imdb_id}`, `GET /3/configuration` |
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

## Newznab (M9)

Same as Torznab with NZB enclosures and `newznab:attr`. Details in M9.

## qBittorrent (M4)

| | |
|---|---|
| Endpoints (WebUI API v2) | `POST /api/v2/auth/login`, `GET /api/v2/app/version`, `POST /api/v2/torrents/add`, `GET /api/v2/torrents/info?category=`, `GET /api/v2/torrents/properties`, `POST /api/v2/torrents/delete`, `POST /api/v2/torrents/createCategory`, `GET /api/v2/app/preferences` |
| Auth | Cookie session from username and password; log in again on 403 |
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
| Endpoints (v3, read-only) | `GET /api/v3/system/status`, `/movie`, `/rootfolder`, `/qualityprofile`, `/tag`, `/indexer`, `/downloadclient` |
| Auth | `X-Api-Key` header |
| Rate limits | None |
| Failure | Import stops with a report; nothing partial is hidden; safe to re-run |
| Mock | Recorded responses from a real Radarr, shared with the contract tests |

Radarr masks secret fields in `/indexer` and `/downloadclient` responses.
Open question: import those entries disabled and ask the user to re-enter the
secret.

## Sonarr (M8), Prowlarr (M11)

Sonarr: v3, same pattern with `/series`. Prowlarr: API v1, `GET /api/v1/indexer`.

## Open questions

- Optional HTTP proxy for indexer traffic.
- Bazarr settings import: practical or not.
