import type { components } from './api/schema.js';

export type ProxySettings = components['schemas']['ProxySettings'];

export interface Draft {
  /** Empty keeps the stored proxy URL. */
  url: string;
  bypassLocal: boolean;
}

export function draftOf(settings: ProxySettings): Draft {
  return { url: '', bypassLocal: settings.bypassLocal };
}

/**
 * The fields to send: only what changed, and never a field that an
 * environment variable sets. Empty means there is nothing to save.
 */
export function changes(draft: Draft, settings: ProxySettings) {
  const body: { url?: string; bypassLocal?: boolean } = {};
  const url = draft.url.trim();
  if (url && !settings.overridden.includes('url')) body.url = url;
  if (draft.bypassLocal !== settings.bypassLocal && !settings.overridden.includes('bypassLocal')) {
    body.bypassLocal = draft.bypassLocal;
  }
  return body;
}

/** One line about the proxy that is set, or an empty string. */
export function summary(settings: ProxySettings): string {
  if (settings.urlMustBeReentered) {
    return 'The stored proxy URL cannot be read. Enter it again.';
  }
  if (!settings.hasUrl || !settings.address) return '';
  return settings.hasCredentials ? `${settings.address} (with credentials)` : settings.address;
}
