# 10 Configuration reference

Status: reviewed (2026-11-16).

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

`PUID`, `PGID` and `UMASK` are read by the entrypoint of the Docker image,
not by the binary. The image's `HEALTHCHECK` calls port 7979: keep the port
of `UNSEEN_BIND` in a container and change the published port instead.

State after the auth mount (#30): the binary reads `UNSEEN_DATA_DIR`,
`UNSEEN_BIND`, `UNSEEN_SECRET` and `UNSEEN_BASE_URL`; the image handles
`PUID`, `PGID`, `UMASK` and `TZ`. The generated secret is the file `secret`
in the data directory. Each other variable is read from the issue that needs
it: `UNSEEN_TRUSTED_PROXIES` with the first use of the
client address (M1), `UNSEEN_LOG_LEVEL` and `UNSEEN_LOG_FORMAT` with the log
files (M5). Until then the log level is `info` and the format is text.

## Database settings (table `settings`)

Location: one row per key in the table `settings`, edited in the UI under
Settings. A missing row means the default below.

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

### Environment overrides

`UNSEEN_SETTING_<KEY>` sets a key of the `settings` table from the
environment. `<KEY>` is the key in upper case with `.` written as `_`:
`tmdb.api_key` is `UNSEEN_SETTING_TMDB_API_KEY`, `tmdb.language` is
`UNSEEN_SETTING_TMDB_LANGUAGE`. Variables are read once at startup; an empty
value does not override.

- The environment value wins over the table. The table is never written for
  an overridden key, and a secret in the environment is not stored.
- A request that changes an overridden setting is refused with 409
  `setting_overridden`, naming the variable.
- The settings response lists the overridden fields in `overridden`, so a UI
  can show them as read-only.
- Only settings, not tables: indexers, clients, profiles and the rest cannot
  be declared in the environment.

## Decisions (2026-11-15)

- Port 7979. It continues the family pattern (Radarr 7878, Sonarr 8989,
  Prowlarr 9696, Bazarr 6767) and none of those apps uses it, so Unseen can
  run next to them during a migration.
- No config file. Root folders, profiles, indexers and clients live in
  tables with relations and secrets; a file would be a second source of
  truth for them and would need its own merge rules. No real advantage.
- Environment overrides for database settings: a setting given as
  `UNSEEN_SETTING_<KEY>` wins and is read-only (see above).
