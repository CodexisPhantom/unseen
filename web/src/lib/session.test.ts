import { afterEach, describe, expect, it, vi } from 'vitest';
import { api } from './api/client.js';
import { session, type Me } from './session.svelte.js';

const me = (permissions: Me['permissions']): Me => ({
	user: { id: 'u1', name: 'Member', email: 'member@example.com', twoFactorEnabled: false },
	roles: [],
	isOwner: false,
	permissions,
	scopedPermissions: permissions.map((permission) => ({ permission, libraryIds: null }))
});

describe('session', () => {
	afterEach(() => {
		session.me = null;
		vi.restoreAllMocks();
	});

	it('grants nothing while signed out', () => {
		expect(session.can('library:read')).toBe(false);
		expect(session.canAny(['library:read', 'queue:read'])).toBe(false);
	});

	it('answers from the permissions of /me', () => {
		session.me = me(['library:read']);
		expect(session.can('library:read')).toBe(true);
		expect(session.can('library:add')).toBe(false);
		expect(session.canAny(['queue:read', 'library:read'])).toBe(true);
		expect(session.canAny([])).toBe(false);
	});

	it('reloads /me on load()', async () => {
		const updated = me(['library:read', 'queue:read']);
		vi.spyOn(api, 'GET').mockResolvedValue({
			data: updated,
			response: new Response(null, { status: 200 })
		} as never);

		const result = await session.load();
		expect(result).toEqual(updated);
		expect(session.me).toEqual(updated);
	});

	it('clears the session on 401 and keeps it on transient failures', async () => {
		session.me = me(['library:read']);
		vi.spyOn(api, 'GET').mockResolvedValueOnce({
			data: undefined,
			error: undefined,
			response: new Response(null, { status: 500 })
		} as never);

		expect(await session.load()).toBeNull();
		expect(session.me).not.toBeNull();

		vi.spyOn(api, 'GET').mockRejectedValueOnce(new Error('offline'));
		expect(await session.load()).toBeNull();
		expect(session.me).not.toBeNull();

		vi.spyOn(api, 'GET').mockResolvedValueOnce({
			data: undefined,
			error: undefined,
			response: new Response(null, { status: 401 })
		} as never);
		expect(await session.load()).toBeNull();
		expect(session.me).toBeNull();
	});
});
