import type { components } from './api/schema.js';

export type DownloadClient = components['schemas']['DownloadClient'];

export interface Draft {
  kind: string;
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
    kind: client?.kind ?? 'qbittorrent',
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

export function kindName(kind: string): string {
  return kind === 'sabnzbd' ? 'SABnzbd' : 'qBittorrent';
}

/** Why the mapping cannot be saved, or an empty string. */
export function draftProblem(draft: Draft, editing: boolean): string {
  const remote = draft.remotePath.trim() !== '';
  const local = draft.localPath.trim() !== '';
  if (remote !== local) return 'Set both paths of the remote path mapping, or neither.';
  if (!editing && draft.kind === 'sabnzbd' && draft.apiKey.trim() === '') {
    return 'SABnzbd needs its API key.';
  }
  return '';
}

/**
 * The request body. A new client leaves empty optional fields out. An edit
 * sends them as empty strings, which clear the stored value, and leaves the
 * password and the API key out when they are empty, which keeps the stored
 * ones. The kind is sent only on create: it is immutable on edit.
 */
export function requestBody(draft: Draft, editing: boolean) {
  const text = (value: string) => {
    const trimmed = value.trim();
    return editing || trimmed ? trimmed : undefined;
  };
  const isSab = draft.kind === 'sabnzbd';
  return {
    name: draft.name.trim(),
    url: draft.url.trim(),
    kind: editing ? undefined : draft.kind,
    username: isSab ? undefined : text(draft.username),
    password: isSab ? undefined : draft.password ? draft.password : undefined,
    apiKey: draft.apiKey ? draft.apiKey : undefined,
    category: draft.category.trim(),
    remotePath: text(draft.remotePath),
    localPath: text(draft.localPath),
    enabled: draft.enabled
  };
}
