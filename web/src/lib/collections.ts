import type { components } from './api/schema.js';

export type Collection = components['schemas']['Collection'];
export type CollectionDetail = components['schemas']['CollectionDetail'];
export type CollectionPatch = components['schemas']['CollectionPatch'];
export type MissingMovie = components['schemas']['MissingMovie'];
export type Problem = components['schemas']['Problem'];

export type CollectionMonitoredFilter = 'all' | 'monitored' | 'unmonitored';
export type CollectionSortKey = 'title' | 'movies' | 'added';
export type SortDirection = 'asc' | 'desc';
export type ReleaseState = 'released' | 'announced' | 'tba';

export interface CollectionFilters {
	text: string;
	monitored: CollectionMonitoredFilter;
}

export const noCollectionFilters: CollectionFilters = {
	text: '',
	monitored: 'all'
};

export const releaseStateNames: Record<ReleaseState, string> = {
	released: 'Released',
	announced: 'Announced',
	tba: 'TBA'
};

export interface CollectionDraft {
	rootFolderId: number | null;
	qualityProfileId: number | null;
	minimumAvailability: string;
}

export interface CollectionDefaults {
	rootFolderId: number | null;
	qualityProfileId: number | null;
}

const TMDB_PROBLEM_CODES = new Set([
	'tmdb_not_configured',
	'tmdb_key_rejected',
	'tmdb_rate_limited',
	'tmdb_unavailable'
]);

/** An API error carrying the HTTP status and machine-readable problem code. */
export class CollectionApiError extends Error {
	readonly status: number;
	readonly code: string | undefined;

	constructor(message: string, status: number, code?: string) {
		super(message);
		this.name = 'CollectionApiError';
		this.status = status;
		this.code = code;
	}
}

/** Extracts the most specific human-readable message from an RFC 9457 problem. */
export function problemMessage(problem: Problem | undefined, fallback: string): string {
	if (!problem) return fallback;
	const detail = problem.detail?.trim();
	if (detail) return detail;
	const fieldError = problem.errors?.find((entry) => entry.message.trim() !== '');
	if (fieldError) return `${fieldError.field}: ${fieldError.message}`;
	const title = problem.title?.trim();
	return title || fallback;
}

/** True when a problem comes from TMDB configuration or availability. */
export function isTmdbProblem(code: string | undefined, status?: number): boolean {
	return (code !== undefined && TMDB_PROBLEM_CODES.has(code)) || status === 502;
}

export function toCollectionError(
	problem: Problem | undefined,
	status: number,
	fallback: string
): CollectionApiError {
	return new CollectionApiError(problemMessage(problem, fallback), status, problem?.code);
}

export function matchesCollection(collection: Collection, filters: CollectionFilters): boolean {
	const text = filters.text.trim().toLowerCase();
	if (text && !collection.title.toLowerCase().includes(text)) {
		return false;
	}
	if (filters.monitored === 'monitored' && !collection.monitored) return false;
	if (filters.monitored === 'unmonitored' && collection.monitored) return false;
	return true;
}

const compareCollections: Record<CollectionSortKey, (a: Collection, b: Collection) => number> = {
	title: (a, b) => a.sortTitle.localeCompare(b.sortTitle),
	movies: (a, b) => a.movieCount - b.movieCount,
	added: (a, b) => a.addedAt.localeCompare(b.addedAt)
};

/** The collections that pass `filters`, in order. Never changes `collections`. */
export function visibleCollections(
	collections: readonly Collection[],
	filters: CollectionFilters,
	sort: CollectionSortKey,
	direction: SortDirection
): Collection[] {
	const sign = direction === 'asc' ? 1 : -1;
	return collections
		.filter((collection) => matchesCollection(collection, filters))
		.sort(
			(a, b) =>
				sign * compareCollections[sort](a, b) || a.sortTitle.localeCompare(b.sortTitle)
		);
}

/** Concise count summary for a collection card or header. */
export function collectionCountLabel(inLibrary: number, missing?: number | null): string {
	const libraryText = `${inLibrary} in library`;
	return missing == null ? libraryText : `${libraryText} · ${missing} missing`;
}

/** The list response has an in-library count, but no TMDB-backed missing count. */
export function collectionListCountLabel(collection: Pick<Collection, 'movieCount'>): string {
	return collectionCountLabel(collection.movieCount);
}

/** Today's UTC date as `YYYY-MM-DD`. */
export function todayIsoDate(now = new Date()): string {
	return now.toISOString().slice(0, 10);
}

/** The release year of a missing movie, from `year` or the release date prefix. */
export function missingYear(movie: Pick<MissingMovie, 'releaseDate' | 'year'>): number | null {
	if (movie.year != null && Number.isInteger(movie.year) && movie.year > 0) return movie.year;
	const parsed = Number(movie.releaseDate?.trim().slice(0, 4));
	return Number.isInteger(parsed) && parsed > 0 ? parsed : null;
}

