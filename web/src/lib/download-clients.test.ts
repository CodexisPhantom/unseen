import { describe, expect, it } from 'vitest';
import { draftOf, draftProblem, requestBody, type DownloadClient } from './download-clients.js';

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

describe('draftOf', () => {
  it('starts a new client with the Radarr category and enabled', () => {
    expect(draftOf(null)).toMatchObject({ category: 'radarr', enabled: true, password: '' });
  });

  it('copies an existing client and never the password', () => {
    expect(draftOf(client)).toEqual({
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
});

describe('requestBody', () => {
  it('leaves empty optional fields out of a new client', () => {
    const body = requestBody({ ...draftOf(null), name: ' A ', url: 'http://q' }, false);
    expect(body).toMatchObject({ name: 'A', username: undefined, remotePath: undefined });
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
});

describe('draftProblem', () => {
  it('wants both mapping paths or neither', () => {
    expect(draftProblem({ ...draftOf(null), remotePath: '/a' })).not.toBe('');
    expect(draftProblem({ ...draftOf(null), localPath: '/b' })).not.toBe('');
    expect(draftProblem({ ...draftOf(null), remotePath: '/a', localPath: '/b' })).toBe('');
    expect(draftProblem(draftOf(null))).toBe('');
  });
});
