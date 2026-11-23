const wrongCredentials = 'Wrong email or password.';

// Codes of the auth library. `FAILED_TO_CREATE_SESSION` is what a disabled
// user gets, after the password was right (crates/api/tests/auth.rs).
const messages: Record<string, string> = {
	INVALID_EMAIL_OR_PASSWORD: wrongCredentials,
	FAILED_TO_CREATE_SESSION: 'This account is disabled. Ask the Owner to enable it.'
};

/**
 * The message for a failed sign-in. Unknown accounts and wrong passwords
 * share one message, so the form does not reveal which accounts exist.
 */
export function signInMessage(error: { code?: string; status: number }): string {
	if (error.code && messages[error.code]) return messages[error.code];
	if (error.status === 401) return wrongCredentials;
	if (error.status === 429) return 'Too many attempts. Wait a moment and try again.';
	return 'Sign-in failed. Try again.';
}

/**
 * `redirectTo` comes from the URL, so only a path of this app is allowed.
 * Anything else, and the sign-in and setup pages themselves, leads to `fallback`.
 */
export function safeRedirect(target: string | null, fallback = '/'): string {
	if (!target?.startsWith('/') || target.startsWith('//') || target.startsWith('/\\')) {
		return fallback;
	}
	const path = target.split(/[?#]/, 1)[0];
	return path === '/login' || path === '/setup' ? fallback : target;
}
