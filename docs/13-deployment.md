# 13 Deployment and operations

Status: draft. Image name and port are placeholders.

## docker-compose example

```yaml
services:
  unseen:
    image: ghcr.io/codexisphantom/unseen:latest
    environment:
      - PUID=1000
      - PGID=1000
      - TZ=Europe/Paris
      # - UNSEEN_BASE_URL=https://unseen.example.com
    volumes:
      - ./config/unseen:/config
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
      - "8080:8080"   # qBittorrent WebUI, reached by Unseen
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

## Volumes

| Mount | Contents |
|---|---|
| `/config` | `unseen.db`, backups, logs, generated secret |
| `/data` | Downloads and library under one mount, so hardlinks work ([09-filesystem.md](09-filesystem.md)) |

## Upgrades and migrations

- Pull the new image and restart. Migrations run at startup.
- Before a migration runs, Unseen copies the database to
  `/config/backups/pre-migration-<version>.db`.
- Downgrades are not supported. Restore the pre-migration backup instead.
- If a migration fails, the process exits with a clear error and does not serve.

## Logging

Structured logs on stdout (`UNSEEN_LOG_FORMAT`). Secrets are never logged.
Unimplemented compat endpoints log at `warn` with method and path.
`system:logs` gives access to recent logs in the UI (open question: file or
in-memory ring buffer).

## Health

`GET /api/v1/health` returns 200 with version and database status. Public.
The Docker image declares it as `HEALTHCHECK`.

## Backup and restore

- Manual: stop the container, copy `/config`.
- Online: `system:backup` triggers a consistent SQLite backup (`VACUUM INTO`).
  UI and schedule come in M11.
- Restore: stop, replace `unseen.db`, start.

## Running next to an existing Radarr, then switching

1. Start Unseen with the same `/data` mount as Radarr.
2. Run "Import from Radarr". Unseen reads Radarr's API and adopts the files
   in place. Radarr is not modified.
3. Keep automation off in Unseen (tasks disabled) while both run, so that
   only one app grabs releases.
4. Give Unseen its own qBittorrent category, or reuse Radarr's when Radarr is
   stopped.
5. Compare libraries. Then stop Radarr, enable tasks in Unseen, and point
   Seerr to Unseen with URL base `/radarr`.
6. Roll back: stop Unseen, start Radarr. Files were never moved.

## Open questions

- Port number and image registry.
- Non-Docker installs: plain binary, systemd unit.
- Log access for `system:logs`.
