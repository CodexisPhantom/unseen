import { afterEach, describe, expect, it } from 'vitest';
import { session, type Me } from './session.svelte.js';

const me = (permissions: Me['permissions']): Me => ({
	user: { id: 'u1', name: 'Member', email: 'member@example.com', twoFactorEnabled: false },
	roles: [],
	isOwner: false,
	permissions
});

describe('session', () => {
	afterEach(() => {
		session.me = null;
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
});
