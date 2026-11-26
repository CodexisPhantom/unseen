import { describe, expect, it } from 'vitest';
import {
	LOG_LEVELS,
	fieldsText,
	fileOptionLabel,
	formatBytes,
	formatTimestamp,
	hasFields,
	levelBadgeVariant,
	levelLabel,
	levelQueryParam,
	parseLevelFilter
} from './logs.js';

describe('parseLevelFilter', () => {
	it('accepts valid levels case-insensitively and falls back to all', () => {
		for (const level of LOG_LEVELS) {
			expect(parseLevelFilter(level)).toBe(level);
		}
		expect(parseLevelFilter('WARN')).toBe('warn');
		expect(parseLevelFilter(' error ')).toBe('error');
		expect(parseLevelFilter('')).toBe('all');
		expect(parseLevelFilter('verbose')).toBe('all');
		expect(parseLevelFilter(undefined)).toBe('all');
	});
});

describe('levelQueryParam', () => {
	it('omits all and passes concrete levels through', () => {
		expect(levelQueryParam('all')).toBeUndefined();
		expect(levelQueryParam('warn')).toBe('warn');
		expect(levelQueryParam('error')).toBe('error');
	});
});

describe('levelLabel', () => {
	it('returns readable labels for known and unknown levels', () => {
		expect(levelLabel('all')).toBe('All levels');
		expect(levelLabel('error')).toBe('Error');
		expect(levelLabel('warn')).toBe('Warn');
		expect(levelLabel('info')).toBe('Info');
		expect(levelLabel('debug')).toBe('Debug');
		expect(levelLabel('trace')).toBe('Trace');
		expect(levelLabel('custom')).toBe('Custom');
		expect(levelLabel('')).toBe('Info');
	});
});

describe('levelBadgeVariant', () => {
	it('maps levels to badge variants', () => {
		expect(levelBadgeVariant('error')).toBe('destructive');
		expect(levelBadgeVariant('warn')).toBe('default');
		expect(levelBadgeVariant('info')).toBe('secondary');
		expect(levelBadgeVariant('debug')).toBe('outline');
		expect(levelBadgeVariant('trace')).toBe('outline');
	});
});

describe('formatTimestamp', () => {
	it('formats valid ISO timestamps and keeps invalid ones intact', () => {
		expect(formatTimestamp('')).toBe('');
		expect(formatTimestamp('not-a-date')).toBe('not-a-date');
		const formatted = formatTimestamp('2026-11-26T08:00:00.000Z');
		expect(formatted.length).toBeGreaterThan(0);
		expect(formatted).not.toBe('Invalid Date');
	});
});

describe('formatBytes', () => {
	it('formats byte sizes across units', () => {
		expect(formatBytes(0)).toBe('0 B');
		expect(formatBytes(-5)).toBe('0 B');
		expect(formatBytes(512)).toBe('512 B');
		expect(formatBytes(1536)).toBe('1.5 KB');
		expect(formatBytes(2 * 1024 * 1024)).toBe('2.0 MB');
		expect(formatBytes(3 * 1024 * 1024 * 1024)).toBe('3.0 GB');
	});
});

describe('fields helpers', () => {
	it('detects and formats non-empty structured fields', () => {
		expect(hasFields(null)).toBe(false);
		expect(hasFields(undefined)).toBe(false);
		expect(hasFields([])).toBe(false);
		expect(hasFields({})).toBe(false);
		expect(hasFields({ key: 'value' })).toBe(true);

		expect(fieldsText({})).toBe('');
		expect(fieldsText(null)).toBe('');
		expect(fieldsText({ host: 'example.test', retries: 2 })).toContain('"host": "example.test"');
	});
});

describe('fileOptionLabel', () => {
	it('combines file name and formatted size', () => {
		expect(fileOptionLabel({ name: 'unseen-2026-11-26.log', sizeBytes: 2048 })).toBe(
			'unseen-2026-11-26.log (2.0 KB)'
		);
	});
});
