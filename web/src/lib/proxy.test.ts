import { describe, expect, it } from 'vitest';
import { changes, draftOf, summary, type ProxySettings } from './proxy.js';

const base: ProxySettings = {
  hasUrl: true,
  address: 'socks5h://proxy.example:1080',
  hasCredentials: true,
  urlMustBeReentered: false,
  bypassLocal: true,
  overridden: []
};

describe('changes', () => {
  it('sends nothing for an untouched form', () => {
    expect(changes(draftOf(base), base)).toEqual({});
  });

  it('sends a new URL, trimmed, and a changed switch', () => {
    const draft = { url: '  http://proxy:3128 ', bypassLocal: false };
    expect(changes(draft, base)).toEqual({ url: 'http://proxy:3128', bypassLocal: false });
  });

  it('leaves out a field that the environment sets', () => {
    const settings = { ...base, overridden: ['url', 'bypassLocal'] };
    const draft = { url: 'http://proxy:3128', bypassLocal: false };
    expect(changes(draft, settings)).toEqual({});
  });
});

describe('summary', () => {
  it('shows the address and whether credentials are stored', () => {
    expect(summary(base)).toBe('socks5h://proxy.example:1080 (with credentials)');
    expect(summary({ ...base, hasCredentials: false })).toBe('socks5h://proxy.example:1080');
  });

  it('is empty without a proxy and asks for the URL when it cannot be read', () => {
    expect(summary({ ...base, hasUrl: false, address: null })).toBe('');
    expect(summary({ ...base, hasUrl: false, address: null, urlMustBeReentered: true })).toContain(
      'Enter it again'
    );
  });
});
