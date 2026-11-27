import { describe, expect, it } from 'vitest';
import {
	filterByQuality,
	formatAge,
	formatScore,
	qualityChoices,
	rejectionText,
	sortReleases,
	type Release
} from './releases.js';

function release(id: string, overrides: Partial<Release> = {}): Release {
	return {
		id,
		guid: id,
		title: id,
		indexerId: 1,
		indexerName: 'Indexer',
		qualityId: 7,
		qualityName: 'Bluray-1080p',
		size: 1000,
		seeders: 10,
		leechers: 0,
		publishedAt: null,
		ageHours: 5,
		protocol: 'torrent',
		languages: ['en'],
		releaseGroup: null,
		edition: null,
		proper: false,
		repack: false,
		accepted: true,
		rejections: [],
		customFormats: [],
		score: {
			rank: 1,
			qualityRank: 1,
			customFormatScore: 0,
			revision: 1,
			indexerPriority: 25,
			seederScore: 1,
			preferredSizeDistance: null
		},
		...overrides
	};
}

const ids = (releases: readonly Release[]) => releases.map((r) => r.id);

describe('sortReleases', () => {
	const a = release('a', { size: 300, seeders: 5, title: 'B movie' });
	const b = release('b', { size: 100, seeders: null, title: 'a movie' });
	const c = release('c', { size: 200, seeders: 50, title: 'C movie' });

	it('keeps the server order without a key', () => {
		expect(ids(sortReleases([c, a, b], null, 'asc'))).toEqual(['c', 'a', 'b']);
	});

	it('sorts by a number in both directions', () => {
		expect(ids(sortReleases([a, b, c], 'size', 'asc'))).toEqual(['b', 'c', 'a']);
		expect(ids(sortReleases([a, b, c], 'size', 'desc'))).toEqual(['a', 'c', 'b']);
	});

	it('puts a missing value last in both directions', () => {
		expect(ids(sortReleases([a, b, c], 'seeders', 'asc'))).toEqual(['a', 'c', 'b']);
		expect(ids(sortReleases([a, b, c], 'seeders', 'desc'))).toEqual(['c', 'a', 'b']);
	});

	it('sorts titles without regard to case', () => {
		expect(ids(sortReleases([a, b, c], 'title', 'asc'))).toEqual(['b', 'a', 'c']);
	});

	it('sorts by custom format score and keeps ties in server order', () => {
		const high = release('high', { score: { ...a.score, customFormatScore: 500 } });
		expect(ids(sortReleases([a, high, c], 'score', 'desc'))).toEqual(['high', 'a', 'c']);
	});

	it('does not change the input', () => {
		const input = [a, b, c];
		sortReleases(input, 'size', 'asc');
		expect(ids(input)).toEqual(['a', 'b', 'c']);
	});
});

describe('quality filter', () => {
	const hd = release('hd', { qualityId: 7, qualityName: 'Bluray-1080p' });
	const uhd = release('uhd', { qualityId: 19, qualityName: 'WEBDL-2160p' });
	const hd2 = release('hd2', { qualityId: 7, qualityName: 'Bluray-1080p' });

	it('lists each quality once, best first', () => {
		expect(qualityChoices([hd, uhd, hd2])).toEqual([
			{ id: 19, name: 'WEBDL-2160p' },
			{ id: 7, name: 'Bluray-1080p' }
		]);
	});

	it('keeps one quality, or everything for null', () => {
		expect(ids(filterByQuality([hd, uhd, hd2], 7))).toEqual(['hd', 'hd2']);
		expect(ids(filterByQuality([hd, uhd, hd2], null))).toEqual(['hd', 'uhd', 'hd2']);
	});
});

describe('formatting', () => {
	it('joins the rejection reasons', () => {
		const rejected = release('r', {
			accepted: false,
			rejections: [
				{ code: 'quality_not_allowed', reason: 'Quality is not allowed' },
				{ code: 'too_large', reason: 'Larger than the maximum' }
			]
		});
		expect(rejectionText(rejected)).toBe('Quality is not allowed\nLarger than the maximum');
	});

	it('shows an age in the nearest unit', () => {
		expect(formatAge(0.5)).toBe('30 min');
		expect(formatAge(5.9)).toBe('5 h');
		expect(formatAge(72)).toBe('3 d');
		expect(formatAge(24 * 800)).toBe('2 y');
		expect(formatAge(null)).toBe('—');
	});

	it('signs a score', () => {
		expect(formatScore(500)).toBe('+500');
		expect(formatScore(-100)).toBe('-100');
		expect(formatScore(0)).toBe('0');
	});
});
