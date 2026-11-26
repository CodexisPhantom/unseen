import type { components } from './api/schema.js';

export type ProfileItem = components['schemas']['ProfileItem'];
export type QualityProfile = components['schemas']['QualityProfile'];
export type ProfileFormatScore = components['schemas']['ProfileFormatScore'];
export type FormatScoreInput = components['schemas']['FormatScoreInput'];

/** Radarr's quality ids and names in the default order, lowest first (crates/core/src/quality.rs). */
const qualities: readonly (readonly [number, string])[] = [
	[0, 'Unknown'],
	[24, 'WORKPRINT'],
	[25, 'CAM'],
	[26, 'TELESYNC'],
	[27, 'TELECINE'],
	[29, 'REGIONAL'],
	[28, 'DVDSCR'],
	[1, 'SDTV'],
	[2, 'DVD'],
	[23, 'DVD-R'],
	[8, 'WEBDL-480p'],
	[12, 'WEBRip-480p'],
	[20, 'Bluray-480p'],
	[21, 'Bluray-576p'],
	[4, 'HDTV-720p'],
	[5, 'WEBDL-720p'],
	[14, 'WEBRip-720p'],
	[6, 'Bluray-720p'],
	[9, 'HDTV-1080p'],
	[3, 'WEBDL-1080p'],
	[15, 'WEBRip-1080p'],
	[7, 'Bluray-1080p'],
	[30, 'Remux-1080p'],
	[16, 'HDTV-2160p'],
	[18, 'WEBDL-2160p'],
	[17, 'WEBRip-2160p'],
	[19, 'Bluray-2160p'],
	[31, 'Remux-2160p'],
	[22, 'BR-DISK'],
	[10, 'Raw-HD']
];

const qualityNames = new Map(qualities);

/** The WEBDL and WEBRip pair of a resolution is one group, as in the seeded profiles. */
const webGroups = [
	{ id: 1000, name: 'WEB 480p', qualityIds: [8, 12] },
	{ id: 1001, name: 'WEB 720p', qualityIds: [5, 14] },
	{ id: 1002, name: 'WEB 1080p', qualityIds: [3, 15] },
	{ id: 1003, name: 'WEB 2160p', qualityIds: [18, 17] }
];

const BLURAY_1080P = 7;

/** The id a cutoff uses for an item: the quality id or the group id. */
export function itemId(item: ProfileItem): number {
	return item.type === 'quality' ? item.qualityId : item.id;
}

export function qualityName(id: number): string {
	return qualityNames.get(id) ?? `Quality ${id}`;
}

export function itemName(item: ProfileItem): string {
	return item.type === 'quality' ? qualityName(item.qualityId) : item.name;
}

/** The members of a group, for a second line under its name. */
export function itemDetail(item: ProfileItem): string {
	return item.type === 'group' ? item.qualityIds.map(qualityName).join(', ') : '';
}

/** Every quality, nothing allowed except Bluray-1080p. The start of a new profile. */
export function defaultItems(): ProfileItem[] {
	const inGroup = new Set(webGroups.flatMap((group) => group.qualityIds));
	const items: ProfileItem[] = [];
	for (const [id] of qualities) {
		const group = webGroups.find((candidate) => candidate.qualityIds[0] === id);
		if (group) items.push({ type: 'group', allowed: false, ...group });
		else if (!inGroup.has(id))
			items.push({ type: 'quality', qualityId: id, allowed: id === BLURAY_1080P });
	}
	return items;
}

/** A copy of `items` with the entry at `from` moved to `to`. Out of range returns a copy. */
export function moveItem<T>(items: readonly T[], from: number, to: number): T[] {
	const copy = [...items];
	if (from < 0 || from >= copy.length) return copy;
	const target = Math.min(Math.max(to, 0), copy.length - 1);
	const [moved] = copy.splice(from, 1);
	copy.splice(target, 0, moved);
	return copy;
}

/**
 * The index a dragged row takes: the number of other rows whose centre is
 * above the pointer. `centres` holds the vertical centre of every row except
 * the dragged one, top to bottom.
 */
export function dropIndex(centres: readonly number[], pointerY: number): number {
	return centres.filter((centre) => pointerY > centre).length;
}

/** The items with the allowed flag of the entry at `index` set. */
export function setAllowed(items: readonly ProfileItem[], index: number, allowed: boolean) {
	return items.map((item, i) => (i === index ? { ...item, allowed } : item));
}

/** Items the cutoff can name: the allowed ones. */
export function cutoffChoices(items: readonly ProfileItem[]): ProfileItem[] {
	return items.filter((item) => item.allowed);
}

