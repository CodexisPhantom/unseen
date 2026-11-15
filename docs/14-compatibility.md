# 14 Sonarr/Radarr compatibility

Status: draft. Scope decision: ADR 0008.

"Compatible" means the four layers below. Anything not listed is not promised.

## A. On disk

| Item | V1 (movies) | Later |
|---|---|---|
| Adopt an existing library in place | Radarr | Sonarr (M8) |
| Default naming equals upstream defaults | Yes | TV |
| Template tokens accepted | See [09-filesystem.md](09-filesystem.md) | TV tokens |
| Hardlink behaviour | Same | Same |
| qBittorrent category convention | Category per client, reused on import | Same |

## B. Concepts and names

V1: quality names and ids, quality profiles with cutoff and upgrades, root
folders, tags, monitored, minimum availability. Release parsing agrees with
Radarr on the table in [07-release-scoring.md](07-release-scoring.md).

After V1, open question: custom formats, TRaSH guides, Recyclarr.

## C. Import from running instances

Read-only against the source. Safe to re-run (`import_map` table).

| Radarr resource | Endpoint | Becomes in Unseen |
|---|---|---|
| Root folders | `GET /rootfolder` | Root folder with the same path (path mapping if mounts differ) |
| Quality profiles | `GET /qualityprofile` | Quality profile; custom format scores are dropped and reported |
| Tags | `GET /tag` | Tag, matched by label |
| Movies | `GET /movie` | Movie matched by `tmdbId`, with path, monitored, minimum availability, tags; existing file adopted in place |
| Indexers | `GET /indexer` | Torznab indexers only; secrets are masked upstream, so entries arrive disabled |
| Download clients | `GET /downloadclient` | qBittorrent only; password must be re-entered |

Several instances: each import source has a target quality profile. All
movies from that source get that profile, or keep their mapped profile if no
target is set. A movie present in two instances is merged by `tmdbId`; the
conflict rule (which profile wins) is an open question.

Later: Sonarr (M8), Prowlarr indexers through its v1 API (M11), Bazarr
settings (open question).

## D. API for companion tools

Base: `/radarr/api/v3`. Auth: `X-Api-Key` header or `?apikey=` query, backed
by Unseen API keys. Paths are matched case-insensitively. Fields without a
value in Unseen are returned with a default, never omitted. Unimplemented
endpoints return 501 with a JSON body and are logged.

V1 client: Seerr. Endpoint list taken from Seerr's source,
`seerr-team/seerr` branch `develop` at commit `fc53b3d` (2026-11-15):
`server/api/servarr/base.ts`, `server/api/servarr/radarr.ts`.

| Method and path | Status | Client | Permission |
|---|---|---|---|
| `GET /system/status` | V1 | Seerr connection test | `settings:read` |
| `GET /qualityprofile` | V1 | Seerr | `profiles:read` |
| `GET /rootfolder` | V1 | Seerr | `settings:read` |
| `GET /tag` | V1 | Seerr | `library:read` |
| `POST /tag` | V1 | Seerr (tag per requester) | `library:edit` |
| `PUT /tag/{id}` | V1 | Seerr | `library:edit` |
| `GET /movie` | V1 | Seerr library scan | `library:read` |
| `GET /movie?tmdbId=` | V1 | Seerr | `library:read` |
| `GET /movie/{id}` | V1 | Seerr availability sync | `library:read` |
| `GET /movie/lookup?term=tmdb:{id}` | V1 | Seerr, before every add | `library:read` |
| `POST /movie` | V1 | Seerr request | `library:add` |
| `PUT /movie` | V1 | Seerr re-monitor | `library:edit` |
| `DELETE /movie/{id}` | V1 | Seerr remove | `library:delete` |
| `POST /command` (`MoviesSearch`) | V1 | Seerr | `search:run` |
| `POST /command` (`RefreshMonitoredDownloads`) | V1 | Seerr download tracker | `queue:read` |
| `GET /queue` | V1 | Seerr download tracker | `queue:read` |
| `GET /health`, `/diskspace`, `/calendar`, `/wanted/missing` | Later | Homarr, Homepage | to define |
| `GET /history`, `/movie/lookup?term=<text>` | Later | Dashboards, other request tools | to define |
| `GET /customformat`, `PUT /qualityprofile/{id}` | Later, with custom formats | Recyclarr | to define |
| `/notification`, `/config/*`, `/indexer`, `/downloadclient` writes | Never | | |
| `/sonarr/api/v3/*` | M8 | Seerr | |

`POST /movie` with `addOptions.searchForMovie = true` also needs
`search:run`. Open question: one route, one permission is the rule; draft
answer is that the add succeeds and the search is skipped with a log line
when the key lacks `search:run`.

## Field mapping: Radarr MovieResource

| Radarr field | Unseen source |
|---|---|
| `id` | `movies.id` |
| `title`, `originalTitle`, `sortTitle`, `year`, `overview`, `runtime` | same columns |
| `tmdbId`, `imdbId` | `tmdb_id`, `imdb_id` |
| `titleSlug` | TMDB id as string |
| `path` | root folder path + `folder_name` |
| `rootFolderPath` | root folder path |
| `folderName` | same as `path` |
| `qualityProfileId` | `quality_profile_id` |
| `monitored`, `minimumAvailability` | same columns |
| `isAvailable` | computed from release dates and minimum availability |
| `hasFile`, `movieFile` | from `movie_files`; `movieFile.mediaInfo` defaults when unknown |
| `status` | `tba`, `announced`, `inCinemas`, `released` from TMDB dates |
| `added` | `added_at` |
| `tags` | tag ids |
| `images`, `ratings`, `genres`, `studio`, `certification`, `youTubeTrailerId`, `website`, `collection`, `popularity`, `statistics` | value if stored, otherwise the schema default (empty list, empty string, zero) |

`system/status` returns `appName: "Radarr"`, a fixed Radarr version string
that Seerr accepts (verify the minimum in Seerr's source), and `urlBase:
"/radarr"`.

Quality profile, root folder, tag and queue resources: full mapping tables
are written with their M6 issues.

## Known differences

- One Unseen serves what used to be several instances. A companion tool that
  needs "4K server" and "1080p server" entries points both at Unseen and
  chooses a different quality profile.
- Ids differ from the imported instances.
- No custom formats; related fields return empty lists and zero scores.
- Commands other than the two listed return 501.
- `queue` records carry no custom format data.

## Open questions

- Custom formats, TRaSH guides, Recyclarr.
- Merge rule when one movie exists in two imported instances.
- Radarr version reported in `system/status`.
- Bazarr settings import.
