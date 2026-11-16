# 03 Domain model and glossary

Status: reviewed (2026-11-16). Terms match Sonarr/Radarr unless the "Differs" column says so.

## Entities

| Term | Meaning | Differs from Sonarr/Radarr |
|---|---|---|
| Movie | A film identified by its TMDB id. Has a root folder, one or more quality profiles, tags, monitored state and minimum availability. | Radarr: exactly one profile |
| Movie profile | One quality profile attached to a movie. Holds at most one file. One of them is the primary profile. | New. Replaces a second Radarr instance |
| Collection | A TMDB collection (for example a film series). Can be monitored: its missing movies are added automatically. | No |
| Metadata provider | A source of titles, dates, images and ids. TMDB in V1, TVDB in M7. | Upstream has one fixed source |
| Show, season, episode | TV entities (M7). | Radarr/Sonarr say "series"; the compat API keeps "series" |
| Root folder | A directory that contains one folder per movie or show. | No |
| Quality | A named source and resolution pair, for example `Bluray-1080p`. | No |
| Quality profile | Allowed qualities in order, a cutoff, a language, custom format scores, and whether upgrades are allowed. | Also replaces separate instances |
| Quality definition | Minimum, preferred and maximum size per minute for one quality. | No |
| Custom format | A named set of conditions on a release (name pattern, source, resolution, language, size and others). A quality profile gives each custom format a score. | No |
| TRaSH guide | The community guide that publishes custom formats, score sets and quality sizes. Unseen reads its published JSON. | Upstream needs Recyclarr or manual import |
| Tag | A label on items, indexers and download clients. | No |
| Monitored | Unseen searches for the item. | No |
| Minimum availability | Earliest release stage at which to search: `announced`, `inCinemas`, `released`. | No |
| Wanted item | A monitored movie profile that has no file or whose file is below cutoff. | No separate name upstream |
| Indexer | A Torznab or Newznab endpoint. | Managed in Unseen, not in Prowlarr |
| Release | One search result from an indexer: a name, a size, a download link. Nothing is downloaded yet. | No |
| Grab | The act of sending a release to a download client. | No |
| Download | A release that a download client is handling. Has progress and a state. | Upstream calls this a "queue item" |
| File | A media file inside the library, linked to a movie or episode. A movie can have several: one per movie profile, plus unmanaged files found on disk. | Upstream: "movie file", one per movie |
| Import | Hardlink, copy or rename of a finished download into the library. | No |
| Adopt | Register existing library files without moving them. | Upstream: "library import" |
| Instance import | One-way copy of settings and items from a running Radarr/Sonarr. | New |
| Download client | qBittorrent or SABnzbd. | No |
| Blocklist | Releases that failed and must not be grabbed again. | No |
| User | An account. | Upstream has one user |
| Owner | Built-in role with every permission. | New |
| Role | A named set of permissions, created at runtime. | New |
| Permission | A fixed `resource:action` string defined in code. | New |
| Direct grant | A permission given to one user without a role. | New |
| Audit log | A record of who changed what: sign-ins, users, roles, keys, settings, integrations, deletions. | New |
| API key | A secret that belongs to a user, limited to a subset of that user's permissions. | Upstream has one global key |

## Release, download, file

- A **release** exists on an indexer. Unseen stores it only when grabbed.
- A **download** exists in a download client. It ends when imported, failed or removed.
- A **file** exists in the library. It outlives its download.

## Relationships

```mermaid
erDiagram
  ROOT_FOLDER ||--o{ MOVIE : contains
  COLLECTION ||--o{ MOVIE : groups
  MOVIE ||--|{ MOVIE_PROFILE : has
  QUALITY_PROFILE ||--o{ MOVIE_PROFILE : "applies to"
  MOVIE_PROFILE ||--o| MOVIE_FILE : holds
  MOVIE ||--o{ MOVIE_FILE : has
  QUALITY_PROFILE ||--o{ CUSTOM_FORMAT_SCORE : scores
  CUSTOM_FORMAT ||--o{ CUSTOM_FORMAT_SCORE : ""
  MOVIE ||--o{ DOWNLOAD : "is target of"
  INDEXER ||--o{ DOWNLOAD : "is source of"
  DOWNLOAD_CLIENT ||--o{ DOWNLOAD : handles
  USER }o--o{ ROLE : has
  ROLE ||--o{ PERMISSION : grants
  USER ||--o{ API_KEY : owns
  USER ||--o{ DIRECT_GRANT : has
```

## Several files per movie

Decided 2026-11-15, see ADR 0010.

- A movie has one or more movie profiles, for example `HD-1080p` and
  `Ultra-HD`. Each movie profile is searched, downloaded and upgraded on its
  own and holds at most one file.
- The first profile is the primary profile. The Radarr-compatible API shows
  the primary profile and its file, because Radarr's resource has room for
  one.
- Files found on disk that belong to no profile (a second cut, an old copy)
  are kept as unmanaged files. Unseen lists them and never upgrades or
  deletes them on its own.
- Editions are a property of a file, not a separate slot. To keep two
  editions on purpose, use two profiles or leave one unmanaged.

## Decisions (2026-11-15)

- One movie can have several files: yes, through movie profiles.
- Collections are an entity in V1.

## Open questions

- How does the compat API treat a request from Seerr for a movie that exists
  with another profile (the "4K server" case)? Draft: `POST /movie` with a
  new `qualityProfileId` adds a movie profile. To verify with Seerr in M6.
