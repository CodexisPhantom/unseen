# 16 Frontend conventions

Status: reviewed (2026-11-16). Decision: ADR 0003, ADR 0009.

## Stack

SvelteKit 3, Svelte 5 with runes, TypeScript, `adapter-static` with an SPA
fallback (`fallback: 'index.html'`, `ssr = false`, `prerender = false` in the
root layout). Tailwind CSS v4 through `@tailwindcss/vite`, configured in CSS
(`@import "tailwindcss"`, `@theme`). No `tailwind.config.js`.

Created with (2026-11-16: `sv` scaffolds SvelteKit 3, shadcn-svelte 1.8.1):

```
pnpm dlx sv create web --template minimal --types ts --add tailwindcss="plugins:none" --install pnpm
cd web
pnpm dlx shadcn-svelte@latest init --preset vega --base-color neutral --css src/routes/layout.css
```

What SvelteKit 3 changes against older guides:

- The SvelteKit options are in `vite.config.ts`. There is no
  `svelte.config.js`.
- The library alias is `#lib` (package.json `imports`), not `$lib`. Import a
  component folder by its index file:
  `import { Button } from '#lib/components/ui/button/index.js'`.
- The global CSS file is `src/routes/layout.css`.
- Internal links go through `resolve` from `$app/paths`, with a path that
  has no leading slash: `resolve('library')`, `resolve('')` for the root. A
  string that starts with `/` is read as a route id. ESLint
  (`svelte/no-navigation-without-resolve`) fails on a plain `href`. This also
  prepares sub-path hosting (M11).

## Forbidden because of adapter-static

- `+page.server.ts`, `+layout.server.ts`, server `load` functions
- Form actions
- `+server.ts` routes
- `hooks.server.ts`
- Any import from `$env/*/private`

All data goes through the generated API client. `web/scripts/check-static.mjs`
runs in `bun run check` (and so in `just check`) and fails on the file names
above.

## Routes

```
src/routes/
  +layout.svelte            ModeWatcher, QueryClientProvider, global CSS
  setup/                    first-run Owner creation
  login/
  (app)/                    authenticated shell: sidebar, header
    +layout.ts              loads /me, redirects to /login or /setup
    library/                list, movie detail, collections
    add/                    search TMDB and add
    activity/               queue
    settings/               general, root folders, profiles, quality sizes,
                            custom formats, TRaSH guides, indexers, clients
    users/  roles/  api-keys/
    import/                 adopt folder, import from Radarr
    system/                 status, tasks, logs, audit log
    [...rest]/              placeholder for pages that are not built yet;
                            becomes the "not found" page
```

## API client

- `openapi-typescript` generates `src/lib/api/schema.d.ts` from the committed
  `openapi.json`. `openapi-fetch` is the runtime.
  `just gen` regenerates both files; they are committed.
- In development Vite proxies `/api` to the API on `localhost:7979`.
- One client instance in `src/lib/api/client.ts`. Components never call
  `fetch` directly.
- Errors are problem details ([05-api.md](05-api.md)); a helper turns `code`
  into a message or a form error.
- A 401 response sends the user to `/login`.

- Server data is read and cached with TanStack Query
  (`@tanstack/svelte-query` 6, which uses runes: `createQuery(() => ({ ... }))`
  returns a reactive object). Query functions call the API client. Mutations
  invalidate by query key.

## Auth

The official `better-auth` Svelte client, pinned to the version Better Auth RS
targets (`1.7.6`), pointed at `/api/auth`. Used for sign-in and sign-out only.

## State with runes

- Shared state lives in `*.svelte.ts` modules that export a class or object
  using `$state` and `$derived`.
- `session.svelte.ts`: user, roles, permissions from `/me`.
- `events.svelte.ts`: one `EventSource` on `/api/v1/events`, opened in the
  `(app)` layout, closed on logout. Handlers invalidate or patch the
  matching queries. A reconnect invalidates everything.
- Runes modules hold client state only (session, UI state). Server data
  lives in the query cache, not in hand-written stores.
- No Svelte 4 stores in new code.

## Permission-based hiding

```ts
session.can('library:add')
```

- The `Permission` type is generated from the OpenAPI document.
- Navigation entries and buttons are hidden when `can()` is false.
- A route the user may not see shows a "no access" page.
- On `permissions.changed` (SSE) the session reloads `/me`.
- This is presentation only. The server enforces.

## Components

shadcn-svelte only, copied into `src/lib/components/ui/` and committed.
Anything missing is built on Bits UI in the same style.

Planned set: sidebar, button, input, label, select, checkbox, switch, dialog,
sheet, dropdown-menu, table, tabs, badge, card, alert, sonner, skeleton,
progress, tooltip, form, field, separator, avatar, breadcrumb.

- Tables: the shadcn-svelte data table with `@tanstack/svelte-table`
  (TanStack Table v9).
- Forms: the shadcn-svelte form components (formsnap, sveltekit-superforms,
  zod) in client-only mode: `superForm(defaults(zod4(schema)), { SPA: true })`,
  submit through the API client in `onUpdate`.
- `Field` is used for the layout of controls that are not part of a
  validated form (filters, single switches that save at once).

## Checks

| Command | Does |
|---|---|
| `bun run check` | `scripts/check-static.mjs`, then `svelte-check` |
| `bun run lint` | ESLint with `typescript-eslint` and `eslint-plugin-svelte` |
| `bun run test` | Vitest with coverage; the minimum is in [12-testing.md](12-testing.md) |

Unit tests are `*.test.ts` files next to the module they test. Vendored
shadcn-svelte code is linted but not counted for coverage.

## Theming

- shadcn-svelte CSS variables in the global CSS file (`--background`,
  `--foreground`, `--primary`, `--muted`, `--destructive`, `--border`,
  `--ring`, `--sidebar-*`), mapped in `@theme inline`.
- Light and dark through `mode-watcher`; the toggle offers light, dark and
  system.
- Use tokens (`bg-background`, `text-muted-foreground`), never raw colours.

## Decisions (2026-11-15)

Confirmed in the review of 2026-11-16. `openapi-fetch` and TanStack Query 6
are in use since the health page.

- `openapi-fetch`. It is a few kilobytes, fully typed from the generated
  schema, and a hand-written wrapper would repeat it.
- Forms: the Form components (formsnap and superforms in SPA mode) for every
  validated form, because they give validation, server error mapping and
  dirty state without own code. `Field` for layout outside validated forms.
- Data fetching: TanStack Query. Caching, request deduplication and
  invalidation from SSE events would otherwise be written by hand.
- Sub-path hosting: not in V1. SvelteKit fixes the base path at build time
  and the UI is embedded in the binary, so a runtime base path needs extra
  work. Use a subdomain. Planned for M11.
