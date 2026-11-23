import { describe, expect, it } from 'vitest';
import { entryFor } from './nav.js';

describe('entryFor', () => {
	it('matches the first segment only', () => {
		expect(entryFor('library')?.title).toBe('Library');
		expect(entryFor('library/12/files')?.title).toBe('Library');
		expect(entryFor('/settings/indexers')?.title).toBe('Settings');
		expect(entryFor('libraryx')).toBeUndefined();
	});

	it('needs no permission for the home page and unknown pages', () => {
		expect(entryFor('')).toBeUndefined();
		expect(entryFor('nope')).toBeUndefined();
	});
});
