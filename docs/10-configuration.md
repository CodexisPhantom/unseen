# 10 Configuration reference

Status: draft.

Rule: what the process needs before the database is open lives in
environment variables. Everything else lives in the database and is edited
in the UI. There is no config file in V1 (open question).

## Environment variables

| Variable | Default | Meaning |
|---|---|---|
| `UNSEEN_DATA_DIR` | `/config` | Holds `unseen.db`, backups and logs |
| `UNSEEN_BIND` | `0.0.0.0:7979` | Listen address. Port number is an open question. |
| `UNSEEN_BASE_URL` | none | Public URL, for example `https://unseen.example.com`. Needed for secure cookies behind a proxy. |
| `UNSEEN_SECRET` | generated and stored in the data dir on first run | Auth secret for signing |
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
| `search.minimum_seeders` | `1` | `settings:manage` |
| `downloads.stalled_timeout_minutes` | `0` (off) | `settings:manage` |
| `downloads.remove_completed` | `false` | `settings:manage` |
| `tasks.rss_sync_interval_minutes` | `30` | `settings:manage` |
| `tasks.missing_search_interval_hours` | `24` | `settings:manage` |
| `tasks.metadata_refresh_interval_hours` | `24` | `settings:manage` |
| `auth.session_days` | `30` | `settings:manage` |

Stored in their own tables, not in `settings`: root folders, quality
profiles, indexers, download clients, import sources, users, roles, API keys.

## Open questions

- Port number.
- Optional config file for declarative setups.
- Environment overrides for database settings (useful for automated installs).
