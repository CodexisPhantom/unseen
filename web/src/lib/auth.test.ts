import { describe, expect, it } from 'vitest';
import { safeRedirect, signInMessage } from './auth.js';

describe('signInMessage', () => {
	it('uses one message for a wrong password and an unknown account', () => {
		const wrong = signInMessage({
			code: 'INVALID_EMAIL_OR_PASSWORD',
			status: 401
		});
		expect(signInMessage({ status: 401 })).toBe(wrong);
		expect(wrong).not.toMatch(/exist|found|disabled/i);
	});

	it('names a disabled account', () => {
		expect(signInMessage({ code: 'FAILED_TO_CREATE_SESSION', status: 401 })).toMatch(/disabled/);
	});

	it('explains rate limiting and falls back for anything else', () => {
		expect(signInMessage({ status: 429 })).toMatch(/Too many/);
		expect(signInMessage({ status: 500 })).toBe('Sign-in failed. Try again.');
	});
});

describe('safeRedirect', () => {
	it('keeps a path of the app, with query and hash', () => {
		expect(safeRedirect('/library?sort=title#top')).toBe('/library?sort=title#top');
	});

	it('falls back for empty, foreign and malformed targets', () => {
		const targets = [
			null,
			'',
			'library',
			'//evil.example',
			'/\\evil.example',
			'https://evil.example'
		];
		for (const target of targets) {
			expect(safeRedirect(target)).toBe('/');
		}
	});

	it('does not loop through the sign-in or setup pages', () => {
		expect(safeRedirect('/login?redirectTo=%2F')).toBe('/');
		expect(safeRedirect('/setup')).toBe('/');
		expect(safeRedirect('/login', '/home')).toBe('/home');
	});
});
