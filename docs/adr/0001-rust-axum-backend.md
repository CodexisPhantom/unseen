# 0001 Rust + Axum backend

Status: accepted (2026-11-15)

## Context

Unseen runs all day on small home servers, next to other containers. It does
I/O-bound work: HTTP calls, polling, file operations, a small database.
Sonarr and Radarr use .NET and need a few hundred MB of memory each.

## Decision

Rust with Axum 0.8 and Tokio. A Cargo workspace with the crates `api`,
`core`, `db` and `arr-compat`.

## Consequences

- One static binary, low memory use, small Docker image.
- Background tasks are Tokio tasks in the same process; no separate worker.
- Compile times are long and the contributor pool is smaller than for
  TypeScript or C#.
- Axum 0.8 is also what the authentication library supports (ADR 0006).
