import type { components } from './api/schema.js';
import type { DownloadProgressEvent } from './events.svelte.js';
import { formatBytes } from './root-folders.js';

export type QueueItem = components['schemas']['QueueItem'];

const STATE_LABELS: Record<string, string> = {
	queued: 'Queued',
	downloading: 'Downloading',
	paused: 'Paused',
	completed: 'Completed',
	importing: 'Importing',
	importFailed: 'Import failed'
};

export type BadgeVariant = 'default' | 'secondary' | 'destructive' | 'outline';

/** Human-readable label for a queue download state. */
export function stateLabel(state: string): string {
	return STATE_LABELS[state] ?? state;
}

/** Visual badge variant for a queue download state. */
export function stateBadgeVariant(state: string): BadgeVariant {
	switch (state) {
		case 'downloading':
		case 'importing':
			return 'default';
		case 'completed':
			return 'secondary';
		case 'importFailed':
			return 'destructive';
		default:
			return 'outline';
	}
}

/** Clamps a `0..1` progress fraction to an integer percentage `0..100`. */
export function progressPercent(progress: number): number {
	if (!Number.isFinite(progress)) return 0;
	return Math.round(Math.min(1, Math.max(0, progress)) * 100);
}

/** Formats bytes per second as e.g. `12.5 MB/s`, or a dash when idle. */
export function formatSpeed(bytesPerSecond: number | null | undefined): string {
	if (
		bytesPerSecond === null ||
		bytesPerSecond === undefined ||
		!Number.isFinite(bytesPerSecond) ||
		bytesPerSecond <= 0
	) {
		return '—';
	}
	return `${formatBytes(bytesPerSecond)}/s`;
}

/** Formats remaining seconds as e.g. `1h 12m`, `4m 20s`, or `35s`. */
export function formatEta(seconds: number | null | undefined): string {
	if (
		seconds === null ||
		seconds === undefined ||
		!Number.isFinite(seconds) ||
		seconds < 0
	) {
		return '—';
	}
	const total = Math.round(seconds);
	if (total < 60) return `${total}s`;
	const minutes = Math.floor(total / 60);
	const remSeconds = total % 60;
	if (minutes < 60) {
		return remSeconds > 0 ? `${minutes}m ${remSeconds}s` : `${minutes}m`;
	}
	const hours = Math.floor(minutes / 60);
	const remMinutes = minutes % 60;
	return remMinutes > 0 ? `${hours}h ${remMinutes}m` : `${hours}h`;
}

/**
 * Applies an incoming `download.progress` SSE payload to a cached queue list
 * without mutating the input array.
 */
export function patchQueueProgress(
	items: readonly QueueItem[],
	event: DownloadProgressEvent
): QueueItem[] {
	let changed = false;
	const next = items.map((item) => {
		if (item.id !== event.id) return item;
		changed = true;
		return {
			...item,
			progress: event.progress,
			speed: event.speed ?? item.speed,
			eta: event.eta !== undefined ? event.eta : item.eta,
			state: event.state ?? item.state
		};
	});
	return changed ? next : [...items];
}

/** Active downloads belonging to a single movie. */
export function downloadsForMovie(items: readonly QueueItem[], movieId: number): QueueItem[] {
	return items.filter((item) => item.movieId === movieId);
}

/** Short label for the movie detail page download badge. */
export function downloadBadgeLabel(item: QueueItem): string {
	const label = stateLabel(item.state);
	if (item.state === 'downloading' || item.state === 'paused') {
		return `${label} · ${progressPercent(item.progress)}%`;
	}
	return label;
}
