import { createAuthClient } from 'better-auth/svelte';
import { withBase } from './base-path.js';

// Sign-in and sign-out only, same origin (docs/16-frontend.md, "Auth").
// Prefixed with the runtime base path for sub-path hosting.
export const authClient = createAuthClient({ basePath: withBase('/api/auth') });
