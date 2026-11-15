# 01 Product brief

Status: draft.

## Problem

A typical media server runs Sonarr, Radarr, Prowlarr and Bazarr, often with
two instances of Sonarr and Radarr for 1080p and 4K. That is six services with
separate settings, databases, updates and accounts, and none of them has real
multi-user permissions.

## Goals

- One app replaces Sonarr, Radarr, Prowlarr and Bazarr.
- A quality profile on each item replaces multiple instances.
- Existing Sonarr/Radarr users can switch without moving files or
  reconfiguring download clients and companion tools.
- Several accounts with roles the Owner defines.
- One binary, one SQLite file, one Docker image.

## Non-goals (never)

- Downloading: no built-in torrent or Usenet client.
- VPN management.
- Media playback or transcoding.
- A 100% drop-in Sonarr/Radarr v3 API.
- Writing back to Sonarr/Radarr, or reading their SQLite databases.
- Running Sonarr/Radarr plugins or custom scripts.
- Music, books or other media types.
- Public sign-up or multi-tenant hosting.

## Target user

One household. The Owner runs the server. A few family members or friends
have accounts with limited roles, for example "can add movies, cannot change
settings".

## V1

Contains: movies, collections, torrents (Torznab), qBittorrent, accounts and
permissions, an audit log, custom formats, a built-in TRaSH guides browser,
adoption of an existing Radarr library, import from running Radarr instances,
the Radarr API subset that Seerr needs.

Leaves out: TV, Usenet, subtitles, notifications, backup UI, 2FA and
passkeys, Prowlarr import, Recyclarr support, a request and approval flow,
per-library access scoping, a recycle bin, sub-path hosting.

## Features by milestone

| Milestone | Features |
|---|---|
| M0 | Workspace, app shell, CI, Docker image, OpenAPI client, auth spike, docs |
| M1 | Owner setup, login, roles, per-user grants, users, API keys, permission on every route, audit log, ownership transfer |
| M2 | Add movie from TMDB, several profiles and files per movie, collections, root folders, quality profiles, encrypted secrets, adopt Radarr folder |
| M3 | Torznab indexers, proxy, quality size limits, custom formats, manual search, scored results |
| M4 | qBittorrent handoff, live queue, import with Radarr naming, sidecar subtitles, removal after seeding |
| M5 | Scheduler, RSS sync, automatic search, upgrades, blocklist, TRaSH guides, log viewer |
| M6 | Import from Radarr, `/radarr/api/v3` subset, Seerr verified |
| M7 | Shows, seasons, episodes, season packs, TVDB as a second metadata provider |
| M8 | Adopt and import Sonarr, `/sonarr/api/v3` subset |
| M9 | Newznab, SABnzbd |
| M10 | Subtitles from OpenSubtitles |
| M11 | Notifications, backup/restore, 2FA/passkeys, Prowlarr import, Recyclarr support, library scoping, request flow design, recycle bin, sub-path hosting, user docs |

## Decisions (2026-11-15)

- Custom formats are part of V1 ([07-release-scoring.md](07-release-scoring.md)).
- TRaSH guides are part of V1 as a built-in browser: search the guide, read
  what each custom format and profile does, preview the changes, apply them,
  and keep them in sync. The goal is that the guides are easier to find and
  use than with Radarr plus Recyclarr. Recyclarr itself (the compat write
  endpoints it needs) comes after V1.
- Requests and approvals stay in Seerr for now. A flow inside Unseen is wanted
  later; it gets a design issue in M11.
- Metadata comes from pluggable providers, not from one fixed source. V1 ships
  the TMDB provider behind a provider trait. TVDB is added with TV in M7.
  TMDB, IMDb and TVDB ids keep their own columns because the compat API
  needs them. Ids from further providers go in a child table added with that
  provider.

## Open questions

- Which metadata providers after TMDB and TVDB (IMDb datasets, Trakt, AniDB)?
