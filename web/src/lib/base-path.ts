export function normalizeBase(value: string): string {
	const trimmed = value.trim().replace(/^\/+|\/+$/g, '');
	return trimmed === '' ? '' : `/${trimmed}`;
}

export function basePath(): string {
	if (typeof document === 'undefined') return '';
	const meta = document.querySelector('meta[name="unseenarr-base"]')?.getAttribute('content');
	return normalizeBase(meta ?? '');
}

export function withBase(path: string): string {
	return basePath() + path;
}
