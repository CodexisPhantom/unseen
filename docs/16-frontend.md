# 16 Frontend conventions

Status: draft. Decision: ADR 0003, ADR 0009.

## Stack

SvelteKit 2, Svelte 5 with runes, TypeScript, `adapter-static` with an SPA
fallback (`fallback: 'index.html'`, `ssr = false`, `prerender = false` in the
root layout). Tailwind CSS v4 through `@tailwindcss/vite`, configured in CSS
(`@import "tailwindcss"`, `@theme`). No `tailwind.config.js`.

Created with:

```
pnpm dlx sv create web --add tailwindcss
cd web
pnpm dlx shadcn-svelte@latest init
```

## Forbidden because of adapter-static

- `+page.server.ts`, `+layout.server.ts`, server `load` functions
- Form actions
- `+server.ts` routes
- `hooks.server.ts`
- Any import from `$env/*/private`

All data goes through the generated API client. A lint rule or CI check
fails on the file names above.

## Routes

```
src/routes/
  +layout.svelte            ModeWatcher, global CSS
  setup/                    first-run Owner creation
  login/
  (app)/                    authenticated shell: sidebar, header
    +layout.ts              loads /me, redirects to /login or /setup
    library/                list and movie detail
    add/                    search TMDB and add
    activity/               queue
    settings/               general, root folders, profiles, indexers, clients
    users/  roles/  api-keys/
    import/                 adopt folder, import from Radarr
    system/                 tasks
```

## API client

- `openapi-typescript` generates `src/lib/api/schema.d.ts` from the committed
  `openapi.json`. `openapi-fetch` is the runtime (open question: confirm).
- One client instance in `src/lib/api/client.ts`. Components never call
  `fetch` directly.
- Errors are problem details ([05-api.md](05-api.md)); a helper turns `code`
  into a message or a form error.
- A 401 response sends the user to `/login`.

## Auth

The official `better-auth` Svelte client, pinned to the version Better Auth RS
targets (`1.7.6`), pointed at `/api/auth`. Used for sign-in and sign-out only.

## State with runes

- Shared state lives in `*.svelte.ts` modules that export a class or object
  using `$state` and `$derived`.
- `session.svelte.ts`: user, roles, permissions from `/me`.
- `events.svelte.ts`: one `EventSource` on `/api/v1/events`, opened in the
  `(app)` layout, closed on logout. Handlers update the stores. Reconnects
  refetch the affected lists.
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
progress, tooltip, form, separator, avatar, breadcrumb.

- Tables: the shadcn-svelte data table with `@tanstack/svelte-table`
  (TanStack Table v9).
- Forms: the shadcn-svelte form components (formsnap, sveltekit-superforms,
  zod) in client-only mode: `superForm(defaults(zod4(schema)), { SPA: true })`,
  submit through the API client in `onUpdate`.

## Theming

- shadcn-svelte CSS variables in the global CSS file (`--background`,
  `--foreground`, `--primary`, `--muted`, `--destructive`, `--border`,
  `--ring`, `--sidebar-*`), mapped in `@theme inline`.
- Light and dark through `mode-watcher`; the toggle offers light, dark and
  system.
- Use tokens (`bg-background`, `text-muted-foreground`), never raw colours.

## Open questions

- `openapi-fetch` or a thinner hand-written wrapper.
- shadcn-svelte now also ships a `Field` component; decide Form or Field.
- Data fetching and caching helper (plain runes or TanStack Query).
- Hosting under a sub-path.
