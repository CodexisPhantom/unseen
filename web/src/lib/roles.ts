import type { Permission } from './session.svelte.js';

export interface RoleTemplate {
	name: string;
	description: string;
	/** `catalog` is every permission, in catalog order. */
	permissions: (catalog: readonly Permission[]) => Permission[];
}

// docs/15-permissions.md, "Custom roles". Templates only pre-fill the form.
export const roleTemplates: readonly RoleTemplate[] = [
	{
		name: 'Admin',
		description: 'Every permission that exists today.',
		permissions: (catalog) => [...catalog]
	},
	{
		name: 'Member',
		description: 'Browse, add movies, search and see downloads.',
		permissions: () => ['library:read', 'library:add', 'search:run', 'queue:read']
	},
	{
		name: 'Read-only',
		description: 'Browse the library and see downloads.',
		permissions: () => ['library:read', 'queue:read']
	}
];

export interface PermissionGroup {
	/** The part before the colon, for example `library`. */
	resource: string;
	permissions: Permission[];
}

/** Groups the catalog by resource, keeping the catalog order. */
export function groupPermissions(catalog: readonly Permission[]): PermissionGroup[] {
	const groups = new Map<string, Permission[]>();
	for (const permission of catalog) {
		const resource = permission.split(':')[0];
		groups.set(resource, [...(groups.get(resource) ?? []), permission]);
	}
	return [...groups].map(([resource, permissions]) => ({ resource, permissions }));
}

/** Permissions of a role that the signed-in user does not hold. */
export function missingPermissions(
	role: readonly Permission[],
	can: (permission: Permission) => boolean
): Permission[] {
	return role.filter((permission) => !can(permission));
}
