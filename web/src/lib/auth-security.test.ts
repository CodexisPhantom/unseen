import { describe, expect, it } from 'vitest';
import { formatBackupCodes, needsTwoFactorChallenge, totpSecretFromUri } from './auth-security.js';

describe('totpSecretFromUri', () => {
	it('reads the setup key from an authenticator URI', () => {
		expect(
			totpSecretFromUri('otpauth://totp/Unseenarr%3Auser%40example.com?secret=ABC123&issuer=Unseenarr')
		).toBe('ABC123');
	});

	it('returns an empty string for an invalid URI or a missing key', () => {
		expect(totpSecretFromUri('not a URI')).toBe('');
		expect(totpSecretFromUri('otpauth://totp/Unseenarr:user?issuer=Unseenarr')).toBe('');
	});
});

describe('formatBackupCodes', () => {
	it('puts one recovery code on each line', () => {
		expect(formatBackupCodes(['ABC-123', 'DEF-456'])).toBe('ABC-123\nDEF-456');
	});
});

describe('needsTwoFactorChallenge', () => {
	it('recognizes the two-factor sign-in response', () => {
		expect(needsTwoFactorChallenge({ twoFactorRedirect: true })).toBe(true);
		expect(needsTwoFactorChallenge({ twoFactorRedirect: false })).toBe(false);
		expect(needsTwoFactorChallenge(null)).toBe(false);
	});
});
