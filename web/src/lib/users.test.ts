import { describe, expect, it } from 'vitest';
import type { components } from './api/schema.js';
import type { Permission } from './session.svelte.js';
import { canAssignRole, canChangeUser, effectivePermissions, formatLibraryScope } from './users.js';

type Role = components['schemas']['Role'];

const catalog: Permission[] = ['library:read', 'queue:read', 'queue:manage'];
const role = (id: number, name: string, permissions: Permission[], isOwner = false): Role => ({
	id,
	name,
	description: null,
	isOwner,
	permissions,
	users: 0
});
const roles = [
	role(1, 'Owner', catalog, true),
	role(2, 'Member', ['library:read', 'queue:read']),
	role(3, 'Viewer', ['library:read'])
];

describe('effectivePermissions', () => {
	it('names every source of a permission, in catalog order', () => {
		expect(effectivePermissions([2, 3], ['queue:manage', 'library:read'], roles, catalog)).toEqual([
			{ permission: 'library:read', sources: ['Member', 'Viewer', 'direct'] },
			{ permission: 'queue:read', sources: ['Member'] },
			{ permission: 'queue:manage', sources: ['direct'] }
		]);
	});

	it('gives the Owner role the whole catalog and an empty user nothing', () => {
		expect(effectivePermissions([1], [], roles, catalog).map((entry) => entry.permission)).toEqual(
			catalog
		);
		expect(effectivePermissions([], [], roles, catalog)).toEqual([]);
	});
});

describe('canAssignRole', () => {
	const caller = (isOwner: boolean, held: Permission[]) => ({
		isOwner,
		can: (permission: Permission) => held.includes(permission)
	});

	it('needs every permission of the role', () => {
		expect(canAssignRole(roles[1], caller(false, ['library:read', 'queue:read']))).toBe(true);
		expect(canAssignRole(roles[1], caller(false, ['library:read']))).toBe(false);
	});

	it('keeps the Owner role for Owners, even for a caller who holds everything', () => {
		expect(canAssignRole(roles[0], caller(false, catalog))).toBe(false);
		expect(canAssignRole(roles[0], caller(true, []))).toBe(true);
	});
});

describe('canChangeUser', () => {
	it('lets only an Owner change an Owner', () => {
		expect(canChangeUser({ isOwner: true }, { isOwner: false })).toBe(false);
		expect(canChangeUser({ isOwner: true }, { isOwner: true })).toBe(true);
		expect(canChangeUser({ isOwner: false }, { isOwner: false })).toBe(true);
	});
});

describe('formatLibraryScope', () => {
	const libs = [
		{ id: 1, name: 'Default' },
		{ id: 2, name: 'Kids' }
	];

	it('returns null for all-library scope and names restricted libraries', () => {
		expect(formatLibraryScope(null, libs)).toBeNull();
		expect(formatLibraryScope(undefined, libs)).toBeNull();
		expect(formatLibraryScope([], libs)).toBe('no libraries');
		expect(formatLibraryScope([2], libs)).toBe('Kids');
		expect(formatLibraryScope([1, 2, 99], libs)).toBe('Default, Kids, #99');
	});
});