/**
 * Classifies a missing movie's release state from its TMDB release date (or year)
 * against `today` (`YYYY-MM-DD`).
 */
export function missingReleaseState(
	movie: Pick<MissingMovie, 'releaseDate' | 'year'>,
	today = todayIsoDate()
): ReleaseState {
	const date = movie.releaseDate?.trim().slice(0, 10);
	if (date && /^\d{4}-\d{2}-\d{2}$/.test(date)) {
		return date <= today ? 'released' : 'announced';
	}
	const year = missingYear(movie);
	if (year !== null) {
		const currentYear = Number(today.slice(0, 4));
		if (year < currentYear) return 'released';
		if (year > currentYear) return 'announced';
	}
	return 'tba';
}

function missingDateKey(movie: Pick<MissingMovie, 'releaseDate' | 'year'>): string | null {
	const date = movie.releaseDate?.trim().slice(0, 10);
	if (date && /^\d{4}-\d{2}-\d{2}$/.test(date)) return date;
	const year = missingYear(movie);
	return year !== null ? `${String(year).padStart(4, '0')}-99-99` : null;
}

/** Missing movies in chronological release order, undated movies last. */
export function sortMissingMovies(movies: readonly MissingMovie[]): MissingMovie[] {
	return [...movies].sort((a, b) => {
		const dateA = missingDateKey(a);
		const dateB = missingDateKey(b);
		if (dateA && dateB) return dateA.localeCompare(dateB) || a.title.localeCompare(b.title);
		if (dateA) return -1;
		if (dateB) return 1;
		return a.title.localeCompare(b.title);
	});
}

/** In-library movies of a collection in chronological order by year, then title. */
export function sortCollectionMovies<
	T extends { year: number; inCinemas?: string | null; sortTitle: string }
>(movies: readonly T[]): T[] {
	return [...movies].sort((a, b) => {
		if (a.year !== b.year) return a.year - b.year;
		const dateA = a.inCinemas?.slice(0, 10) ?? '';
		const dateB = b.inCinemas?.slice(0, 10) ?? '';
		if (dateA && dateB) return dateA.localeCompare(dateB) || a.sortTitle.localeCompare(b.sortTitle);
		if (dateA) return -1;
		if (dateB) return 1;
		return a.sortTitle.localeCompare(b.sortTitle);
	});
}

/** Builds editable collection defaults, falling back to remembered choices when unset. */
export function draftOf(
	collection: Collection,
	defaults?: CollectionDefaults
): CollectionDraft {
	return {
		rootFolderId: collection.rootFolderId ?? defaults?.rootFolderId ?? null,
		qualityProfileId: collection.qualityProfileId ?? defaults?.qualityProfileId ?? null,
		minimumAvailability: collection.minimumAvailability || 'released'
	};
}

/** True when any editable default differs from the collection's saved values. */
export function draftChanged(collection: Collection, draft: CollectionDraft): boolean {
	return (
		draft.rootFolderId !== (collection.rootFolderId ?? null) ||
		draft.qualityProfileId !== (collection.qualityProfileId ?? null) ||
		draft.minimumAvailability !== collection.minimumAvailability
	);
}

/** Why enabling monitoring cannot proceed yet, or an empty string. */
export function monitorProblem(draft: CollectionDraft): string {
	if (draft.rootFolderId === null && draft.qualityProfileId === null) {
		return 'Choose a root folder and a quality profile before monitoring this collection.';
	}
	if (draft.rootFolderId === null) {
		return 'Choose a root folder before monitoring this collection.';
	}
	if (draft.qualityProfileId === null) {
		return 'Choose a quality profile before monitoring this collection.';
	}
	return '';
}

/** Request body for toggling collection monitoring. */
export function buildMonitorPatch(monitored: boolean, draft: CollectionDraft): CollectionPatch {
	if (!monitored) {
		return { monitored: false };
	}
	return {
		monitored: true,
		...(draft.rootFolderId !== null ? { rootFolderId: draft.rootFolderId } : {}),
		...(draft.qualityProfileId !== null ? { qualityProfileId: draft.qualityProfileId } : {}),
		minimumAvailability: draft.minimumAvailability
	};
}

/** Request body for saving collection defaults without toggling monitoring. */
export function buildDefaultsPatch(draft: CollectionDraft): CollectionPatch {
	return {
		...(draft.rootFolderId !== null ? { rootFolderId: draft.rootFolderId } : {}),
		...(draft.qualityProfileId !== null ? { qualityProfileId: draft.qualityProfileId } : {}),
		minimumAvailability: draft.minimumAvailability
	};
}

/** How many movies monitoring newly added, never negative. */
export function addedMovieCount(beforeCount: number, afterCount: number): number {
	return Math.max(0, afterCount - beforeCount);
}
