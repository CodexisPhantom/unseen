import { describe, expect, it } from 'vitest';
import {
  createRequestBody,
  draftOf,
  draftProblem,
  INDEXER_KINDS,
  indexerKindLabel,
  indexerUrlPlaceholder,
  isIndexerKind,
  parseCategories,
  requestBody,
  testSummary,
  type Indexer,
  type IndexerKind
} from './indexers.js';

const indexer: Indexer = {
  id: 1,
  name: 'Tracker',
  kind: 'torznab',
  url: 'http://prowlarr:9696/1/api',
  hasApiKey: true,
  apiKeyMustBeReentered: false,
  categories: [2000, 2040],
  priority: 10,
  useProxy: true,
  enabled: false,
  tags: [],
  createdAt: '2026-11-25T00:00:00.000Z'
};

const usenetIndexer: Indexer = {
  ...indexer,
  id: 2,
  name: 'NZBgeek',
  kind: 'newznab',
  url: 'https://api.nzbgeek.info/api',
  priority: 15,
  enabled: true
};

describe('kind helpers', () => {
  it('lists supported kinds, labels, and URL placeholders', () => {
    expect(INDEXER_KINDS).toEqual([
      { value: 'torznab', label: 'Torznab' },
      { value: 'newznab', label: 'Newznab' }
    ]);
    expect(isIndexerKind('torznab')).toBe(true);
    expect(isIndexerKind('newznab')).toBe(true);
    expect(isIndexerKind('rss')).toBe(false);
    expect(indexerKindLabel('torznab')).toBe('Torznab');
    expect(indexerKindLabel('newznab')).toBe('Newznab');
    expect(indexerKindLabel('custom')).toBe('custom');
    expect(indexerUrlPlaceholder('torznab')).toContain('prowlarr');
    expect(indexerUrlPlaceholder('newznab')).toContain('nzbgeek');
  });
});

describe('draftOf', () => {
  it('starts a new indexer as torznab with movie categories, priority 25 and enabled', () => {
    expect(draftOf(null)).toMatchObject({
      kind: 'torznab',
      categories: '2000',
      priority: '25',
      enabled: true,
      apiKey: ''
    });
  });

  it('copies an existing indexer including kind and never the API key', () => {
    expect(draftOf(indexer)).toEqual({
      name: 'Tracker',
      kind: 'torznab',
      url: 'http://prowlarr:9696/1/api',
      apiKey: '',
      categories: '2000, 2040',
      priority: '10',
      enabled: false
    });
    expect(draftOf(usenetIndexer)).toEqual({
      name: 'NZBgeek',
      kind: 'newznab',
      url: 'https://api.nzbgeek.info/api',
      apiKey: '',
      categories: '2000, 2040',
      priority: '15',
      enabled: true
    });
    expect(draftOf({ ...indexer, kind: 'unknown' }).kind).toBe('torznab');
  });
});

describe('parseCategories', () => {
  it('reads ids separated by commas or spaces', () => {
    expect(parseCategories('2000, 2040 2045')).toEqual([2000, 2040, 2045]);
    expect(parseCategories('')).toEqual([]);
    expect(parseCategories('2000, movies')).toBeUndefined();
    expect(parseCategories('-1')).toBeUndefined();
  });
});

describe('draftProblem', () => {
  it('wants an API key only for a new indexer', () => {
    expect(draftProblem(draftOf(null), false)).not.toBe('');
    expect(draftProblem({ ...draftOf(null), apiKey: 'k' }, false)).toBe('');
    expect(draftProblem({ ...draftOf(null), kind: 'newznab', apiKey: 'k' }, false)).toBe('');
    expect(draftProblem(draftOf(indexer), true)).toBe('');
  });

  it('refuses an unknown kind, a priority outside 1 to 50 and bad categories', () => {
    const draft = draftOf(indexer);
    expect(draftProblem({ ...draft, kind: 'rss' as IndexerKind }, false)).not.toBe('');
    expect(draftProblem({ ...draft, priority: '0' }, true)).not.toBe('');
    expect(draftProblem({ ...draft, priority: '51' }, true)).not.toBe('');
    expect(draftProblem({ ...draft, priority: '2.5' }, true)).not.toBe('');
    expect(draftProblem({ ...draft, categories: 'x' }, true)).not.toBe('');
  });
});

describe('requestBody and createRequestBody', () => {
  it('keeps the stored API key when the field is empty', () => {
    const draft = draftOf(indexer);
    expect(requestBody(draft).apiKey).toBeUndefined();
    expect(requestBody({ ...draft, apiKey: ' token ' }).apiKey).toBe('token');
  });

  it('sends numbers and trims text, and includes kind on create', () => {
    expect(requestBody({ ...draftOf(indexer), name: ' A ', priority: '5' })).toMatchObject({
      name: 'A',
      priority: 5,
      categories: [2000, 2040],
      enabled: false
    });
    expect(
      createRequestBody({ ...draftOf(usenetIndexer), name: ' Geek ', apiKey: ' nzb-token ' })
    ).toEqual({
      name: 'Geek',
      kind: 'newznab',
      url: 'https://api.nzbgeek.info/api',
      apiKey: 'nzb-token',
      categories: [2000, 2040],
      priority: 15,
      enabled: true
    });
  });
});

describe('testSummary', () => {
  it('lists the search modes and counts the categories', () => {
    const result = {
      movieSearch: true,
      textSearch: false,
      imdbId: true,
      tmdbId: false,
      categories: [{ id: 2000, name: 'Movies' }]
    };
    expect(testSummary(result)).toBe('Connected. movie search, IMDb id; 1 category.');
    expect(testSummary({ ...result, movieSearch: false, imdbId: false, categories: [] })).toBe(
      'Connected. No search mode; 0 categories.'
    );
  });
});
