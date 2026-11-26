import { describe, expect, it } from 'vitest';
import {
	downloadBadgeLabel,
	downloadsForMovie,
	formatEta,
	formatSpeed,
	patchQueueProgress,
	progressPercent,
	stateBadgeVariant,
	stateLabel,
	type QueueItem
} from './queue.js';

function sampleItem(overrides: Partial<QueueItem> = {}): QueueItem {
	return {
		id: 1,
		movieId: 42,
		movieTitle: 'The Matrix',
		movieYear: 1999,
		movieProfileId: 7,
		downloadClientId: 3,
		downloadClientName: 'qBit',
		releaseTitle: 'The.Matrix.1999.1080p.BluRay',
		qualityId: 7,
		customFormatScore: 0,
		size: 10_000_000_000,
		progress: 0.25,
		speed: 1_048_576,
		eta: 180,
		state: 'downloading',
		error: null,
		grabbedAt: '2026-11-26T12:00:00.000Z',
		...overrides
	};
}

describe('queue helpers', () => {
	it('formats progress percentages with clamping', () => {
		expect(progressPercent(0)).toBe(0);
		expect(progressPercent(0.426)).toBe(43);
		expect(progressPercent(1)).toBe(100);
		expect(progressPercent(-0.5)).toBe(0);
		expect(progressPercent(1.5)).toBe(100);
		expect(progressPercent(Number.NaN)).toBe(0);
	});

	it('formats download speeds', () => {
		expect(formatSpeed(0)).toBe('—');
		expect(formatSpeed(-10)).toBe('—');
		expect(formatSpeed(null)).toBe('—');
		expect(formatSpeed(512)).toBe('512 B/s');
		expect(formatSpeed(1024 * 1024 * 12.5)).toBe('12.5 MB/s');
	});

	it('formats ETA values', () => {
		expect(formatEta(null)).toBe('—');
		expect(formatEta(-1)).toBe('—');
		expect(formatEta(0)).toBe('0s');
		expect(formatEta(45)).toBe('45s');
		expect(formatEta(120)).toBe('2m');
		expect(formatEta(125)).toBe('2m 5s');
		expect(formatEta(3600)).toBe('1h');
		expect(formatEta(3665)).toBe('1h 1m');
	});

	it('maps states to labels and badge variants', () => {
		expect(stateLabel('queued')).toBe('Queued');
		expect(stateLabel('downloading')).toBe('Downloading');
		expect(stateLabel('paused')).toBe('Paused');
		expect(stateLabel('completed')).toBe('Completed');
		expect(stateLabel('importing')).toBe('Importing');
		expect(stateLabel('importFailed')).toBe('Import failed');
		expect(stateLabel('custom')).toBe('custom');

		expect(stateBadgeVariant('downloading')).toBe('default');
		expect(stateBadgeVariant('importing')).toBe('default');
		expect(stateBadgeVariant('completed')).toBe('secondary');
		expect(stateBadgeVariant('importFailed')).toBe('destructive');
		expect(stateBadgeVariant('queued')).toBe('outline');
	});

	it('patches live progress from SSE without mutating original items', () => {
		const first = sampleItem({ id: 1, progress: 0.2, speed: 500, eta: 200, state: 'downloading' });
		const second = sampleItem({ id: 2, movieId: 99, progress: 0.1 });
		const items = [first, second];

		const updated = patchQueueProgress(items, {
			id: 1,
			progress: 0.75,
			speed: 2_000_000,
			eta: 45,
			state: 'downloading'
		});

		expect(updated[0]?.progress).toBe(0.75);
		expect(updated[0]?.speed).toBe(2_000_000);
		expect(updated[0]?.eta).toBe(45);
		expect(updated[1]).toBe(second);
		expect(first.progress).toBe(0.2);

		// Partial payloads preserve existing speed/eta/state.
		const partial = patchQueueProgress(updated, { id: 1, progress: 0.9 });
		expect(partial[0]?.progress).toBe(0.9);
		expect(partial[0]?.speed).toBe(2_000_000);
		expect(partial[0]?.eta).toBe(45);
		expect(partial[0]?.state).toBe('downloading');
	});

	it('filters downloads for a movie and builds badge labels', () => {
		const a = sampleItem({ id: 1, movieId: 42, progress: 0.64, state: 'downloading' });
		const b = sampleItem({ id: 2, movieId: 99, state: 'importing' });
		expect(downloadsForMovie([a, b], 42)).toEqual([a]);
		expect(downloadsForMovie([a, b], 7)).toEqual([]);

		expect(downloadBadgeLabel(a)).toBe('Downloading · 64%');
		expect(downloadBadgeLabel(b)).toBe('Importing');
	});
});
