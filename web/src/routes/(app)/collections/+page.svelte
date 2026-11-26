<script lang="ts">
	import { resolve } from '$app/paths';
	import { createQueries, createQuery } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import {
		CollectionApiError,
		collectionCountLabel,
		noCollectionFilters,
		toCollectionError,
		visibleCollections,
		type CollectionFilters,
		type CollectionMonitoredFilter,
		type CollectionSortKey,
		type SortDirection
	} from '#lib/collections.js';
	import NoAccess from '#lib/components/no-access.svelte';
	import { Badge } from '#lib/components/ui/badge/index.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import * as Select from '#lib/components/ui/select/index.js';
	import { posterUrl } from '#lib/library.js';
	import { session } from '#lib/session.svelte.js';

	/** Cards added each time the end of the list scrolls into view. */
	const CHUNK = 24;

	const read = $derived(session.can('library:read'));

	const collections = createQuery(() => ({
		queryKey: ['collections'],
		enabled: read,
		queryFn: async () => {
			const { data, error, response } = await api.GET('/api/v1/collections');
			if (!data) {
				throw toCollectionError(error, response.status, 'The collections could not be loaded.');
			}
			return data;
		}
	}));

	let filters = $state<CollectionFilters>({ ...noCollectionFilters });
	let sort = $state<CollectionSortKey>('title');
	let direction = $state<SortDirection>('asc');
	let shown = $state(CHUNK);
	let end = $state<HTMLElement | null>(null);

	const rows = $derived(visibleCollections(collections.data ?? [], filters, sort, direction));
	const visible = $derived(rows.slice(0, shown));

	// Reset scroll chunking when filter or sort changes.
	$effect(() => {
		void [filters.text, filters.monitored, sort, direction];
		shown = CHUNK;
	});

	// Show more when the end marker comes near the viewport.
	$effect(() => {
		if (!end) return;
		const watcher = new IntersectionObserver(
			(entries) => {
				if (entries.some((entry) => entry.isIntersecting)) shown += CHUNK;
			},
			{ rootMargin: '600px' }
		);
		watcher.observe(end);
		return () => watcher.disconnect();
	});

	// Load details for visible collections to show missing movie counts (cached per collection).
	const details = createQueries(() => ({
		queries: visible.map((collection) => ({
			queryKey: ['collection', collection.id] as const,
			enabled: read,
			retry: false,
			staleTime: 5 * 60_000,
			queryFn: async () => {
				const { data, error, response } = await api.GET('/api/v1/collections/{id}', {
					params: { path: { id: collection.id } }
				});
				if (!data) {
					throw toCollectionError(error, response.status, 'The collection details could not be loaded.');
				}
				return data;
			}
		}))
	}));

	const detailSummaries = $derived(
		new Map(
			details.flatMap((query) =>
				query.data
					? [
							[
								query.data.id,
								{ inLibrary: query.data.movies.length, missing: query.data.missing.length }
							] as const
						]
					: []
			)
		)
	);

	const detailError = $derived.by(() => {
		for (const query of details) {
			if (query.isError) return query.error;
		}
		return null;
	});

	const tmdbNotConfigured = $derived(
		detailError instanceof CollectionApiError && detailError.code === 'tmdb_not_configured'
	);

	const detailErrorMessage = $derived(detailError?.message ?? '');

	const sortNames: Record<CollectionSortKey, string> = {
		title: 'Title',
		movies: 'In library',
		added: 'Date added'
	};
	const monitoredNames: Record<CollectionMonitoredFilter, string> = {
		all: 'Any state',
		monitored: 'Monitored',
		unmonitored: 'Not monitored'
	};
</script>

<svelte:head>
	<title>Collections · Unseen</title>
</svelte:head>

