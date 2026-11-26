import type { QueryClient } from '@tanstack/svelte-query';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
	EVENTS_URL,
	INITIAL_BACKOFF_MS,
	MAX_BACKOFF_MS,
	events,
	movieUpdatedQueryKeys
} from './events.svelte.js';
import { session } from './session.svelte.js';

type Listener = (event: MessageEvent<string>) => void;

class MockEventSource {
	static instances: MockEventSource[] = [];

	readonly url: string;
	onopen: ((event: Event) => void) | null = null;
	onerror: ((event: Event) => void) | null = null;
	closed = false;

	private listeners = new Map<string, Set<Listener>>();

	constructor(url: string) {
		this.url = url;
		MockEventSource.instances.push(this);
	}

	addEventListener(type: string, listener: Listener): void {
		let bucket = this.listeners.get(type);
		if (!bucket) {
			bucket = new Set();
			this.listeners.set(type, bucket);
		}
		bucket.add(listener);
	}

	close(): void {
		this.closed = true;
	}

	emitOpen(): void {
		this.onopen?.(new Event('open'));
	}

	emitError(): void {
		this.onerror?.(new Event('error'));
	}

	emitEvent(type: string, data: unknown): void {
		const serialized = typeof data === 'string' ? data : JSON.stringify(data);
		const event = { data: serialized } as MessageEvent<string>;
		for (const listener of this.listeners.get(type) ?? []) {
			listener(event);
		}
	}
}

function makeQueryClient() {
	const invalidateQueries = vi.fn().mockResolvedValue(undefined);
	return {
		client: { invalidateQueries } as unknown as QueryClient,
		invalidateQueries
	};
}

