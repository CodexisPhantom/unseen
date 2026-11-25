import type { components } from './api/schema.js';

export type DownloadClient = components['schemas']['DownloadClient'];

export interface Draft {
  name: string;
  url: string;
  username: string;
  /** Empty keeps the stored password when editing. */
  password: string;
  /** Empty keeps the stored API key when editing. */
  apiKey: string;
  category: string;
  remotePath: string;
  localPath: string;
  enabled: boolean;
}

/** The form values for a new client, or for the client being edited. */
export function draftOf(client: DownloadClient | null): Draft {
  return {
    name: client?.name ?? '',
    url: client?.url ?? '',
    username: client?.username ?? '',
    password: '',
    apiKey: '',
    category: client?.category ?? 'radarr',
    remotePath: client?.remotePath ?? '',
    localPath: client?.localPath ?? '',
    enabled: client?.enabled ?? true
  };
}

/** Why the mapping cannot be saved, or an empty string. */
export function draftProblem(draft: Draft): string {
  const remote = draft.remotePath.trim() !== '';
  const local = draft.localPath.trim() !== '';
  return remote === local ? '' : 'Set both paths of the remote path mapping, or neither.';
}

/**
 * The request body. A new client leaves empty optional fields out. An edit
 * sends them as empty strings, which clear the stored value, and leaves the
 * password and the API key out when they are empty, which keeps the stored
 * ones.
 */
export function requestBody(draft: Draft, editing: boolean) {
  const text = (value: string) => {
    const trimmed = value.trim();
    return editing || trimmed ? trimmed : undefined;
  };
  return {
    name: draft.name.trim(),
    url: draft.url.trim(),
    username: text(draft.username),
    password: draft.password ? draft.password : undefined,
    apiKey: draft.apiKey ? draft.apiKey : undefined,
    category: draft.category.trim(),
    remotePath: text(draft.remotePath),
    localPath: text(draft.localPath),
    enabled: draft.enabled
  };
}
