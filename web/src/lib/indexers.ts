import type { components } from './api/schema.js';

export type Indexer = components['schemas']['Indexer'];
export type IndexerTestResult = components['schemas']['IndexerTestResult'];
export type IndexerKind = 'torznab' | 'newznab';

export const INDEXER_KINDS: readonly { value: IndexerKind; label: string }[] = [
  { value: 'torznab', label: 'Torznab' },
  { value: 'newznab', label: 'Newznab' }
];

export function isIndexerKind(value: string): value is IndexerKind {
  return value === 'torznab' || value === 'newznab';
}

export function indexerKindLabel(kind: string): string {
  if (kind === 'newznab') return 'Newznab';
  if (kind === 'torznab') return 'Torznab';
  return kind;
}

export function indexerUrlPlaceholder(kind: IndexerKind): string {
  return kind === 'newznab' ? 'https://api.nzbgeek.info/api' : 'http://prowlarr:9696/1/api';
}

export interface Draft {
  name: string;
  kind: IndexerKind;
  url: string;
  /** Empty keeps the stored API key when editing. */
  apiKey: string;
  /** Torznab or Newznab category ids, separated by commas or spaces. */
  categories: string;
  priority: string;
  enabled: boolean;
}

/** The form values for a new indexer, or for the indexer being edited. */
export function draftOf(indexer: Indexer | null): Draft {
  return {
    name: indexer?.name ?? '',
    kind: indexer && isIndexerKind(indexer.kind) ? indexer.kind : 'torznab',
    url: indexer?.url ?? '',
    apiKey: '',
    categories: indexer?.categories.join(', ') ?? '2000',
    priority: String(indexer?.priority ?? 25),
    enabled: indexer?.enabled ?? true
  };
}

/** The category ids in the text, or undefined when an entry is not a whole number. */
export function parseCategories(text: string): number[] | undefined {
  const entries = text.split(/[\s,]+/).filter(Boolean);
  if (!entries.every((entry) => /^\d+$/.test(entry))) return undefined;
  return entries.map(Number);
}

/** Why the form cannot be saved, or an empty string. */
export function draftProblem(draft: Draft, editing: boolean): string {
  if (!isIndexerKind(draft.kind)) {
    return 'Choose Torznab or Newznab.';
  }
  if (parseCategories(draft.categories) === undefined) {
    return 'Categories are whole numbers separated by commas, for example 2000, 2040.';
  }
  const priority = Number(draft.priority);
  if (!Number.isInteger(priority) || priority < 1 || priority > 50) {
    return 'Priority is a whole number from 1 to 50.';
  }
  if (!editing && draft.apiKey.trim() === '') return 'Enter the API key.';
  return '';
}

/**
 * The request body for an update. The API key is left out when it is empty,
 * which keeps the stored key on an edit. Call `draftProblem` first.
 */
export function requestBody(draft: Draft) {
  const apiKey = draft.apiKey.trim();
  return {
    name: draft.name.trim(),
    url: draft.url.trim(),
    apiKey: apiKey ? apiKey : undefined,
    categories: parseCategories(draft.categories) ?? [],
    priority: Number(draft.priority),
    enabled: draft.enabled
  };
}

/** The request body for creating an indexer, including kind and API key. */
export function createRequestBody(draft: Draft) {
  return {
    ...requestBody(draft),
    kind: draft.kind,
    apiKey: draft.apiKey.trim()
  };
}

/** One line for what a passed test learned. */
export function testSummary(result: IndexerTestResult): string {
  const searches = [
    result.movieSearch && 'movie search',
    result.textSearch && 'text search',
    result.imdbId && 'IMDb id',
    result.tmdbId && 'TMDB id'
  ].filter(Boolean);
  const count = result.categories.length;
  return `Connected. ${searches.length ? searches.join(', ') : 'No search mode'}; ${count} ${
    count === 1 ? 'category' : 'categories'
  }.`;
}
