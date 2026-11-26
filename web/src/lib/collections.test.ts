import { describe, expect, it } from 'vitest';
import {
	CollectionApiError,
	addedMovieCount,
	buildDefaultsPatch,
	buildMonitorPatch,
	collectionCountLabel,
	collectionLibraryState,
	collectionListCountLabel,
	draftChanged,
	draftOf,
	isTmdbProblem,
	matchesCollection,
	missingReleaseState,
	missingYear,
	monitorProblem,
	noCollectionFilters,
	problemMessage,
	releaseStateNames,
	sortCollectionMovies,
	sortMissingMovies,
	toCollectionError,
	todayIsoDate,
	visibleCollections,
	type Collection,
	type MissingMovie,
	type Problem
} from './collections.js';

function collection(patch: Partial<Collection>): Collection {
	return {
		id: 1,
		tmdbId: 2344,
		title: 'The Matrix Collection',
		sortTitle: 'matrix collection',
		overview: null,
		posterPath: null,
		monitored: false,
		rootFolderId: null,
		qualityProfileId: null,
		minimumAvailability: 'released',
		movieCount: 1,
		addedAt: '2026-01-01T00:00:00Z',
		updatedAt: '2026-01-01T00:00:00Z',
		...patch
	};
}

function missing(patch: Partial<MissingMovie>): MissingMovie {
	return {
		tmdbId: 604,
		title: 'The Matrix Reloaded',
		year: 2003,
		releaseDate: '2003-05-15',
		overview: null,
		posterPath: null,
		...patch
	};
}

const matrix = collection({
	id: 1,
	title: 'The Matrix Collection',
	sortTitle: 'matrix collection',
	monitored: true,
	movieCount: 3,
	addedAt: '2026-02-01T00:00:00Z'
});
const alien = collection({
	id: 2,
	title: 'Alien Collection',
	sortTitle: 'alien collection',
	monitored: false,
	movieCount: 1,
	addedAt: '2026-03-01T00:00:00Z'
});
const dune = collection({
	id: 3,
	title: 'Dune Collection',
	sortTitle: 'dune collection',
	monitored: true,
	movieCount: 2,
	addedAt: '2026-01-15T00:00:00Z'
});
const all = [matrix, alien, dune];

describe('matchesCollection and visibleCollections', () => {
	it('lets every collection through without filters', () => {
		expect(all.every((c) => matchesCollection(c, noCollectionFilters))).toBe(true);
	});

	it('filters by title text ignoring case and whitespace, and by monitored state', () => {
		expect(
			visibleCollections(all, { ...noCollectionFilters, text: '  MATRIX ' }, 'title', 'asc')
		).toEqual([matrix]);
		expect(
			visibleCollections(all, { ...noCollectionFilters, monitored: 'monitored' }, 'title', 'asc').map(
				(c) => c.id
			)
		).toEqual([3, 1]);
		expect(
			visibleCollections(all, { ...noCollectionFilters, monitored: 'unmonitored' }, 'title', 'asc').map(
				(c) => c.id
			)
		).toEqual([2]);
	});

	it('sorts by title, movie count and date added, breaking ties by sortTitle without mutating input', () => {
		const order = (key: 'title' | 'movies' | 'added', dir: 'asc' | 'desc') =>
			visibleCollections(all, noCollectionFilters, key, dir).map((c) => c.id);

		expect(order('title', 'asc')).toEqual([2, 3, 1]);
		expect(order('title', 'desc')).toEqual([1, 3, 2]);
		expect(order('movies', 'desc')).toEqual([1, 3, 2]);
		expect(order('added', 'desc')).toEqual([2, 1, 3]);

		const tied = collection({
			id: 4,
			title: 'Another Collection',
			sortTitle: 'another collection',
			movieCount: 3
		});
		const input = [matrix, tied];
		expect(visibleCollections(input, noCollectionFilters, 'movies', 'desc').map((c) => c.id)).toEqual(
			[4, 1]
		);
		expect(input).toEqual([matrix, tied]);
	});
});

describe('collectionCountLabel', () => {
	it('formats in-library count alone and with missing count', () => {
		expect(collectionCountLabel(2)).toBe('2 in library');
		expect(collectionCountLabel(0, null)).toBe('0 in library');
		expect(collectionCountLabel(2, 1)).toBe('2 in library · 1 missing');
		expect(collectionCountLabel(3, 0)).toBe('3 in library · 0 missing');
	});

	it('builds list summaries from the list payload count without needing collection details', () => {
		expect(collectionListCountLabel({ movieCount: 3 })).toBe('3 in library');
	});
});

