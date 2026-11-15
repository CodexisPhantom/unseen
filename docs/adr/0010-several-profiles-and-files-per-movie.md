# 0010 Several quality profiles and files per movie

Status: accepted (2026-11-15)

## Context

Radarr keeps one file per movie. People who want a 1080p and a 4K copy run
two Radarr instances. Unseen exists to remove that second instance, and the
import from two instances must not lose one of the two files.

## Decision

- A movie has one or more movie profiles: rows that attach a quality
  profile to the movie. Each movie profile is a separate wanted item with
  its own search, download, upgrade and at most one file.
- One movie profile is the primary one.
- Files on disk that belong to no profile are kept as unmanaged files.
- Editions are a property of a file, not a slot of their own.
- The Radarr-compatible API shows the primary profile and its file.

Schema: `movie_profiles`, and `movie_files.movie_profile_id`
([04-database.md](../04-database.md)).

## Alternatives

- One file per movie, as Radarr: does not replace the second instance.
- Free list of files per movie without profiles: no rule for which file an
  upgrade replaces.
- One movie row per version: breaks `tmdb_id` uniqueness and the compat API.

## Consequences

- Search, queue, import and upgrade work per movie profile, not per movie.
- Importing the same movie from two Radarr instances gives one movie with
  two profiles and two files.
- The compat API hides non-primary profiles. How a companion tool requests
  a second profile (Seerr's "4K server") is still to verify.
- Two files share one movie folder, so file names must differ; the default
  template includes the quality.
