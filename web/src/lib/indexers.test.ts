import { describe, expect, it } from 'vitest';
import {
  draftOf,
  draftProblem,
  parseCategories,
  requestBody,
  testSummary,
  type Indexer
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

describe('draftOf', () => {
  it('starts a new indexer with movie categories, priority 25 and enabled', () => {
    expect(draftOf(null)).toMatchObject({
      categories: '2000',
      priority: '25',
      enabled: true,
      apiKey: ''
    });
  });

  it('copies an existing indexer and never the API key', () => {
    expect(draftOf(indexer)).toEqual({
      name: 'Tracker',
      url: 'http://prowlarr:9696/1/api',
      apiKey: '',
      categories: '2000, 2040',
      priority: '10',
      enabled: false
    });
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
    expect(draftProblem(draftOf(indexer), true)).toBe('');
  });

  it('refuses a priority outside 1 to 50 and bad categories', () => {
    const draft = draftOf(indexer);
    expect(draftProblem({ ...draft, priority: '0' }, true)).not.toBe('');
    expect(draftProblem({ ...draft, priority: '51' }, true)).not.toBe('');
    expect(draftProblem({ ...draft, priority: '2.5' }, true)).not.toBe('');
    expect(draftProblem({ ...draft, categories: 'x' }, true)).not.toBe('');
  });
});

describe('requestBody', () => {
  it('keeps the stored API key when the field is empty', () => {
    const draft = draftOf(indexer);
    expect(requestBody(draft).apiKey).toBeUndefined();
    expect(requestBody({ ...draft, apiKey: ' secret ' }).apiKey).toBe('secret');
  });

  it('sends numbers and trims text', () => {
    expect(requestBody({ ...draftOf(indexer), name: ' A ', priority: '5' })).toMatchObject({
      name: 'A',
      priority: 5,
      categories: [2000, 2040],
      enabled: false
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
