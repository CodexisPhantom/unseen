# 12 Testing strategy

Status: draft.

## Layers

| Layer | What | Where |
|---|---|---|
| Unit | Parsing, scoring, naming templates, permission checks, state machines. Table-driven. | `crates/core` |
| Database | Queries and migrations against a temp SQLite file per test. Migrations run from zero on every run. | `crates/db` |
| Integration | The Axum app in-process, real SQLite, external services mocked with `wiremock`. | `crates/api/tests` |
| Route permission coverage | Walks the router: every route declares one permission or is public, and matches `x-permission` in OpenAPI. | `crates/api/tests` |
| Permission behaviour | For each route: 401 without credentials, 403 without the permission, success with it. Generated from the OpenAPI document. | `crates/api/tests` |
| Compat contract | Our responses validate against the upstream Radarr OpenAPI schema; recorded request/response pairs from a real Radarr are the fixtures. | `crates/arr-compat/tests` |
| Frontend unit | Stores and helpers with Vitest. | `web` |
| End to end | A few Playwright tests against the real binary: setup, login, add movie, see queue. | `web/e2e` |

## Fixtures

- `fixtures/tmdb`, `fixtures/torznab`, `fixtures/qbittorrent`: recorded
  responses, secrets removed.
- `fixtures/radarr`: recorded pairs per implemented endpoint, with the Radarr
  version in the file name.
- The release-name table in [07-release-scoring.md](07-release-scoring.md) is
  a data file read by the tests and by the doc check.

## Compat contract tests

1. Record: a script calls a real Radarr and stores the pairs.
2. Schema check: each of our responses validates against the pinned Radarr
   OpenAPI document. No field in the upstream schema is omitted.
3. Shape check: for the same request, our response has the same set of keys
   and types as the recorded one.
4. Client check: one test replays the exact request sequence that Seerr sends.

## CI

On every pull request:

1. `cargo fmt --check`, `cargo clippy -- -D warnings`
2. `cargo test --workspace`
3. `pnpm check`, `pnpm lint`, `pnpm test`, `pnpm build`
4. OpenAPI and generated client are up to date (`just gen` produces no diff)
5. Docker image builds

On `main`: also the Playwright tests and the image push.

## Not tested automatically

- Real indexers, real TMDB.
- A real qBittorrent, except one optional local test.
- Seerr end to end: a manual checklist in M6, automated later if it pays off.

## Open questions

- Coverage target, if any.
- Automate the Seerr test with containers in CI.
