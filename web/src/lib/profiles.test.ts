import { describe, expect, it } from 'vitest';
import {
	allowedSummary,
	cutoffName,
	defaultItems,
	draftOf,
	draftProblem,
	dropIndex,
	itemDetail,
	itemId,
	itemName,
	moveItem,
	qualityName,
	resolveCutoff,
	setAllowed,
	type ProfileItem,
	type QualityProfile
} from './profiles.js';

const quality = (qualityId: number, allowed: boolean): ProfileItem => ({
	type: 'quality',
	qualityId,
	allowed
});
const group: ProfileItem = {
	type: 'group',
	id: 1002,
	name: 'WEB 1080p',
	qualityIds: [3, 15],
	allowed: true
};

describe('names', () => {
	it('names qualities, groups and unknown ids', () => {
		expect(qualityName(7)).toBe('Bluray-1080p');
		expect(qualityName(999)).toBe('Quality 999');
		expect(itemName(group)).toBe('WEB 1080p');
		expect(itemId(group)).toBe(1002);
		expect(itemId(quality(7, true))).toBe(7);
		expect(itemDetail(group)).toBe('WEBDL-1080p, WEBRip-1080p');
		expect(itemDetail(quality(7, true))).toBe('');
	});
});

describe('defaultItems', () => {
	const items = defaultItems();

	it('lists every quality once, with the four web pairs as groups', () => {
		const ids = items.flatMap((item) =>
			item.type === 'quality' ? [item.qualityId] : item.qualityIds
		);
		expect(new Set(ids).size).toBe(30);
		expect(ids).toHaveLength(30);
		expect(items.filter((item) => item.type === 'group').map(itemId)).toEqual([
			1000, 1001, 1002, 1003
		]);
	});

	it('allows Bluray-1080p only', () => {
		expect(items.filter((item) => item.allowed).map(itemId)).toEqual([7]);
	});
});

describe('moveItem', () => {
	it('moves down and up', () => {
		expect(moveItem(['a', 'b', 'c'], 0, 2)).toEqual(['b', 'c', 'a']);
		expect(moveItem(['a', 'b', 'c'], 2, 0)).toEqual(['c', 'a', 'b']);
	});

	it('clamps the target and ignores a bad source', () => {
		expect(moveItem(['a', 'b', 'c'], 0, 9)).toEqual(['b', 'c', 'a']);
		expect(moveItem(['a', 'b', 'c'], 1, -4)).toEqual(['b', 'a', 'c']);
		expect(moveItem(['a', 'b'], 5, 0)).toEqual(['a', 'b']);
	});

	it('does not change the input', () => {
		const input = ['a', 'b'];
		moveItem(input, 0, 1);
		expect(input).toEqual(['a', 'b']);
	});
});

describe('dropIndex', () => {
	// Centres of the three rows that are not dragged.
	const centres = [10, 30, 50];

	it('counts the other rows above the pointer', () => {
		expect(dropIndex(centres, 0)).toBe(0);
		expect(dropIndex(centres, 20)).toBe(1);
		expect(dropIndex(centres, 500)).toBe(3);
		expect(dropIndex([], 5)).toBe(0);
	});
});

describe('cutoff', () => {
	const items = [quality(4, true), quality(5, false), quality(7, true)];

	it('keeps an allowed cutoff', () => {
		expect(resolveCutoff(items, 4)).toBe(4);
	});

	it('falls back to the highest allowed item', () => {
		expect(resolveCutoff(items, 5)).toBe(7);
		expect(resolveCutoff(items, null)).toBe(7);
		expect(resolveCutoff(setAllowed(items, 2, false), 7)).toBe(4);
	});

	it('is null when nothing is allowed', () => {
		expect(resolveCutoff([quality(4, false)], 4)).toBeNull();
	});
});

describe('setAllowed', () => {
	it('changes one entry and leaves the rest', () => {
		const items = [quality(4, false), quality(5, false)];
		expect(setAllowed(items, 1, true).map((item) => item.allowed)).toEqual([false, true]);
		expect(items[1].allowed).toBe(false);
	});
});

describe('draftProblem', () => {
	const ok = { name: 'HD', upgradeAllowed: true, cutoff: 7, items: [quality(7, true)] };

	it('accepts a complete draft', () => {
		expect(draftProblem(ok)).toBe('');
	});

	it('names the first broken rule', () => {
		expect(draftProblem({ ...ok, name: '  ' })).toMatch(/name/);
		expect(draftProblem({ ...ok, items: [quality(7, false)] })).toMatch(/at least one/);
		expect(draftProblem({ ...ok, cutoff: null })).toMatch(/cutoff/);
	});
});

describe('draftOf', () => {
	it('starts a new profile with a valid draft', () => {
		const draft = draftOf(null);
		expect(draft.name).toBe('');
		expect(draft.cutoff).toBe(7);
		expect(draftProblem({ ...draft, name: 'x' })).toBe('');
	});

	it('copies an existing profile', () => {
		const profile = {
			name: 'HD',
			upgradeAllowed: false,
			cutoff: 1002,
			items: [quality(7, true), group]
		} as QualityProfile;
		const draft = draftOf(profile);
		expect(draft).toMatchObject({ name: 'HD', upgradeAllowed: false, cutoff: 1002 });
		draft.items[0].allowed = false;
		expect(profile.items[0].allowed).toBe(true);
	});
});

describe('table text', () => {
	const profile = { cutoff: 1002, items: [quality(4, false), quality(7, true), group] };

	it('summarises allowed items and the cutoff', () => {
		expect(allowedSummary(profile.items)).toBe('Bluray-1080p, WEB 1080p');
		expect(cutoffName(profile)).toBe('WEB 1080p');
		expect(cutoffName({ ...profile, cutoff: 7 })).toBe('Bluray-1080p');
		expect(cutoffName({ ...profile, cutoff: 31 })).toBe('Remux-2160p');
	});
});
