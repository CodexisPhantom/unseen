import type { Permission } from './session.svelte.js';

export interface NavEntry {
	title: string;
	/** First path segment under the app root, for example `library`. */
	path: string;
	/** Shown when the user holds any of these. */
	anyOf: readonly Permission[];
}

export const navEntries: readonly NavEntry[] = [
	{ title: 'Library', path: 'library', anyOf: ['library:read'] },
	{ title: 'Add', path: 'add', anyOf: ['library:add'] },
	{ title: 'Activity', path: 'activity', anyOf: ['queue:read'] },
	{
		title: 'Settings',
		path: 'settings',
		anyOf: ['settings:read', 'profiles:read', 'indexers:read', 'clients:read']
	},
	{
		title: 'System',
		path: 'system',
		anyOf: ['system:tasks', 'system:logs', 'system:backup', 'system:audit']
	}
];

/**
 * The entry that owns `relativePath` (the path after the app root, without a
 * leading slash), or undefined when the page needs no permission.
 */
export function entryFor(relativePath: string): NavEntry | undefined {
	const segment = relativePath.replace(/^\/+/, '').split('/')[0];
	return navEntries.find((entry) => entry.path === segment);
}
