import { afterEach, describe, expect, it, vi } from 'vitest';
import { basePath, normalizeBase, withBase } from './base-path.js';

afterEach(() => {
	vi.unstubAllGlobals();
});

describe('normalizeBase', () => {
	it('covers the empty and slash cases', () => {
		expect(normalizeBase('')).toBe('');
		expect(normalizeBase('/unseen/')).toBe('/unseen');
		expect(normalizeBase('unseen')).toBe('/unseen');
		expect(normalizeBase('  /unseen/  ')).toBe('/unseen');
	});
});

describe('basePath', () => {
	it('reads the injected meta tag', () => {
		vi.stubGlobal('document', {
			querySelector: () => ({ getAttribute: () => '/unseen' })
		});
		expect(basePath()).toBe('/unseen');
	});

	it('is empty without a document', () => {
		expect(basePath()).toBe('');
	});
});

describe('withBase', () => {
	it('prefixes API paths', () => {
		vi.stubGlobal('document', {
			querySelector: () => ({ getAttribute: () => '/unseen' })
		});
		expect(withBase('/api/v1/health')).toBe('/unseen/api/v1/health');
	});
});
