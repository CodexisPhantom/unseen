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

## Subtitle providers

`core` defines a subtitle provider trait; OpenSubtitles is the only V1
implementation and other providers are out of scope. The rest of the code
depends on the trait only.

```rust
trait SubtitleProvider: Send + Sync {
    /// Candidates for one movie, best provider order first. No quota used.
    async fn search(&self, query: &SubtitleQuery) -> Result<Vec<SubtitleCandidate>, SubtitleError>;
    /// The subtitle file of one candidate. Uses one unit of quota.
    async fn download(&self, id: &str) -> Result<DownloadedSubtitle, SubtitleError>;
}
```

- `SubtitleQuery`: TMDB id, IMDb id, title, year, the languages and kinds
  wanted, the 64-bit OpenSubtitles hash of the video file, and the file name.
- `SubtitleCandidate`: the provider's id, language, kind, release name,
  whether the hash matched, whether the uploader is trusted, download count,
  rating, and whether it is machine or AI translated.
- `DownloadedSubtitle`: the bytes, the file extension, and the quota left
  and when it resets, when the provider says so.
- `SubtitleError`: `NotFound`, `Unauthorized` (bad API key or login),
  `QuotaExhausted { resets_at }`, `RateLimited` (still limited after the
  retries) and `Unavailable(String)`. They map to HTTP 502, except
  `QuotaExhausted`, which maps to 429 with the reset time.

Scoring of candidates (below) is in `core`, not in the provider, so that a
second provider needs no new scoring code.

## OpenSubtitles (M10)

| | |
|---|---|
| Endpoints (REST API v1, `https://api.opensubtitles.com/api/v1`) | `POST /login`, `GET /subtitles`, `POST /download` |
| Auth | A consumer API key in the `Api-Key` header on every request, and the bearer token that `/login` returns for downloads. The user enters their own API key, username and password in settings (stored encrypted). Unseen ships no shared key. |
| Search | By `tmdb_id`, `languages`, `moviehash`, `hearing_impaired` and `foreign_parts_only`. A search uses no download quota. |
| Download quota | Each `POST /download` uses one unit. The answer carries the units left and the reset time; Unseen keeps them and never hard-codes a number, because the quota depends on the account level and the provider changes it. At zero, downloads stop until the reset time and the next scheduled run tries again. A 429 on a download is treated the same way. |
| Rate limits | Search and download calls go through one limiter, a minimum delay between calls (default 1 s). On 429, honor `Retry-After`, else back off. |
| Failure | A failed search leaves the subtitle wanted and records the error on the task. A bad API key or login disables the provider and shows an error in settings until it is changed. |
| Mock | JSON fixtures: search results, a login answer, a download answer, a quota-exhausted answer |

Open question for #106: confirm the request fields and the quota numbers
against the provider's current documentation, which could not be read when
this design was written. The behaviour above does not depend on the numbers.

### Which subtitle is chosen

A candidate is rejected when its kind is not the one the profile item asks
for, or when it is machine or AI translated (setting
`subtitles.exclude_machine_translated`, default on). The others get a score
out of 100:

| Match | Points |
|---|---|
| Same TMDB id (always true for a search by id) | 40 |
| Hash of the video file matches | 30 |
| Release group in the file name equals the candidate's | 12 |
| Source (`Bluray`, `WEBDL` and so on) equals | 8 |
| Resolution equals | 5 |
| Uploader is trusted by the provider | 5 |

The best candidate at or above `subtitles.minimum_score` (default 60) is
downloaded. A tie goes to the higher download count. Below the minimum the
subtitle stays wanted and the manual search in the UI lists every
candidate with its score. A hash match plus the TMDB id is enough (70); the
id plus the release group and the source is enough (60); the id alone is not.

### Download flow

1. For a wanted item, search with the movie's ids and the hash of the file.
2. Score the candidates and take the best one at or above the minimum.
3. `POST /download`, write the file next to the video with the name in
   [09-filesystem.md](09-filesystem.md), through a temporary file and a
   rename, and add the `subtitles` row.
4. Run after an import, and on the schedule
   `tasks.subtitle_search_interval_hours` (default 24) for items still wanted.
   A movie is searched at most once per run per item.

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
- Bazarr settings import: not in V1. Decided in the subtitles design: it is
  not practical enough to build. A Bazarr language profile has a few items
  that a user recreates in a minute, its provider logins are secrets that
  cannot be read back, and the part that matters, the subtitle files, is
  already adopted from disk because Bazarr and Unseen both name them
  `<video>.<language>[.hi|.forced].srt`. Bazarr's own API was not checked.
- Subtitle providers: OpenSubtitles only in V1, behind a trait.
