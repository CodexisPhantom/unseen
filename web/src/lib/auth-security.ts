export function totpSecretFromUri(uri: string): string {
	try {
		return new URL(uri).searchParams.get('secret') ?? '';
	} catch {
		return '';
	}
}

export function formatBackupCodes(codes: string[]): string {
	return codes.join('\n');
}

export function needsTwoFactorChallenge(data: unknown): boolean {
	return (
		typeof data === 'object' &&
		data !== null &&
		'twoFactorRedirect' in data &&
		data.twoFactorRedirect === true
	);
}