describe('events', () => {
	const originalEventSource = globalThis.EventSource;

	beforeEach(() => {
		vi.useFakeTimers();
		MockEventSource.instances = [];
		globalThis.EventSource = MockEventSource as unknown as typeof EventSource;
	});

	afterEach(() => {
		events.disconnect();
		vi.useRealTimers();
		vi.restoreAllMocks();
		globalThis.EventSource = originalEventSource;
	});

	it('opens a single EventSource and tracks connected state', () => {
		const { client, invalidateQueries } = makeQueryClient();

		events.connect(client);
		events.connect(client);

		expect(MockEventSource.instances).toHaveLength(1);
		expect(MockEventSource.instances[0]?.url).toBe(EVENTS_URL);
		expect(events.connected).toBe(false);

		MockEventSource.instances[0]?.emitOpen();
		expect(events.connected).toBe(true);
		expect(invalidateQueries).not.toHaveBeenCalled();

		events.disconnect();
		expect(events.connected).toBe(false);
		expect(MockEventSource.instances[0]?.closed).toBe(true);
	});

	it('invalidates matching queries and reloads session on events', () => {
		const { client, invalidateQueries } = makeQueryClient();
		const loadSpy = vi.spyOn(session, 'load').mockResolvedValue(null);

		events.connect(client);
		const source = MockEventSource.instances[0]!;
		source.emitOpen();

		source.emitEvent('download.progress', {
			id: 1,
			progress: 0.5,
			speed: 1024,
			eta: 60,
			state: 'downloading'
		});
		expect(invalidateQueries).toHaveBeenLastCalledWith({ queryKey: ['queue'] });

		source.emitEvent('download.state', {
			id: 1,
			oldState: 'downloading',
			newState: 'importing'
		});
		expect(invalidateQueries).toHaveBeenLastCalledWith({ queryKey: ['queue'] });

		source.emitEvent('movie.updated', { id: 42 });
		expect(invalidateQueries).toHaveBeenCalledWith({ queryKey: ['movies'] });
		expect(invalidateQueries).toHaveBeenCalledWith({ queryKey: ['movie', 42] });

		source.emitEvent('task.status', { taskName: 'rss_sync', status: 'running' });
		expect(invalidateQueries).toHaveBeenLastCalledWith({ queryKey: ['tasks'] });

		source.emitEvent('permissions.changed', {});
		expect(loadSpy).toHaveBeenCalledTimes(1);
	});

	it('maps a movie update to its movie query key', () => {
		expect(movieUpdatedQueryKeys({ id: 42 })).toEqual([['movies'], ['movie', 42]]);
	});

	it('ignores invalid event payloads without throwing', () => {
		const { client, invalidateQueries } = makeQueryClient();

		events.connect(client);
		const source = MockEventSource.instances[0]!;
		source.emitOpen();

		source.emitEvent('download.progress', '{not json');
		source.emitEvent('download.state', '{not json');
		source.emitEvent('movie.updated', '{not json');
		source.emitEvent('task.status', '{not json');

		expect(invalidateQueries).not.toHaveBeenCalled();
	});

	it('reconnects with exponential backoff and invalidates all queries on reconnect', () => {
		const { client, invalidateQueries } = makeQueryClient();

		events.connect(client);
		const first = MockEventSource.instances[0]!;
		first.emitOpen();
		expect(events.connected).toBe(true);

		// First error closes the source and waits INITIAL_BACKOFF_MS (1s).
		first.emitError();
		expect(events.connected).toBe(false);
		expect(first.closed).toBe(true);

		vi.advanceTimersByTime(INITIAL_BACKOFF_MS - 1);
		expect(MockEventSource.instances).toHaveLength(1);
		vi.advanceTimersByTime(1);
		expect(MockEventSource.instances).toHaveLength(2);

		// Second error backs off to 2s.
		const second = MockEventSource.instances[1]!;
		second.emitError();
		vi.advanceTimersByTime(INITIAL_BACKOFF_MS * 2 - 1);
		expect(MockEventSource.instances).toHaveLength(2);
		vi.advanceTimersByTime(1);
		expect(MockEventSource.instances).toHaveLength(3);

		// Repeated failures cap at MAX_BACKOFF_MS.
		for (let step = 0; step < 6; step++) {
			const current = MockEventSource.instances.at(-1)!;
			current.emitError();
			vi.advanceTimersByTime(MAX_BACKOFF_MS);
		}
		const countBeforeCapCheck = MockEventSource.instances.length;
		MockEventSource.instances.at(-1)!.emitError();
		vi.advanceTimersByTime(MAX_BACKOFF_MS - 1);
		expect(MockEventSource.instances).toHaveLength(countBeforeCapCheck);
		vi.advanceTimersByTime(1);
		expect(MockEventSource.instances).toHaveLength(countBeforeCapCheck + 1);

		// Reconnecting invalidates every query and resets backoff.
		const recovered = MockEventSource.instances.at(-1)!;
		recovered.emitOpen();
		expect(events.connected).toBe(true);
		expect(invalidateQueries).toHaveBeenCalledWith();

		// Next error after recovery starts back at INITIAL_BACKOFF_MS.
		const countAfterRecovery = MockEventSource.instances.length;
		recovered.emitError();
		vi.advanceTimersByTime(INITIAL_BACKOFF_MS);
		expect(MockEventSource.instances).toHaveLength(countAfterRecovery + 1);
	});

	it('cancels pending reconnect timers on disconnect', () => {
		const { client } = makeQueryClient();

		events.connect(client);
		MockEventSource.instances[0]!.emitError();
		events.disconnect();

		vi.advanceTimersByTime(MAX_BACKOFF_MS);
		expect(MockEventSource.instances).toHaveLength(1);
	});

	it('patches cached queue progress on download.progress without a reload', () => {
		const queue = [
			{
				id: 1,
				movieId: 42,
				movieTitle: 'The Matrix',
				movieYear: 1999,
				movieProfileId: null,
				downloadClientId: 1,
				downloadClientName: 'qBit',
				releaseTitle: 'The.Matrix.1999.1080p',
				qualityId: 7,
				size: 1000,
				progress: 0.1,
				speed: 100,
				eta: 90,
				state: 'downloading',
				error: null,
				grabbedAt: '2026-11-26T00:00:00.000Z'
			}
		];
		let cached = queue;
		const invalidateQueries = vi.fn().mockResolvedValue(undefined);
		const setQueryData = vi.fn((_key: unknown, updater: (prev: typeof queue) => typeof queue) => {
			cached = updater(cached);
		});
		const client = { invalidateQueries, setQueryData } as unknown as QueryClient;

		events.connect(client);
		const source = MockEventSource.instances[0]!;
		source.emitOpen();
		source.emitEvent('download.progress', {
			id: 1,
			progress: 0.65,
			speed: 5000,
			eta: 20,
			state: 'downloading'
		});

		expect(cached[0]?.progress).toBe(0.65);
		expect(cached[0]?.speed).toBe(5000);
		expect(cached[0]?.eta).toBe(20);
		expect(invalidateQueries).toHaveBeenCalledWith({ queryKey: ['queue'] });
	});

	it('is a safe no-op when EventSource is unavailable', () => {
		const { client } = makeQueryClient();
		globalThis.EventSource = undefined as unknown as typeof EventSource;

		events.connect(client);
		expect(events.connected).toBe(false);
		expect(MockEventSource.instances).toHaveLength(0);
	});
});
