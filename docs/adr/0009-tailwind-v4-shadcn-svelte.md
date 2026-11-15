# 0009 Tailwind v4 + shadcn-svelte

Status: accepted (2026-11-15)

## Context

The UI needs many standard components: tables, forms, dialogs, a sidebar.
Writing and maintaining them by hand is not the goal of this project.

## Decision

- Tailwind CSS v4, configured in CSS (`@import "tailwindcss"`, `@theme`)
  through the `@tailwindcss/vite` plugin. No Tailwind v3 and no
  `tailwind.config.js` unless a tool forces it.
- shadcn-svelte for all UI components. Components are copied into
  `web/src/lib/components/ui/` and committed.
- Theming through its CSS variables; light and dark with `mode-watcher`.
- Tables: the shadcn-svelte data table on `@tanstack/svelte-table`
  (TanStack Table v9).
- Forms: the shadcn-svelte form components (formsnap, sveltekit-superforms,
  zod) in client-only mode (`SPA: true`).
- No second component library. Missing components are built on Bits UI in
  the same style.

## Consequences

- Components are our code: we can change them, and we carry their updates.
- A consistent look with little design work.
- sveltekit-superforms is designed around server actions; SPA mode is the
  supported way without a server, and all forms must use it.
- The stack moves fast. Re-check the install steps when adding components
  ([16-frontend.md](../16-frontend.md)).
