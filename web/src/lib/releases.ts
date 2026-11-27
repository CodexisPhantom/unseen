import type { components } from './api/schema.js';

export type Release = components['schemas']['Release'];
export type MovieReleases = components['schemas']['MovieReleases'];

export type SortKey = 'title' | 'indexer' | 'quality' | 'size' | 'seeders' | 'age' | 'score';
export type SortDirection = 'asc' | 'desc';

/** The value of a release in a column. `null` means the indexer did not report it. */
const value: Record<SortKey, (release: Release) => string | number | null> = {
	title: (r) => r.title.toLowerCase(),
	indexer: (r) => r.indexerName.toLowerCase(),
	quality: (r) => r.qualityId,
	size: (r) => r.size ?? null,
	seeders: (r) => r.seeders ?? null,
	age: (r) => r.ageHours ?? null,
	score: (r) => r.score.customFormatScore
};

/**
 * The releases in the order of `key`. Without a key the order stays as the
 * server ranked it. A missing value sorts last in both directions. Never
 * changes `releases`.
 */
export function sortReleases(
	releases: readonly Release[],
	key: SortKey | null,
	direction: SortDirection
): Release[] {
	if (key === null) return [...releases];
	const sign = direction === 'asc' ? 1 : -1;
	const read = value[key];
	return releases
		.map((release, index) => ({ release, index, sortValue: read(release) }))
		.sort((a, b) => {
			if (a.sortValue === null || b.sortValue === null) {
				if (a.sortValue === b.sortValue) return a.index - b.index;
				return a.sortValue === null ? 1 : -1;
			}
			const order = a.sortValue < b.sortValue ? -1 : a.sortValue > b.sortValue ? 1 : 0;
			return sign * order || a.index - b.index;
		})
		.map((entry) => entry.release);
}

/** The qualities that occur in `releases`, best first. */
export function qualityChoices(releases: readonly Release[]): { id: number; name: string }[] {
	const found = new Map<number, string>();
	for (const release of releases) found.set(release.qualityId, release.qualityName);
	return [...found].map(([id, name]) => ({ id, name })).sort((a, b) => b.id - a.id);
}

/** The releases of one quality, or all of them for `null`. */
export function filterByQuality(
	releases: readonly Release[],
	qualityId: number | null
): Release[] {
	return qualityId === null
		? [...releases]
		: releases.filter((release) => release.qualityId === qualityId);
}

/** The reasons a release is rejected, one per line. */
export function rejectionText(release: Release): string {
	return release.rejections.map((rejection) => rejection.reason).join('\n');
}

/** `45 min`, `5 h`, `12 d` or `3 y`, or a dash. */
export function formatAge(hours: number | null | undefined): string {
	if (hours === null || hours === undefined || !Number.isFinite(hours) || hours < 0) return '—';
	if (hours < 1) return `${Math.round(hours * 60)} min`;
	if (hours < 48) return `${Math.floor(hours)} h`;
	const days = hours / 24;
	if (days < 730) return `${Math.floor(days)} d`;
	return `${Math.floor(days / 365)} y`;
}

/** `+500`, `-100` or `0`. */
export function formatScore(score: number): string {
	return score > 0 ? `+${score}` : String(score);
}
