import { afterEach, describe, expect, it, vi } from 'vitest';
import { basePath, normalizeBase, withBase } from './base-path.js';

afterEach(() => {
	vi.unstubAllGlobals();
});

describe('normalizeBase', () => {
	it('covers the empty and slash cases', () => {
		expect(normalizeBase('')).toBe('');
		expect(normalizeBase('/unseenarr/')).toBe('/unseenarr');
		expect(normalizeBase('unseenarr')).toBe('/unseenarr');
		expect(normalizeBase('  /unseenarr/  ')).toBe('/unseenarr');
	});
});

describe('basePath', () => {
	it('reads the injected meta tag', () => {
		vi.stubGlobal('document', {
			querySelector: () => ({ getAttribute: () => '/unseenarr' })
		});
		expect(basePath()).toBe('/unseenarr');
	});

	it('is empty without a document', () => {
		expect(basePath()).toBe('');
	});
});

describe('withBase', () => {
	it('prefixes API paths', () => {
		vi.stubGlobal('document', {
			querySelector: () => ({ getAttribute: () => '/unseenarr' })
		});
		expect(withBase('/api/v1/health')).toBe('/unseenarr/api/v1/health');
	});
});