describe('collectionLibraryState', () => {
	it('distinguishes a failed fallback request from a loaded empty collection', () => {
		expect(collectionLibraryState(false, false, true, 0)).toBe('error');
		expect(collectionLibraryState(false, false, false, 0)).toBe('empty');
		expect(collectionLibraryState(false, true, false, 0)).toBe('loading');
		expect(collectionLibraryState(true, false, false, 0)).toBe('empty');
	});
});

describe('missing movie release state and sorting', () => {
	it('formats UTC dates and parses missing years', () => {
		expect(todayIsoDate(new Date('2026-11-26T23:45:00Z'))).toBe('2026-11-26');
		expect(missingYear({ year: 2003, releaseDate: '2004-01-01' })).toBe(2003);
		expect(missingYear({ year: null, releaseDate: '2027-06-18' })).toBe(2027);
		expect(missingYear({ year: null, releaseDate: null })).toBeNull();
		expect(missingYear({ year: 0, releaseDate: 'bad' })).toBeNull();
	});

	it('classifies missing movies as released, announced or tba', () => {
		const today = '2026-11-26';
		expect(missingReleaseState({ releaseDate: '2003-05-15', year: 2003 }, today)).toBe('released');
		expect(missingReleaseState({ releaseDate: '2026-11-26', year: 2026 }, today)).toBe('released');
		expect(missingReleaseState({ releaseDate: '2026-11-27', year: 2026 }, today)).toBe('announced');
		expect(missingReleaseState({ releaseDate: '2028-05-01', year: 2028 }, today)).toBe('announced');
		expect(missingReleaseState({ releaseDate: null, year: null }, today)).toBe('tba');
		expect(missingReleaseState({ releaseDate: '', year: null }, today)).toBe('tba');
		// Year-only fallbacks when releaseDate is absent
		expect(missingReleaseState({ releaseDate: null, year: 2020 }, today)).toBe('released');
		expect(missingReleaseState({ releaseDate: null, year: 2028 }, today)).toBe('announced');
		expect(missingReleaseState({ releaseDate: null, year: 2026 }, today)).toBe('tba');

		expect(releaseStateNames.released).toBe('Released');
		expect(releaseStateNames.announced).toBe('Announced');
		expect(releaseStateNames.tba).toBe('TBA');
	});

	it('sorts missing movies chronologically with undated movies last', () => {
		const undatedB = missing({ tmdbId: 5, title: 'Zeta', year: null, releaseDate: null });
		const undatedA = missing({ tmdbId: 4, title: 'Alpha', year: null, releaseDate: null });
		const yearOnly = missing({ tmdbId: 3, title: 'Mid', year: 2024, releaseDate: null });
		const early = missing({ tmdbId: 1, title: 'First', year: 1999, releaseDate: '1999-03-30' });
		const late = missing({ tmdbId: 2, title: 'Second', year: 2024, releaseDate: '2024-03-01' });
		const tie = missing({ tmdbId: 6, title: 'Before First', year: 1999, releaseDate: '1999-03-30' });

		const input = [undatedB, yearOnly, late, early, undatedA, tie];
		expect(sortMissingMovies(input).map((m) => m.tmdbId)).toEqual([6, 1, 2, 3, 4, 5]);
		expect(input.map((m) => m.tmdbId)).toEqual([5, 3, 2, 1, 4, 6]);
	});

	it('sorts collection library movies chronologically by year, cinema date and sortTitle', () => {
		const m1 = { id: 1, year: 2003, inCinemas: '2003-11-05T00:00:00Z', sortTitle: 'matrix revolutions' };
		const m2 = { id: 2, year: 1999, inCinemas: '1999-03-31T00:00:00Z', sortTitle: 'matrix' };
		const m3 = { id: 3, year: 2003, inCinemas: '2003-05-15T00:00:00Z', sortTitle: 'matrix reloaded' };
		const m4 = { id: 4, year: 2003, inCinemas: null, sortTitle: 'animatrix' };

		expect(sortCollectionMovies([m1, m2, m3, m4]).map((m) => m.id)).toEqual([2, 3, 1, 4]);
	});
});

