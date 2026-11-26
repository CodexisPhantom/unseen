import { describe, expect, it } from 'vitest';
import { entryFor, navEntries } from './nav.js';

describe('entryFor', () => {
	it('matches the first segment only', () => {
		expect(entryFor('library')?.title).toBe('Library');
		expect(entryFor('library/12/files')?.title).toBe('Library');
		expect(entryFor('/settings/indexers')?.title).toBe('Settings');
		expect(entryFor('libraryx')).toBeUndefined();
		expect(entryFor('settings/profiles')?.title).toBe('Settings');
	});

	it('lists the indexers page for indexers:read only', () => {
		const entry = navEntries.find((candidate) => candidate.path === 'settings/indexers');
		expect(entry?.anyOf).toEqual(['indexers:read']);
	});

	it('guards the collections pages with library:read', () => {
		expect(entryFor('collections')?.title).toBe('Collections');
		expect(entryFor('collections')?.anyOf).toEqual(['library:read']);
		expect(entryFor('collections/42')?.title).toBe('Collections');
	});

	it('guards the import page with import:run', () => {
		expect(entryFor('import')?.anyOf).toEqual(['import:run']);
	});

	it('needs no permission for the home page and unknown pages', () => {
		expect(entryFor('')).toBeUndefined();
		expect(entryFor('nope')).toBeUndefined();
	});
});
