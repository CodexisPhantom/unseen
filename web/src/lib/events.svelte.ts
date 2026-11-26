import type { QueryClient } from '@tanstack/svelte-query';
import { session } from './session.svelte.js';

export const EVENTS_URL = '/api/v1/events';
export const INITIAL_BACKOFF_MS = 1_000;
export const MAX_BACKOFF_MS = 30_000;

export interface DownloadProgressEvent {
	id: number;
	progress: number;
	speed: number;
	eta: number | null;
	state: string;
}

export interface DownloadStateEvent {
	id: number;
	oldState: string;
	newState: string;
}

export interface MovieUpdatedEvent {
	movieId: number;
}

export interface TaskStatusEvent {
	taskName: string;
	status: string;
}

function parseJson<T>(raw: unknown): T | null {
	if (typeof raw !== 'string') return null;
	try {
		return JSON.parse(raw) as T;
	} catch {
		return null;
	}
}

/**
 * Single `EventSource` client for `GET /api/v1/events` with reconnect backoff
 * and TanStack Query invalidation (docs/16-frontend.md, "State with runes").
 */
class Events {
	connected = $state(false);

	private source: EventSource | null = null;
	private queryClient: QueryClient | null = null;
	private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
	private backoffMs = INITIAL_BACKOFF_MS;
	private needsResync = false;
	private active = false;

	connect(queryClient: QueryClient): void {
		this.queryClient = queryClient;
		if (this.active || typeof EventSource === 'undefined') return;
		this.active = true;
		this.backoffMs = INITIAL_BACKOFF_MS;
		this.needsResync = false;
		this.open();
	}

	disconnect(): void {
		this.active = false;
		this.connected = false;
		this.needsResync = false;
		this.backoffMs = INITIAL_BACKOFF_MS;
		if (this.reconnectTimer !== null) {
			clearTimeout(this.reconnectTimer);
			this.reconnectTimer = null;
		}
		this.closeSource();
		this.queryClient = null;
	}

	private open(): void {
		if (!this.active || typeof EventSource === 'undefined') return;
		this.closeSource();

		const source = new EventSource(EVENTS_URL);
		this.source = source;

		source.onopen = () => {
			if (this.source !== source || !this.active) return;
			this.connected = true;
			this.backoffMs = INITIAL_BACKOFF_MS;
			if (this.needsResync) {
				this.needsResync = false;
				void this.queryClient?.invalidateQueries();
			}
		};

		source.onerror = () => {
			if (this.source !== source || !this.active) return;
			this.connected = false;
			this.needsResync = true;
			this.closeSource();
			this.scheduleReconnect();
		};

		source.addEventListener('download.progress', (event) => {
			const payload = parseJson<DownloadProgressEvent>((event as MessageEvent).data);
			if (!payload) return;
			void this.queryClient?.invalidateQueries({ queryKey: ['queue'] });
		});

		source.addEventListener('download.state', (event) => {
			const payload = parseJson<DownloadStateEvent>((event as MessageEvent).data);
			if (!payload) return;
			void this.queryClient?.invalidateQueries({ queryKey: ['queue'] });
		});

		source.addEventListener('movie.updated', (event) => {
			const payload = parseJson<MovieUpdatedEvent>((event as MessageEvent).data);
			if (!payload) return;
			void this.queryClient?.invalidateQueries({ queryKey: ['movies'] });
			if (typeof payload.movieId === 'number') {
				void this.queryClient?.invalidateQueries({ queryKey: ['movie', payload.movieId] });
			}
		});

		source.addEventListener('task.status', (event) => {
			const payload = parseJson<TaskStatusEvent>((event as MessageEvent).data);
			if (!payload) return;
			void this.queryClient?.invalidateQueries({ queryKey: ['tasks'] });
		});

		source.addEventListener('permissions.changed', () => {
			void session.load();
		});
	}

	private scheduleReconnect(): void {
		if (!this.active || this.reconnectTimer !== null) return;
		const delay = this.backoffMs;
		this.backoffMs = Math.min(this.backoffMs * 2, MAX_BACKOFF_MS);
		this.reconnectTimer = setTimeout(() => {
			this.reconnectTimer = null;
			this.open();
		}, delay);
	}

	private closeSource(): void {
		if (!this.source) return;
		this.source.onopen = null;
		this.source.onerror = null;
		this.source.close();
		this.source = null;
	}
}

export const events = new Events();
