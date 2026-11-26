export const LOG_LEVELS = ['all', 'error', 'warn', 'info', 'debug', 'trace'] as const;

export type LogLevelFilter = (typeof LOG_LEVELS)[number];

export type LogBadgeVariant = 'destructive' | 'default' | 'secondary' | 'outline';

/** Parses a select value into a valid log level filter, defaulting to `all`. */
export function parseLevelFilter(value: string | undefined): LogLevelFilter {
	const normalized = value?.trim().toLowerCase();
	if (normalized && (LOG_LEVELS as readonly string[]).includes(normalized)) {
		return normalized as LogLevelFilter;
	}
	return 'all';
}

/** Query parameter value for `level`, or `undefined` when showing all levels. */
export function levelQueryParam(level: LogLevelFilter): string | undefined {
	return level === 'all' ? undefined : level;
}

/** Human-readable label for a log level filter or entry level. */
export function levelLabel(level: string): string {
	switch (level.trim().toLowerCase()) {
		case 'all':
			return 'All levels';
		case 'error':
			return 'Error';
		case 'warn':
			return 'Warn';
		case 'info':
			return 'Info';
		case 'debug':
			return 'Debug';
		case 'trace':
			return 'Trace';
		default: {
			const trimmed = level.trim();
			return trimmed ? trimmed.charAt(0).toUpperCase() + trimmed.slice(1) : 'Info';
		}
	}
}

/** Maps a log severity level to a shadcn-svelte badge variant. */
export function levelBadgeVariant(level: string): LogBadgeVariant {
	switch (level.trim().toLowerCase()) {
		case 'error':
			return 'destructive';
		case 'warn':
			return 'default';
		case 'info':
			return 'secondary';
		default:
			return 'outline';
	}
}

/** Formats an ISO timestamp for display, falling back to the raw value if invalid. */
export function formatTimestamp(timestamp: string): string {
	if (!timestamp) return '';
	const parsed = new Date(timestamp);
	if (Number.isNaN(parsed.getTime())) return timestamp;
	return parsed.toLocaleString();
}

/** Formats a byte count into a compact human-readable string. */
export function formatBytes(bytes: number): string {
	if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
	const units = ['B', 'KB', 'MB', 'GB'];
	let value = bytes;
	let unit = 0;
	while (value >= 1024 && unit < units.length - 1) {
		value /= 1024;
		unit += 1;
	}
	return unit === 0 ? `${Math.round(value)} B` : `${value.toFixed(1)} ${units[unit]}`;
}

/** Returns true when `fields` is a non-empty plain object. */
export function hasFields(fields: unknown): boolean {
	if (fields === null || typeof fields !== 'object' || Array.isArray(fields)) {
		return false;
	}
	return Object.keys(fields).length > 0;
}

/** Pretty-prints structured log fields as JSON, or returns an empty string. */
export function fieldsText(fields: unknown): string {
	if (!hasFields(fields)) return '';
	return JSON.stringify(fields, null, 2);
}

/** Formats a daily log file option label with its name and size. */
export function fileOptionLabel(file: { name: string; sizeBytes: number }): string {
	return `${file.name} (${formatBytes(file.sizeBytes)})`;
}
