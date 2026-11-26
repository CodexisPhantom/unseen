import { createAuthClient } from 'better-auth/svelte';
import { passkeyClient } from '@better-auth/passkey/client';
import { twoFactorClient } from 'better-auth/client/plugins';

// Sign-in and sign-out only, same origin (docs/16-frontend.md, "Auth").
// The library's default base path is `/api/auth`.
export const authClient = createAuthClient({
	plugins: [twoFactorClient(), passkeyClient()]
});
