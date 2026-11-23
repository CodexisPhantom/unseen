import type { components } from './api/schema.js';

export type Me = components['schemas']['Me'];
export type Permission = components['schemas']['Permission'];

/**
 * The signed-in user from `GET /api/v1/me`. Presentation only: the server
 * enforces every permission (docs/16-frontend.md).
 */
class Session {
	me = $state<Me | null>(null);

	can(permission: Permission): boolean {
		return this.me?.permissions.includes(permission) ?? false;
	}

	/** True when the user holds at least one of `permissions`. */
	canAny(permissions: readonly Permission[]): boolean {
		return permissions.some((permission) => this.can(permission));
	}
}

export const session = new Session();
