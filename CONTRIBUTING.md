# Contributing

The commands below are the target workflow. They exist once the M0 issues are
done.

## Requirements

- Rust stable (edition 2024), `just`
- Node 22+, pnpm
- OpenSSL 3 and ICU 78 development files. `rustls` does not remove the
  OpenSSL requirement (ADR 0006). Set `RUST_ICU_MAJOR_VERSION_NUMBER=78`. On
  Windows also set `OPENSSL_DIR` and `RUST_ICU_LINK_SEARCH_DIR`, and put both
  `bin` folders on `PATH`.

## Commands

| Command | Does |
|---|---|
| `just dev` | Runs the API with reload and the Vite dev server |
| `just check` | `cargo fmt --check`, `cargo clippy -- -D warnings`, `pnpm check`, `pnpm lint` |
| `just coverage` | Rust tests with `cargo llvm-cov`; fails below the minimums in docs/12 |
| `just test` | Rust tests and frontend tests |
| `just gen` | Regenerates `openapi.json` and the TypeScript client |
| `just gen-auth` | Regenerates `crates/db/src/auth_schema.rs` with the Better Auth CLI |
| `just build` | Builds the frontend, then the release binary with the UI embedded |

CI fails when `just gen` changes a committed file.

## Add a shadcn-svelte component

```
cd web
pnpm dlx shadcn-svelte@latest add <name>
```

The component is copied into `web/src/lib/components/ui/`. Commit it. Do not
add a second component library. Build missing components on Bits UI in the
same style. See [docs/16-frontend.md](docs/16-frontend.md).

## Add a route

1. Write the handler in `crates/api` with its utoipa annotation.
2. Declare exactly one required permission, or mark the route public.
3. Run `just gen` and commit the generated files.
4. The route coverage test fails if step 2 is missing.

## Add a permission

1. Add the variant to the `Permission` enum in `crates/core`.
2. Add its line to the catalog in [docs/15-permissions.md](docs/15-permissions.md).
3. Use it on at least one route.
4. Run `just gen`. The frontend permission type updates from the OpenAPI spec.

The Owner role gets new permissions automatically. Custom roles do not.

## Change the database

Add a new versioned migration in `crates/db`. Never edit a released migration.
Auth tables change only through our own migrations. See
[docs/04-database.md](docs/04-database.md).

## Issues and pull requests

- One type label, one or more area labels, one size label, one milestone.
- Compatibility behaviour changes need an update to
  [docs/14-compatibility.md](docs/14-compatibility.md).
