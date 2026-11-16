# 0004 OpenAPI for shared types

Status: accepted (2026-11-15)

## Context

Backend and frontend use different languages. Hand-written TypeScript types
drift from the Rust types. The required permission of each route must be
known to the docs and to the frontend.

## Decision

`utoipa` generates the OpenAPI document from the Rust code. The document is
committed as `openapi.json`. `openapi-typescript` generates the frontend
types from it. Each operation carries the vendor extension `x-permission`.
CI fails when the committed files differ from the generated ones.

## Consequences

- One source of truth for types, routes and permissions.
- Every handler needs a utoipa annotation. Amended 2026-11-16: routes are
  registered through `utoipa-axum`'s `OpenApiRouter`, so a route cannot exist
  without being in the document. The test that remains is the permission
  check of each route (M1).
- The compat APIs are not described by our document; they follow the
  upstream Radarr/Sonarr OpenAPI documents (ADR 0008).
- Generated files are committed, so pull requests show API changes.