/**
 * The cutoff to keep after the items changed: the current one while it is
 * still allowed, otherwise the highest allowed item, or null when none is.
 */
export function resolveCutoff(items: readonly ProfileItem[], cutoff: number | null): number | null {
	const choices = cutoffChoices(items);
	if (cutoff !== null && choices.some((item) => itemId(item) === cutoff)) return cutoff;
	return choices.length ? itemId(choices[choices.length - 1]) : null;
}

export interface Draft {
	name: string;
	upgradeAllowed: boolean;
	cutoff: number | null;
	items: ProfileItem[];
	minFormatScore: number;
	cutoffFormatScore: number;
	minUpgradeFormatScore: number;
	formatScores: ProfileFormatScore[];
}

/** The first rule the draft breaks, as the server words it, or an empty string. */
export function draftProblem(draft: Draft): string {
	if (!draft.name.trim()) return 'The name must not be empty.';
	if (!draft.items.some((item) => item.allowed)) return 'Allow at least one quality.';
	if (draft.cutoff === null) return 'Choose a cutoff.';
	if (!Number.isInteger(draft.minFormatScore)) return 'Minimum format score must be a whole number.';
	if (!Number.isInteger(draft.cutoffFormatScore))
		return 'Upgrade until score must be a whole number.';
	if (!Number.isInteger(draft.minUpgradeFormatScore) || draft.minUpgradeFormatScore < 1)
		return 'Minimum upgrade gain must be at least 1.';
	if (draft.formatScores.some((entry) => !Number.isInteger(entry.score)))
		return 'Custom format scores must be whole numbers.';
	return '';
}

export function draftOf(
	profile: QualityProfile | null,
	availableFormats: readonly ProfileFormatScore[] = []
): Draft {
	if (!profile) {
		const items = defaultItems();
		return {
			name: '',
			upgradeAllowed: true,
			cutoff: resolveCutoff(items, BLURAY_1080P),
			items,
			minFormatScore: 0,
			cutoffFormatScore: 0,
			minUpgradeFormatScore: 1,
			formatScores: availableFormats.map((entry) => ({
				customFormatId: entry.customFormatId,
				name: entry.name,
				score: 0
			}))
		};
	}
	return {
		name: profile.name,
		upgradeAllowed: profile.upgradeAllowed,
		cutoff: resolveCutoff(profile.items, profile.cutoff),
		items: structuredClone(profile.items),
		minFormatScore: profile.minFormatScore ?? 0,
		cutoffFormatScore: profile.cutoffFormatScore ?? 0,
		minUpgradeFormatScore: profile.minUpgradeFormatScore ?? 1,
		formatScores: (profile.formatScores ?? []).map((entry) => ({ ...entry }))
	};
}

/** Returns a copy of `scores` with the score for `customFormatId` updated. */
export function setFormatScore(
	scores: readonly ProfileFormatScore[],
	customFormatId: number,
	score: number
): ProfileFormatScore[] {
	return scores.map((entry) =>
		entry.customFormatId === customFormatId ? { ...entry, score } : entry
	);
}

/** Non-zero score assignments ready for POST/PATCH requests. */
export function formatScorePayload(scores: readonly ProfileFormatScore[]): FormatScoreInput[] {
	return scores
		.filter((entry) => entry.score !== 0)
		.map(({ customFormatId, score }) => ({ customFormatId, score }));
}

/** Allowed names in list order, for a table cell. */
export function allowedSummary(items: readonly ProfileItem[]): string {
	return cutoffChoices(items).map(itemName).join(', ');
}

/** Cutoff name for a table cell. */
export function cutoffName(profile: Pick<QualityProfile, 'items' | 'cutoff'>): string {
	const match = profile.items.find((item) => itemId(item) === profile.cutoff);
	return match ? itemName(match) : qualityName(profile.cutoff);
}

/** Short summary of custom format scoring on a profile, for a table cell. */
export function formatScoreSummary(
	profile: Pick<QualityProfile, 'minFormatScore' | 'cutoffFormatScore' | 'formatScores'>
): string {
	const scored = (profile.formatScores ?? []).filter((entry) => entry.score !== 0).length;
	const parts: string[] = [];
	if (scored > 0) parts.push(`${scored} scored`);
	if (profile.minFormatScore !== 0) parts.push(`min ${profile.minFormatScore}`);
	if (profile.cutoffFormatScore !== 0) parts.push(`cutoff ${profile.cutoffFormatScore}`);
	return parts.length > 0 ? parts.join(' · ') : '—';
}
