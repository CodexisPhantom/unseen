import type { components } from './api/schema.js';

export type MovieLookup = components['schemas']['MovieLookup'];

const CHOICES_KEY = 'unseenarr.add.choices';

/** What the add dialog remembers between visits. */
export interface AddChoices {
	rootFolderId: number | null;
	qualityProfileId: number | null;
}

type Store = Pick<Storage, 'getItem' | 'setItem'>;

function browserStore(): Store | null {
	try {
		return localStorage;
	} catch {
		return null;
	}
}

/** The last root folder and profile used, or nulls when none is stored or the store is unusable. */
export function loadChoices(store: Store | null = browserStore()): AddChoices {
	const none: AddChoices = { rootFolderId: null, qualityProfileId: null };
	try {
		const parsed: unknown = JSON.parse(store?.getItem(CHOICES_KEY) ?? 'null');
		if (typeof parsed !== 'object' || parsed === null) return none;
		const { rootFolderId, qualityProfileId } = parsed as Record<string, unknown>;
		return {
			rootFolderId: Number.isInteger(rootFolderId) ? (rootFolderId as number) : null,
			qualityProfileId: Number.isInteger(qualityProfileId) ? (qualityProfileId as number) : null
		};
	} catch {
		return none;
	}
}

export function saveChoices(choices: AddChoices, store: Store | null = browserStore()) {
	try {
		store?.setItem(CHOICES_KEY, JSON.stringify(choices));
	} catch {
		// The choice is not remembered. Nothing else depends on it.
	}
}

/** The remembered id when it is still offered, else the first offered id, else null. */
export function pickId(remembered: number | null, offered: readonly number[]): number | null {
	if (remembered !== null && offered.includes(remembered)) return remembered;
	return offered[0] ?? null;
}

/** A lookup result's year, from the release date when `year` is missing. */
export function lookupYear(result: MovieLookup): number | null {
	if (result.year != null) return result.year;
	const year = Number(result.releaseDate?.slice(0, 4));
	return Number.isInteger(year) && year > 0 ? year : null;
}
