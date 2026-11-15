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

Contains: movies, torrents (Torznab), qBittorrent, accounts and permissions,
adoption of an existing Radarr library, import from running Radarr instances,
the Radarr API subset that Seerr needs.

Leaves out: TV, Usenet, subtitles, custom formats, notifications, backup UI,
2FA and passkeys, Prowlarr import.

## Features by milestone

| Milestone | Features |
|---|---|
| M0 | Workspace, app shell, CI, Docker image, OpenAPI client, auth spike, docs |
| M1 | Owner setup, login, roles, users, API keys, permission on every route |
| M2 | Add movie from TMDB, root folders, quality profiles, adopt Radarr folder |
| M3 | Torznab indexers, manual search, scored results |
| M4 | qBittorrent handoff, live queue, import with Radarr naming |
| M5 | Scheduler, RSS sync, automatic search, upgrades, blocklist |
| M6 | Import from Radarr, `/radarr/api/v3` subset, Seerr verified |
| M7 | Shows, seasons, episodes, season packs |
| M8 | Adopt and import Sonarr, `/sonarr/api/v3` subset |
| M9 | Newznab, SABnzbd |
| M10 | Subtitles from OpenSubtitles |
| M11 | Notifications, backup/restore, 2FA/passkeys, Prowlarr import, user docs |

## Open questions

- Custom formats and TRaSH guides / Recyclarr support (after V1, not designed).
- Request and approval flow inside Unseen, or leave it to Seerr.
- Is TVDB needed for TV, or is TMDB enough?
