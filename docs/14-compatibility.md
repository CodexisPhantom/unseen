# 14 Sonarr/Radarr compatibility

Status: reviewed (2026-11-16). Scope decision: ADR 0008.

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

V1: quality names and ids, quality definitions, quality profiles with
cutoff, upgrades, language and custom format scores, custom formats in
Radarr's JSON format, root folders, tags (also on indexers and download
clients), collections, monitored, minimum availability. Release parsing
agrees with Radarr on the table in
[07-release-scoring.md](07-release-scoring.md).

TRaSH guides: built in, V1 ([07-release-scoring.md](07-release-scoring.md)).
Recyclarr: after V1 (M11). It needs compat write endpoints for custom
formats, quality profiles and quality definitions.

## C. Import from running instances

Read-only against the source. Safe to re-run (`import_map` table).

| Radarr resource | Endpoint | Becomes in Unseen |
|---|---|---|
| Root folders | `GET /rootfolder` | Root folder with the same path (path mapping if mounts differ) |
| Quality profiles | `GET /qualityprofile` | Quality profile with language, custom format scores and score limits |
| Custom formats | `GET /customformat` | Custom format, matched by name; the JSON is stored unchanged |
| Quality definitions | `GET /qualitydefinition` | Size limits per quality; applied only when the user ticks it, because the table is global |
| Collections | `GET /collection` | Collection matched by `tmdbId`, with its monitored state |
| Tags | `GET /tag` | Tag, matched by label |
| Movies | `GET /movie` | Movie matched by `tmdbId`, with path, monitored, minimum availability, tags; existing file adopted in place |
| Indexers | `GET /indexer` | Torznab indexers only; secrets are masked upstream, so entries arrive disabled |
| Download clients | `GET /downloadclient` | qBittorrent only; password must be re-entered |

Several instances: each import source has a target quality profile. All
movies from that source get that profile, or keep their mapped profile if no
target is set.

A movie present in two instances is merged by `tmdbId`, and nothing is lost:

- The movie gets one movie profile per instance, each with the file that
  instance had. This is the 1080p-and-4K case that needed two instances.
- The first imported source provides the primary profile, the root folder,
  the folder name and the monitored state. The import page lets the user
  order the sources.
- Tags are the union. Minimum availability is the earliest of the two.
- If both instances map to the same quality profile, the movie gets one
  movie profile; the larger file becomes its file and the other is adopted
  as an unmanaged file.
- If the two instances keep the movie in different folders, the file of the
  second one stays where it is and is recorded with its real path. The
  report lists these movies.

Custom formats with the same name and different conditions in two instances:
the first source wins, the second is imported as `<name> (<source name>)`
and reported.

Later: Sonarr (M8), Prowlarr indexers through its v1 API (M11). Bazarr
settings: not in V1, decided in the subtitles design (M10).

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
| `GET/POST/PUT/DELETE /customformat`, `PUT /qualityprofile/{id}`, `GET/PUT /qualitydefinition` | M11 | Recyclarr | `profiles:read`, `profiles:manage` |
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

`system/status` returns `appName: "Radarr"`, `urlBase: "/radarr"` and a fixed
Radarr version: the version of the Radarr release whose OpenAPI document and
recorded responses are pinned for the contract tests. One constant in
`arr-compat`, raised together with the pinned schema. A client that reads
the version then sees exactly the API it was told about. Unseen's own
version is in `GET /api/v1/health`.

`qualityProfileId`, `hasFile` and `movieFile` describe the primary movie
profile. Other profiles and unmanaged files are not visible through the
compat API in V1.

Quality profile, root folder, tag and queue resources: full mapping tables
are written with their M6 issues.

## Known differences

- One Unseen serves what used to be several instances. A companion tool that
  needs "4K server" and "1080p server" entries points both at Unseen and
  chooses a different quality profile.
- Ids differ from the imported instances.
- A movie can have several profiles and files; the compat API shows the
  primary one.
- Custom formats exist, but the compat API does not expose them until M11;
  related fields return empty lists and zero scores until then.
- Commands other than the two listed return 501.
- `queue` records carry no custom format data until M11.

## Decisions (2026-11-15)

- Custom formats: V1. TRaSH guides: V1, built in. Recyclarr: after V1.
- Merge rule for a movie in two instances: both kept as movie profiles, see
  section C.
- `system/status` reports the pinned Radarr version.
- Bazarr settings import: not in V1.

## Open questions

- Seerr with a "4K server" entry that points at Unseen: what must `GET
  /movie?tmdbId=` and `POST /movie` answer so that a 4K request adds a second
  movie profile? Verify with Seerr in M6.