describe('monitoring draft, validation and patches', () => {
	it('builds a draft from saved values or falls back to defaults', () => {
		const unset = collection({
			rootFolderId: null,
			qualityProfileId: null,
			minimumAvailability: 'released'
		});
		expect(draftOf(unset)).toEqual({
			rootFolderId: null,
			qualityProfileId: null,
			minimumAvailability: 'released'
		});
		expect(draftOf(unset, { rootFolderId: 2, qualityProfileId: 5 })).toEqual({
			rootFolderId: 2,
			qualityProfileId: 5,
			minimumAvailability: 'released'
		});

		const saved = collection({
			rootFolderId: 1,
			qualityProfileId: 3,
			minimumAvailability: 'announced'
		});
		expect(draftOf(saved, { rootFolderId: 2, qualityProfileId: 5 })).toEqual({
			rootFolderId: 1,
			qualityProfileId: 3,
			minimumAvailability: 'announced'
		});
	});

	it('detects when editable defaults differ from the saved collection', () => {
		const saved = collection({
			rootFolderId: 1,
			qualityProfileId: 3,
			minimumAvailability: 'released'
		});
		expect(
			draftChanged(saved, { rootFolderId: 1, qualityProfileId: 3, minimumAvailability: 'released' })
		).toBe(false);
		expect(
			draftChanged(saved, { rootFolderId: 2, qualityProfileId: 3, minimumAvailability: 'released' })
		).toBe(true);
		expect(
			draftChanged(saved, { rootFolderId: 1, qualityProfileId: 4, minimumAvailability: 'released' })
		).toBe(true);
		expect(
			draftChanged(saved, { rootFolderId: 1, qualityProfileId: 3, minimumAvailability: 'inCinemas' })
		).toBe(true);
	});

	it('reports missing root folder or quality profile before enabling monitoring', () => {
		expect(
			monitorProblem({ rootFolderId: null, qualityProfileId: null, minimumAvailability: 'released' })
		).toContain('root folder and a quality profile');
		expect(
			monitorProblem({ rootFolderId: null, qualityProfileId: 1, minimumAvailability: 'released' })
		).toContain('root folder');
		expect(
			monitorProblem({ rootFolderId: 1, qualityProfileId: null, minimumAvailability: 'released' })
		).toContain('quality profile');
		expect(
			monitorProblem({ rootFolderId: 1, qualityProfileId: 2, minimumAvailability: 'released' })
		).toBe('');
	});

	it('builds monitor and default patches cleanly', () => {
		const draft = { rootFolderId: 2, qualityProfileId: 7, minimumAvailability: 'inCinemas' };
		expect(buildMonitorPatch(true, draft)).toEqual({
			monitored: true,
			rootFolderId: 2,
			qualityProfileId: 7,
			minimumAvailability: 'inCinemas'
		});
		expect(buildMonitorPatch(false, draft)).toEqual({ monitored: false });
		expect(buildDefaultsPatch(draft)).toEqual({
			rootFolderId: 2,
			qualityProfileId: 7,
			minimumAvailability: 'inCinemas'
		});
		expect(
			buildDefaultsPatch({ rootFolderId: null, qualityProfileId: null, minimumAvailability: 'released' })
		).toEqual({
			minimumAvailability: 'released'
		});
	});

	it('computes newly added movie counts safely', () => {
		expect(addedMovieCount(1, 4)).toBe(3);
		expect(addedMovieCount(2, 2)).toBe(0);
		expect(addedMovieCount(3, 1)).toBe(0);
	});
});

describe('API problem helpers', () => {
	it('picks detail, field error, title or fallback from a problem', () => {
		const base: Problem = {
			type: 'https://unseen.invalid/problems/test',
			title: 'TMDB refused the API key',
			status: 502,
			code: 'tmdb_key_rejected'
		};
		expect(problemMessage(undefined, 'Fallback')).toBe('Fallback');
		expect(problemMessage({ ...base, detail: 'Specific detail' }, 'Fallback')).toBe(
			'Specific detail'
		);
		expect(
			problemMessage(
				{ ...base, detail: null, errors: [{ field: 'rootFolderId', message: 'must be set' }] },
				'Fallback'
			)
		).toBe('rootFolderId: must be set');
		expect(problemMessage(base, 'Fallback')).toBe('TMDB refused the API key');
		expect(problemMessage({ ...base, title: '  ' }, 'Fallback')).toBe('Fallback');
	});

	it('recognizes TMDB problems and wraps API errors', () => {
		expect(isTmdbProblem('tmdb_not_configured', 409)).toBe(true);
		expect(isTmdbProblem('tmdb_key_rejected', 502)).toBe(true);
		expect(isTmdbProblem('tmdb_rate_limited', 502)).toBe(true);
		expect(isTmdbProblem('tmdb_unavailable', 502)).toBe(true);
		expect(isTmdbProblem(undefined, 502)).toBe(true);
		expect(isTmdbProblem('not_found', 404)).toBe(false);
		expect(isTmdbProblem('validation', 422)).toBe(false);

		const err = toCollectionError(
			{
				type: 'https://unseen.invalid/problems/tmdb',
				title: 'TMDB is not configured',
				status: 409,
				code: 'tmdb_not_configured',
				detail: 'Set the TMDB API key in settings before searching.'
			},
			409,
			'Failed'
		);
		expect(err).toBeInstanceOf(CollectionApiError);
		expect(err.status).toBe(409);
		expect(err.code).toBe('tmdb_not_configured');
		expect(err.message).toBe('Set the TMDB API key in settings before searching.');
	});
});
