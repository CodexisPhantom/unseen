# 10 Configuration reference

Status: draft.

Rule: what the process needs before the database is open lives in
environment variables. Everything else lives in the database and is edited
in the UI. There is no config file.

## Environment variables

| Variable | Default | Meaning |
|---|---|---|
| `UNSEEN_DATA_DIR` | `/config` | Holds `unseen.db`, `backups/`, `logs/` and the generated secret |
| `UNSEEN_BIND` | `0.0.0.0:7979` | Listen address |
| `UNSEEN_BASE_URL` | none | Public URL, for example `https://unseen.example.com`. Needed for secure cookies behind a proxy. |
| `UNSEEN_SECRET` | generated and stored in the data dir on first run | Root secret. The auth signing key and the key that encrypts integration secrets are derived from it. Without it the stored integration secrets cannot be read. |
| `UNSEEN_TRUSTED_PROXIES` | none | CIDRs whose `X-Forwarded-*` headers are trusted |
| `UNSEEN_LOG_LEVEL` | `info` | `error`, `warn`, `info`, `debug`, `trace` |
| `UNSEEN_LOG_FORMAT` | `text` | `text` or `json` |
| `PUID`, `PGID` | `1000` | User and group of the process in Docker |
| `UMASK` | `022` | File creation mask |
| `TZ` | `UTC` | Time zone for logs and schedules |

## Database settings (table `settings`)

| Key | Default | Permission to change |
|---|---|---|
| `tmdb.api_key` | none (secret) | `settings:manage` |
| `tmdb.language` | `en-US` | `settings:manage` |
| `naming.movie_folder` | `{Movie Title} ({Release Year})` | `settings:manage` |
| `naming.movie_file` | `{Movie Title} ({Release Year}) {Quality Full}` | `settings:manage` |
| `naming.rename_movies` | `false` | `settings:manage` |
| `naming.replace_illegal_characters` | `true` | `settings:manage` |
| `import.use_hardlinks` | `true` | `settings:manage` |
| `import.skip_free_space_check` | `false` | `settings:manage` |
| `import.extra_file_extensions` | `srt,ass,ssa,sub,idx` | `settings:manage` |
| `proxy.url` | none (secret when it holds credentials) | `settings:manage` |
| `proxy.bypass_local` | `true` | `settings:manage` |
| `trash.sync_interval_hours` | `24` | `settings:manage` |
| `logs.retention_days` | `14` | `settings:manage` |
| `search.minimum_seeders` | `1` | `settings:manage` |
| `downloads.stalled_timeout_minutes` | `360`; `0` is off | `settings:manage` |
| `tasks.rss_sync_interval_minutes` | `30` | `settings:manage` |
| `tasks.missing_search_interval_hours` | `24` | `settings:manage` |
| `tasks.metadata_refresh_interval_hours` | `24` | `settings:manage` |
| `auth.session_days` | `30` | `settings:manage` |

Stored in their own tables, not in `settings`: root folders, quality
profiles, quality definitions, custom formats, indexers, download clients
(including "remove completed downloads"), import sources, users, roles, API
keys.

## Decisions (2026-11-15)

- Port 7979. It continues the family pattern (Radarr 7878, Sonarr 8989,
  Prowlarr 9696, Bazarr 6767) and none of those apps uses it, so Unseen can
  run next to them during a migration.
- No config file. Root folders, profiles, indexers and clients live in
  tables with relations and secrets; a file would be a second source of
  truth for them and would need its own merge rules. No real advantage.
- Environment overrides for database settings: not in V1. Planned for M11:
  a setting given in the environment wins and is read-only in the UI.
