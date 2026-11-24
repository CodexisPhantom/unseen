import { describe, expect, it } from 'vitest';
import { groupPermissions, missingPermissions, roleTemplates } from './roles.js';
import type { Permission } from './session.svelte.js';

const catalog: Permission[] = [
	'library:read',
	'library:add',
	'queue:read',
	'roles:manage',
	'system:audit'
];

describe('groupPermissions', () => {
	it('groups by resource and keeps the catalog order', () => {
		expect(groupPermissions(catalog)).toEqual([
			{ resource: 'library', permissions: ['library:read', 'library:add'] },
			{ resource: 'queue', permissions: ['queue:read'] },
			{ resource: 'roles', permissions: ['roles:manage'] },
			{ resource: 'system', permissions: ['system:audit'] }
		]);
	});
});

describe('roleTemplates', () => {
	const byName = (name: string) => roleTemplates.find((template) => template.name === name)!;

	it('Admin lists every permission of the catalog', () => {
		expect(byName('Admin').permissions(catalog)).toEqual(catalog);
	});

	it('Member and Read-only follow docs/15-permissions.md', () => {
		expect(byName('Member').permissions(catalog)).toEqual([
			'library:read',
			'library:add',
			'search:run',
			'queue:read'
		]);
		expect(byName('Read-only').permissions(catalog)).toEqual(['library:read', 'queue:read']);
	});
});

describe('missingPermissions', () => {
	it('lists what the user lacks', () => {
		const can = (permission: Permission) => permission === 'library:read';
		expect(missingPermissions(['library:read', 'queue:read'], can)).toEqual(['queue:read']);
		expect(missingPermissions([], can)).toEqual([]);
	});
});
