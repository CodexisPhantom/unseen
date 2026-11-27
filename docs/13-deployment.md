# 13 Deployment and operations

Status: reviewed (2026-11-16).

## docker-compose example

```yaml
services:
  unseenarr:
    image: ghcr.io/codexisphantom/unseenarr:latest
    environment:
      - PUID=1000
      - PGID=1000
      - TZ=Europe/Paris
      # - UNSEENARR_BASE_URL=https://unseenarr.example.com
      # Sub-path hosting: - UNSEENARR_BASE_URL=https://unseenarr.example.com/unseenarr
    volumes:
      - ./config/unseenarr:/config
      - /data:/data
    ports:
      - "7979:7979"
    restart: unless-stopped

  gluetun:
    image: qmcgaw/gluetun
    cap_add: [NET_ADMIN]
    environment:
      - VPN_SERVICE_PROVIDER=...
    ports:
      - "8080:8080"   # qBittorrent WebUI, reached by Unseenarr
    restart: unless-stopped

  qbittorrent:
    image: lscr.io/linuxserver/qbittorrent
    network_mode: "service:gluetun"
    environment:
      - PUID=1000
      - PGID=1000
      - WEBUI_PORT=8080
    volumes:
      - ./config/qbittorrent:/config
      - /data/torrents:/data/torrents
    restart: unless-stopped

  sabnzbd:            # used from M9
    image: lscr.io/linuxserver/sabnzbd
    volumes:
      - ./config/sabnzbd:/config
      - /data/usenet:/data/usenet
    restart: unless-stopped
```

## Images

Published to two registries with the same tags (`latest`, `x.y.z`, `x.y`):

- GitHub Container Registry: `ghcr.io/codexisphantom/unseenarr`
- Docker Hub: `codexisphantom/unseenarr`

The image must contain the OpenSSL 3 and ICU 78 shared libraries: the
binary links both (ADR 0006).

The `Dockerfile` has three stages: frontend build (Node), binary build and
runtime (both Ubuntu 26.04, which ships these libraries). The image is about
180 MB. The entrypoint gives `/config` to `PUID:PGID`, applies `UMASK` and
starts `unseenarr` as that user. `docker-compose.example.yml` in the repository
root is the example above; use `build: .` in place of `image` to build from
source.

Docker is the only supported install for now. No plain binary download and
no systemd unit yet.

## Volumes

| Mount | Contents |
|---|---|
| `/config` | `unseenarr.db`, backups, logs, generated secret |
| `/data` | Downloads and library under one mount, so hardlinks work ([09-filesystem.md](09-filesystem.md)) |

## Upgrades and migrations

- Pull the new image and restart. Migrations run at startup.
- Before a migration runs, Unseenarr copies the database to
  `/config/backups/pre-migration-<version>.db`.
- Downgrades are not supported. Restore the pre-migration backup instead.
- If a migration fails, the process exits with a clear error and does not serve.

## Logging

Structured logs on stdout (`UNSEENARR_LOG_FORMAT`). Secrets are never logged.
Unimplemented compat endpoints log at `warn` with method and path.
Logs are also written to daily files in `/config/logs`, kept for
`logs.retention_days` (default 14). `system:logs` gives access to them in the
UI: `GET /api/v1/logs` returns the most recent lines with a level filter and
a text filter, and the page can download a file for a bug report. Files are
used instead of a memory buffer so that the lines before a crash or restart
are still there.

## Health

`GET /api/v1/health` returns 200 with version and database status. Public.
The Docker image declares it as `HEALTHCHECK`.

## Backup and restore

- Manual: stop the container, copy `/config`.
- Online: `system:backup` triggers a consistent SQLite backup (`VACUUM INTO`).
  UI and schedule come in M11.
- Restore: stop, replace `unseenarr.db`, start.

## Running next to an existing Radarr, then switching

1. Start Unseenarr with the same `/data` mount as Radarr.
2. Run "Import from Radarr". Unseenarr reads Radarr's API and adopts the files
   in place. Radarr is not modified.
3. Keep automation off in Unseenarr (tasks disabled) while both run, so that
   only one app grabs releases.
4. Give Unseenarr its own qBittorrent category, or reuse Radarr's when Radarr is
   stopped.
5. Compare libraries. Then stop Radarr, enable tasks in Unseenarr, and point
   Seerr to Unseenarr with URL base `/radarr`.
6. Roll back: stop Unseenarr, start Radarr. Files were never moved.

## Decisions (2026-11-15)

- Port 7979 ([10-configuration.md](10-configuration.md)).
- Registries: Docker Hub and GitHub Container Registry.
- Non-Docker installs: not for now.
- Log access through `system:logs`: yes, in V1.

## Open questions

- Multi-arch images (amd64 and arm64) from the first release, or later.