{#if !read}
	<NoAccess anyOf={['library:read']} />
{:else}
	<div class="flex flex-wrap items-center justify-between gap-2">
		<h1 class="text-2xl font-semibold">
			Collections
			{#if collections.data}
				<span class="text-base font-normal text-muted-foreground">
					{#if rows.length === collections.data.length}
						{rows.length} {rows.length === 1 ? 'collection' : 'collections'}
					{:else}
						{rows.length} of {collections.data.length}
						{collections.data.length === 1 ? 'collection' : 'collections'}
					{/if}
				</span>
			{/if}
		</h1>
	</div>

	<div class="mt-4 flex flex-wrap items-center gap-2">
		<Input
			class="w-full sm:w-64"
			type="search"
			placeholder="Filter by title"
			aria-label="Filter collections by title"
			bind:value={filters.text}
		/>
		<Select.Root type="single" bind:value={filters.monitored}>
			<Select.Trigger class="w-40" aria-label="Monitored state">
				{monitoredNames[filters.monitored]}
			</Select.Trigger>
			<Select.Content>
				{#each Object.entries(monitoredNames) as [value, label] (value)}
					<Select.Item {value} {label} />
				{/each}
			</Select.Content>
		</Select.Root>
		<Select.Root type="single" bind:value={sort}>
			<Select.Trigger class="w-36" aria-label="Sort by">{sortNames[sort]}</Select.Trigger>
			<Select.Content>
				{#each Object.entries(sortNames) as [value, label] (value)}
					<Select.Item {value} {label} />
				{/each}
			</Select.Content>
		</Select.Root>
		<Button
			variant="outline"
			size="sm"
			aria-label={direction === 'asc' ? 'Ascending' : 'Descending'}
			onclick={() => (direction = direction === 'asc' ? 'desc' : 'asc')}
		>
			{direction === 'asc' ? 'A → Z' : 'Z → A'}
		</Button>
	</div>

	{#if detailErrorMessage}
		<p role="alert" class="mt-4 text-sm text-muted-foreground">
			Missing counts could not be loaded: {detailErrorMessage}
			{#if tmdbNotConfigured && session.can('settings:read')}
				<a class="underline" href={resolve('settings')}>Open Settings</a>.
			{/if}
		</p>
	{/if}

	{#if collections.isPending}
		<p class="mt-4 text-muted-foreground">Loading…</p>
	{:else if collections.isError}
		<p role="alert" class="mt-4 text-destructive">{collections.error.message}</p>
	{:else if collections.data.length === 0}
		<p class="mt-4 text-muted-foreground">
			No collections yet. Collections appear when movies that belong to a TMDB collection are added
			or adopted.
			{#if session.can('library:add')}
				<a class="underline" href={resolve('add')}>Add a movie</a>.
			{/if}
		</p>
	{:else if rows.length === 0}
		<p class="mt-4 text-muted-foreground">No collection matches the filters.</p>
	{:else}
		<ul
			class="mt-4 grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
		>
			{#each visible as collection (collection.id)}
				{@const poster = posterUrl(collection.posterPath)}
				{@const summary = detailSummaries.get(collection.id)}
				<li class="card flex flex-col gap-2">
					<a
						href={resolve('/(app)/collections/[id]', { id: String(collection.id) })}
						class="block overflow-hidden rounded-lg border bg-muted"
					>
						{#if poster}
							<img
								src={poster}
								alt={collection.title}
								loading="lazy"
								decoding="async"
								width="342"
								height="513"
								class="aspect-[2/3] w-full object-cover"
							/>
						{:else}
							<div
								class="flex aspect-[2/3] items-center justify-center p-2 text-center text-sm text-muted-foreground"
							>
								{collection.title}
							</div>
						{/if}
					</a>
					<div class="flex flex-col gap-1">
						<a
							href={resolve('/(app)/collections/[id]', { id: String(collection.id) })}
							class="block truncate text-sm font-medium"
							title={collection.title}
						>
							{collection.title}
						</a>
						<div class="flex flex-wrap items-center justify-between gap-1">
							<span class="text-xs text-muted-foreground">
								{collectionCountLabel(
									summary?.inLibrary ?? collection.movieCount,
									summary?.missing ?? null
								)}
							</span>
							<Badge variant={collection.monitored ? 'secondary' : 'outline'}>
								{collection.monitored ? 'Monitored' : 'Unmonitored'}
							</Badge>
						</div>
					</div>
				</li>
			{/each}
		</ul>
	{/if}

	{#if visible.length < rows.length}
		<div bind:this={end} class="h-8" aria-hidden="true"></div>
	{/if}
{/if}

<style>
	/* A card far below the viewport costs no layout or paint until it is near. */
	.card {
		content-visibility: auto;
		contain-intrinsic-size: auto 340px;
	}
</style>
