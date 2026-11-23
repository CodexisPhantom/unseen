import { createAuthClient } from 'better-auth/svelte';

// Sign-in and sign-out only, same origin (docs/16-frontend.md, "Auth").
// The library's default base path is `/api/auth`.
export const authClient = createAuthClient();
