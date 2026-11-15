# 0003 SvelteKit static build embedded in the binary

Status: accepted (2026-11-15)

## Context

The result must be one binary on one port. A Node server in production would
be a second process and a second runtime in the image.

## Decision

SvelteKit 2 with Svelte 5 and `adapter-static` with an SPA fallback. The
build output is embedded in the Rust binary with `rust-embed` and served by
Axum. Unknown non-API paths return `index.html`.

## Consequences

- No Node in production.
- No server `load` functions, form actions, `+server.ts` routes or
  server hooks. All data goes through the generated API client.
- No server-side rendering. The first paint waits for JavaScript; acceptable
  for an authenticated app.
- The frontend must be built before the release binary. `just build` and the
  Dockerfile handle the order.
- In development, Vite serves the UI and proxies `/api` to the Rust process.
