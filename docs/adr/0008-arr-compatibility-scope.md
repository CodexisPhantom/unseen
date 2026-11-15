# 0008 Sonarr/Radarr compatibility scope and URL bases

Status: accepted (2026-11-15)

## Context

People who run Sonarr and Radarr have libraries on disk, download client
setups and companion tools such as Seerr. Unseen is only useful to them if
those keep working. A full reimplementation of the v3 APIs is not realistic.

Radarr and Sonarr both serve `/api/v3` with overlapping paths
(`system/status`, `qualityprofile`, `rootfolder`, `tag`, `command`, `queue`).
One app cannot serve both at the same prefix.

## Decision

- Compatibility is a design constraint from the first milestone, defined as
  four layers: on disk, concepts and names, one-way import from running
  instances, and an API subset for companion tools
  ([14-compatibility.md](../14-compatibility.md)).
- The compat APIs live at `/radarr/api/v3` and `/sonarr/api/v3`. External
  tools use the URL base `/radarr` or `/sonarr`.
- The API subset is driven by named clients. V1: the endpoints Seerr's
  Radarr client calls, taken from its source code.
- Response shapes follow the upstream OpenAPI documents. Missing values are
  returned as defaults, never omitted. Unimplemented endpoints return 501
  with a JSON body.
- Auth with `X-Api-Key` or `?apikey=`, backed by Unseen API keys and their
  permissions.
- Contract tests compare our responses with recorded responses from a real
  instance and with the upstream schema.
- Ids of core entities are integers, and TMDB, IMDb and TVDB ids are stored
  on every item, because the compat layer needs them.

Not goals: 100% of the v3 API, writing back to Sonarr/Radarr, reading their
SQLite databases, running their plugins or custom scripts.

## Consequences

- Seerr works without changes on its side.
- Tools that hardcode a root `/api/v3` without a URL base setting cannot be
  supported.
- Upstream API changes need attention; the pinned upstream schema makes them
  visible in CI.
- Amended 2026-11-15: custom formats and a built-in TRaSH guide browser are
  in V1. They use Radarr's custom format JSON unchanged. Recyclarr needs
  compat write endpoints and comes after V1.
- Amended 2026-11-15: a movie can have several quality profiles and files
  (ADR 0010). The compat API shows the primary one.
- DTOs and mapping live in `crates/arr-compat`, away from the core types.
