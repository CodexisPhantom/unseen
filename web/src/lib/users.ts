import type { components } from './api/schema.js';
import type { Permission } from './session.svelte.js';

type Role = components['schemas']['Role'];

export interface PermissionSource {
	permission: Permission;
	/** Role names, and `direct` for a direct grant. */
	sources: string[];
}

/**
 * The effective permissions of a user with the source of each: every role
 * the user holds plus direct grants. The Owner role holds the whole catalog.
 */
export function effectivePermissions(
	roleIds: readonly number[],
	direct: readonly Permission[],
	roles: readonly Role[],
	catalog: readonly Permission[]
): PermissionSource[] {
	const held = roles.filter((role) => roleIds.includes(role.id));
	return catalog
		.map((permission) => ({
			permission,
			sources: [
				...held.filter((role) => role.permissions.includes(permission)).map((role) => role.name),
				...(direct.includes(permission) ? ['direct'] : [])
			]
		}))
		.filter((entry) => entry.sources.length > 0);
}

/** Only an Owner gives the Owner role; any other role needs all its permissions. */
export function canAssignRole(
	role: Role,
	caller: { isOwner: boolean; can: (permission: Permission) => boolean }
): boolean {
	return role.isOwner
		? caller.isOwner
		: role.permissions.every((permission) => caller.can(permission));
}

/** Only an Owner changes an Owner. */
export function canChangeUser(target: { isOwner: boolean }, caller: { isOwner: boolean }): boolean {
	return caller.isOwner || !target.isOwner;
}
