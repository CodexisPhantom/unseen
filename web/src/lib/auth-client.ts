import { createAuthClient } from 'better-auth/svelte';
import { passkeyClient } from '@better-auth/passkey/client';
import { twoFactorClient } from 'better-auth/client/plugins';
import { withBase } from './base-path.js';

export const authClient = createAuthClient({
	basePath: withBase('/api/auth'),
	plugins: [twoFactorClient(), passkeyClient()]
});
