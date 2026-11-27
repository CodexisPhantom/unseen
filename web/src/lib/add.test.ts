import { describe, expect, it } from 'vitest';
import { loadChoices, lookupYear, pickId, saveChoices, type MovieLookup } from './add.js';

function memory(initial?: string) {
	const data = new Map<string, string>();
	if (initial !== undefined) data.set('unseenarr.add.choices', initial);
	return {
		getItem: (key: string) => data.get(key) ?? null,
		setItem: (key: string, value: string) => void data.set(key, value)
	};
}

describe('remembered choices', () => {
	it('returns what was saved', () => {
		const store = memory();
		saveChoices({ rootFolderId: 2, qualityProfileId: 7 }, store);
		expect(loadChoices(store)).toEqual({ rootFolderId: 2, qualityProfileId: 7 });
	});

	it('returns nulls for nothing, junk or an unusable store', () => {
		const none = { rootFolderId: null, qualityProfileId: null };
		expect(loadChoices(memory())).toEqual(none);
		expect(loadChoices(memory('{not json'))).toEqual(none);
		expect(loadChoices(memory('{"rootFolderId":"a","qualityProfileId":1.5}'))).toEqual(none);
		expect(loadChoices(null)).toEqual(none);
	});

	it('does not throw when the store refuses a write', () => {
		const full = {
			getItem: () => null,
			setItem: () => {
				throw new Error('quota');
			}
		};
		expect(() => saveChoices({ rootFolderId: 1, qualityProfileId: 1 }, full)).not.toThrow();
		expect(() => saveChoices({ rootFolderId: 1, qualityProfileId: 1 }, null)).not.toThrow();
	});
});

describe('pickId', () => {
	it('keeps a remembered id that is still offered', () => {
		expect(pickId(2, [1, 2, 3])).toBe(2);
	});
	it('falls back to the first offered id', () => {
		expect(pickId(9, [1, 2])).toBe(1);
		expect(pickId(null, [4])).toBe(4);
	});
	it('is null when nothing is offered', () => {
		expect(pickId(1, [])).toBeNull();
	});
});

describe('lookupYear', () => {
	const base = { tmdbId: 1, title: 't', originalTitle: 't', overview: '' } as MovieLookup;
	it('prefers year, then the release date', () => {
		expect(lookupYear({ ...base, year: 1999, releaseDate: '2001-01-01' })).toBe(1999);
		expect(lookupYear({ ...base, releaseDate: '2001-05-02' })).toBe(2001);
		expect(lookupYear(base)).toBeNull();
	});
});
