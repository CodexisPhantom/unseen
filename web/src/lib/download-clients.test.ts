import { describe, expect, it } from 'vitest';
import { draftOf, draftProblem, kindName, requestBody, type DownloadClient } from './download-clients.js';

const client: DownloadClient = {
  id: 1,
  name: 'qBittorrent',
  kind: 'qbittorrent',
  url: 'http://qbittorrent:8080',
  username: 'admin',
  hasPassword: true,
  passwordMustBeReentered: false,
  hasApiKey: false,
  apiKeyMustBeReentered: false,
  category: 'movies',
  remotePath: null,
  localPath: null,
  enabled: false,
  tags: [],
  createdAt: '2026-11-25T00:00:00.000Z'
};

const sabnzbd: DownloadClient = { ...client, id: 2, name: 'SAB', kind: 'sabnzbd' };

describe('draftOf', () => {
  it('starts a new client with the Radarr category and enabled', () => {
    expect(draftOf(null)).toMatchObject({ kind: 'qbittorrent', category: 'radarr', enabled: true, password: '' });
  });

  it('copies an existing client and never the password', () => {
    expect(draftOf(client)).toEqual({
      kind: 'qbittorrent',
      name: 'qBittorrent',
      url: 'http://qbittorrent:8080',
      username: 'admin',
      password: '',
      apiKey: '',
      category: 'movies',
      remotePath: '',
      localPath: '',
      enabled: false
    });
  });

  it('copies a sabnzbd kind', () => {
    expect(draftOf(sabnzbd).kind).toBe('sabnzbd');
  });
});

describe('requestBody', () => {
  it('leaves empty optional fields out of a new client', () => {
    const body = requestBody({ ...draftOf(null), name: ' A ', url: 'http://q' }, false);
    expect(body).toMatchObject({ name: 'A', kind: 'qbittorrent', username: undefined, remotePath: undefined });
    expect(body.password).toBeUndefined();
  });

  it('clears fields on an edit and keeps the password when it is empty', () => {
    const draft = { ...draftOf(client), username: '', remotePath: '', localPath: '' };
    const body = requestBody(draft, true);
    expect(body).toMatchObject({ username: '', remotePath: '', localPath: '' });
    expect(body.password).toBeUndefined();
    expect(requestBody({ ...draft, password: 'new' }, true).password).toBe('new');
    expect(body.apiKey).toBeUndefined();
    expect(requestBody({ ...draft, apiKey: 'qbt_new' }, true).apiKey).toBe('qbt_new');
  });

  it('builds a new sabnzbd body without credentials and with the kind', () => {
    const body = requestBody(
      { ...draftOf(null), kind: 'sabnzbd', name: 'S', url: 'http://s', apiKey: 'key1' },
      false
    );
    expect(body).toMatchObject({ kind: 'sabnzbd', username: undefined, password: undefined, apiKey: 'key1' });
  });

  it('leaves the kind out of an edited sabnzbd body', () => {
    const body = requestBody({ ...draftOf(sabnzbd), apiKey: 'key1' }, true);
    expect(body.kind).toBeUndefined();
    expect(body.username).toBeUndefined();
    expect(body.password).toBeUndefined();
    expect(body.apiKey).toBe('key1');
  });
});

describe('draftProblem', () => {
  it('wants both mapping paths or neither', () => {
    expect(draftProblem({ ...draftOf(null), remotePath: '/a' }, false)).not.toBe('');
    expect(draftProblem({ ...draftOf(null), localPath: '/b' }, false)).not.toBe('');
    expect(draftProblem({ ...draftOf(null), remotePath: '/a', localPath: '/b' }, false)).toBe('');
    expect(draftProblem(draftOf(null), false)).toBe('');
  });

  it('needs an API key for a new sabnzbd client only', () => {
    expect(draftProblem({ ...draftOf(null), kind: 'sabnzbd', apiKey: '' }, false)).toBe(
      'SABnzbd needs its API key.'
    );
    expect(
      draftProblem({ ...draftOf(null), kind: 'sabnzbd', apiKey: 'key1' }, false)
    ).toBe('');
    expect(draftProblem({ ...draftOf(sabnzbd), apiKey: '' }, true)).toBe('');
    expect(draftProblem({ ...draftOf(null), apiKey: '' }, false)).toBe('');
  });

  it('still enforces the mapping rule', () => {
    expect(
      draftProblem({ ...draftOf(null), kind: 'sabnzbd', apiKey: 'k', remotePath: '/a' }, false)
    ).not.toBe('');
  });
});

describe('kindName', () => {
  it('names both kinds', () => {
    expect(kindName('sabnzbd')).toBe('SABnzbd');
    expect(kindName('qbittorrent')).toBe('qBittorrent');
    expect(kindName('other')).toBe('qBittorrent');
  });
});
