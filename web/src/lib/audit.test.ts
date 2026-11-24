import { describe, expect, it } from 'vitest';
import { actionLabel, dataText, pageCount, startOfDay, targetLabel } from './audit.js';

describe('actionLabel', () => {
	it('turns an action into a label', () => {
		expect(actionLabel('role.updated')).toBe('Role updated');
		expect(actionLabel('auth.sign_in_failed')).toBe('Auth sign in failed');
	});
});

describe('targetLabel', () => {
	it('joins kind and id', () => {
		expect(targetLabel('role', '3')).toBe('role 3');
		expect(targetLabel('user', null)).toBe('user');
		expect(targetLabel(null, null)).toBe('');
	});
});

describe('dataText', () => {
	it('prints the data, or nothing', () => {
		expect(dataText(null)).toBe('');
		expect(dataText({ name: { old: 'a', new: 'b' } })).toContain('"old": "a"');
	});
});

describe('pageCount', () => {
	it('is at least one', () => {
		expect(pageCount(0, 50)).toBe(1);
		expect(pageCount(51, 50)).toBe(2);
	});
});

describe('startOfDay', () => {
	it('is empty without a day, and one day later with an offset of 1', () => {
		expect(startOfDay('')).toBeUndefined();
		const day = new Date(startOfDay('2026-11-24')!).getTime();
		const next = new Date(startOfDay('2026-11-24', 1)!).getTime();
		expect(next - day).toBeGreaterThanOrEqual(23 * 3600 * 1000);
		expect(next - day).toBeLessThanOrEqual(25 * 3600 * 1000);
	});
});
