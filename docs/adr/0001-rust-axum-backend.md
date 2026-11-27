# 0001 Rust + Axum backend

Status: accepted (2026-11-15)

## Context

Unseenarr runs all day on small home servers, next to other containers. It does
I/O-bound work: HTTP calls, polling, file operations, a small database.
Sonarr and Radarr use .NET and need a few hundred MB of memory each.

## Decision

Rust with Axum 0.8 and Tokio. A Cargo workspace with the crates `api`,
`core`, `db` and `arr-compat`.

## Consequences

- One binary, low memory use. Amended 2026-11-16: the binary is not static
  and the image is not small. It links OpenSSL 3 and ICU dynamically
  (ADR 0006), and the image, built on Ubuntu 26.04 for these libraries, is
  about 180 MB.
- Background tasks are Tokio tasks in the same process; no separate worker.
- Compile times are long and the contributor pool is smaller than for
  TypeScript or C#.
- Axum 0.8 is also what the authentication library supports (ADR 0006).
