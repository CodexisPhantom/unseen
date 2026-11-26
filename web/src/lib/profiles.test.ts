import { describe, expect, it } from 'vitest';
import {
	allowedSummary,
	cutoffName,
	defaultItems,
	draftOf,
	draftProblem,
	dropIndex,
	formatScorePayload,
	formatScoreSummary,
	itemDetail,
	itemId,
	itemName,
	moveItem,
	qualityName,
	resolveCutoff,
	setAllowed,
	setFormatScore,
	type ProfileFormatScore,
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
	const ok = {
		name: 'HD',
		upgradeAllowed: true,
		cutoff: 7,
		items: [quality(7, true)],
		minFormatScore: 0,
		cutoffFormatScore: 0,
		minUpgradeFormatScore: 1,
		formatScores: [] as ProfileFormatScore[]
	};

	it('accepts a complete draft', () => {
		expect(draftProblem(ok)).toBe('');
	});

	it('names the first broken rule', () => {
		expect(draftProblem({ ...ok, name: '  ' })).toMatch(/name/);
		expect(draftProblem({ ...ok, items: [quality(7, false)] })).toMatch(/at least one/);
		expect(draftProblem({ ...ok, cutoff: null })).toMatch(/cutoff/);
		expect(draftProblem({ ...ok, minFormatScore: 1.5 })).toMatch(/whole number/);
		expect(draftProblem({ ...ok, cutoffFormatScore: Number.NaN })).toMatch(/whole number/);
		expect(draftProblem({ ...ok, minUpgradeFormatScore: 0 })).toMatch(/at least 1/);
		expect(
			draftProblem({
				...ok,
				formatScores: [{ customFormatId: 1, name: 'HDR', score: 1.5 }]
			})
		).toMatch(/whole numbers/);
	});
});

describe('draftOf', () => {
	it('starts a new profile with a valid draft and zeroes available formats', () => {
		const available: ProfileFormatScore[] = [{ customFormatId: 2, name: 'HDR', score: 500 }];
		const draft = draftOf(null, available);
		expect(draft.name).toBe('');
		expect(draft.cutoff).toBe(7);
		expect(draft.minFormatScore).toBe(0);
		expect(draft.cutoffFormatScore).toBe(0);
		expect(draft.minUpgradeFormatScore).toBe(1);
		expect(draft.formatScores).toEqual([{ customFormatId: 2, name: 'HDR', score: 0 }]);
		expect(draftProblem({ ...draft, name: 'x' })).toBe('');
	});

	it('copies an existing profile including format scores', () => {
		const profile = {
			name: 'HD',
			upgradeAllowed: false,
			cutoff: 1002,
			items: [quality(7, true), group],
			minFormatScore: 50,
			cutoffFormatScore: 500,
			minUpgradeFormatScore: 25,
			formatScores: [{ customFormatId: 1, name: 'HDR', score: 300 }]
		} as QualityProfile;
		const draft = draftOf(profile);
		expect(draft).toMatchObject({
			name: 'HD',
			upgradeAllowed: false,
			cutoff: 1002,
			minFormatScore: 50,
			cutoffFormatScore: 500,
			minUpgradeFormatScore: 25
		});
		draft.items[0].allowed = false;
		draft.formatScores[0].score = 0;
		expect(profile.items[0].allowed).toBe(true);
		expect(profile.formatScores[0].score).toBe(300);
	});
});

describe('format scores helpers', () => {
	const scores: ProfileFormatScore[] = [
		{ customFormatId: 1, name: 'HDR', score: 500 },
		{ customFormatId: 2, name: 'x265', score: 0 }
	];

	it('updates a single format score without mutating input', () => {
		const next = setFormatScore(scores, 2, -100);
		expect(next).toEqual([
			{ customFormatId: 1, name: 'HDR', score: 500 },
			{ customFormatId: 2, name: 'x265', score: -100 }
		]);
		expect(scores[1].score).toBe(0);
	});

	it('filters zero scores from the request payload', () => {
		expect(formatScorePayload(scores)).toEqual([{ customFormatId: 1, score: 500 }]);
	});

	it('summarises format scores for the table', () => {
		expect(
			formatScoreSummary({
				minFormatScore: 0,
				cutoffFormatScore: 0,
				formatScores: [{ customFormatId: 1, name: 'HDR', score: 0 }]
			})
		).toBe('—');
		expect(
			formatScoreSummary({
				minFormatScore: 100,
				cutoffFormatScore: 500,
				formatScores: scores
			})
		).toBe('1 scored · min 100 · cutoff 500');
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
