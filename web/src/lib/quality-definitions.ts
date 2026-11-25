import type { components } from './api/schema.js';

export type QualityDefinition = components['schemas']['QualityDefinition'];

/** What the inputs hold: text, so that an empty field can mean "none". */
export interface Draft {
  title: string;
  minSize: string;
  preferredSize: string;
  maxSize: string;
}

const text = (value: number | null | undefined) => (value === null || value === undefined ? '' : String(value));

export function draftOf(definition: QualityDefinition): Draft {
  return {
    title: definition.title,
    minSize: text(definition.minSize),
    preferredSize: text(definition.preferredSize),
    maxSize: text(definition.maxSize)
  };
}

/** A number, null for an empty field, NaN for text that is not a number. */
function parse(value: string): number | null {
  const trimmed = value.trim();
  return trimmed === '' ? null : Number(trimmed);
}

/** Why the limits cannot be saved, or an empty string. Same rules as the API. */
export function problem(draft: Draft): string {
  const min = parse(draft.minSize);
  const preferred = parse(draft.preferredSize);
  const max = parse(draft.maxSize);
  if (draft.title.trim() === '') return 'The name must not be empty.';
  if (min === null || !Number.isFinite(min) || min < 0) return 'The minimum must be 0 or more.';
  if (max !== null && (!Number.isFinite(max) || max <= 0)) {
    return 'The maximum must be more than 0, or empty for no limit.';
  }
  if (max !== null && max < min) return 'The maximum must not be below the minimum.';
  if (preferred !== null && (!Number.isFinite(preferred) || preferred < min)) {
    return 'The preferred size must not be below the minimum.';
  }
  if (preferred !== null && max !== null && preferred > max) {
    return 'The preferred size must not be above the maximum.';
  }
  return '';
}

/** Only the fields that changed. An emptied optional field is sent as null. */
export function changes(draft: Draft, definition: QualityDefinition) {
  const body: {
    title?: string;
    minSize?: number;
    preferredSize?: number | null;
    maxSize?: number | null;
  } = {};
  if (draft.title.trim() !== definition.title) body.title = draft.title.trim();
  const min = parse(draft.minSize);
  if (min !== null && min !== definition.minSize) body.minSize = min;
  const preferred = parse(draft.preferredSize);
  if (preferred !== (definition.preferredSize ?? null)) body.preferredSize = preferred;
  const max = parse(draft.maxSize);
  if (max !== (definition.maxSize ?? null)) body.maxSize = max;
  return body;
}
