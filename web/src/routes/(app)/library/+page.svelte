<script lang="ts">
	import { resolve } from '$app/paths';
	import { createQuery } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import MonitoredSwitch from '#lib/components/monitored-switch.svelte';
	import NoAccess from '#lib/components/no-access.svelte';
	import { Badge } from '#lib/components/ui/badge/index.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import * as Select from '#lib/components/ui/select/index.js';
	import * as Table from '#lib/components/ui/table/index.js';
	import {
		noFilters,
		posterUrl,
		visibleMovies,
		type Filters,
		type Movie,
		type MonitoredFilter,
		type SortDirection,
		type SortKey
	} from '#lib/library.js';
	import { session } from '#lib/session.svelte.js';

	/** Cards or rows added each time the end of the list scrolls into view. */
	const CHUNK = 60;
	const VIEW_KEY = 'unseenarr.library.view';

	const read = $derived(session.can('library:read'));

	const movies = createQuery(() => ({
		queryKey: ['movies'],
		enabled: read,
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/movies');
			if (!data) throw new Error(error?.detail ?? 'The library could not be loaded.');
			return data;
		}
	}));
	const tags = createQuery(() => ({
		queryKey: ['tags'],
		enabled: read,
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/tags');
			if (!data) throw new Error(error?.detail ?? 'The tags could not be loaded.');
			return data;
		}
	}));
	const profiles = createQuery(() => ({
		queryKey: ['quality-profiles'],
		enabled: session.can('profiles:read'),
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/quality-profiles');
			if (!data) throw new Error(error?.detail ?? 'The profiles could not be loaded.');
			return data;
		}
	}));

	function storedView(): 'grid' | 'table' {
		try {
			return localStorage.getItem(VIEW_KEY) === 'table' ? 'table' : 'grid';
		} catch {
			return 'grid';
		}
	}

	let view = $state<'grid' | 'table'>(storedView());
	let filters = $state<Filters>({ ...noFilters });
	let sort = $state<SortKey>('title');
	let direction = $state<SortDirection>('asc');
	let failure = $state('');
	let shown = $state(CHUNK);
	let end = $state<HTMLElement | null>(null);

	const rows = $derived(visibleMovies(movies.data ?? [], filters, sort, direction));
	const page = $derived(rows.slice(0, shown));

	// A new filter or sort starts at the top again.
	$effect(() => {
		void [filters.text, filters.monitored, filters.profileId, filters.tagId, sort, direction];
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

	function setView(next: 'grid' | 'table') {
		view = next;
		try {
			localStorage.setItem(VIEW_KEY, next);
		} catch {
			// The choice is not remembered. Nothing else depends on it.
		}
	}

	const sortNames: Record<SortKey, string> = { title: 'Title', year: 'Year', added: 'Date added' };
	const monitoredNames: Record<MonitoredFilter, string> = {
		all: 'Any state',
		monitored: 'Monitored',
		unmonitored: 'Not monitored'
	};

	function profileLabel(movie: Movie): string {
		return movie.profiles
			.map(
				(link) =>
					profiles.data?.find((profile) => profile.id === link.qualityProfileId)?.name ??
					`Profile ${link.qualityProfileId}`
			)
			.join(', ');
	}

	function tagLabels(movie: Movie): string[] {
		return movie.tags.map((id) => tags.data?.find((tag) => tag.id === id)?.label ?? `#${id}`);
	}
</script>

<svelte:head>
	<title>Library · Unseenarr</title>
</svelte:head>

{#if !read}
	<NoAccess anyOf={['library:read']} />
{:else}
	<div class="flex flex-wrap items-center justify-between gap-2">
		<h1 class="text-2xl font-semibold">
			Library
			{#if movies.data}
				<span class="text-base font-normal text-muted-foreground">
					{rows.length === movies.data.length
						? `${rows.length} movies`
						: `${rows.length} of ${movies.data.length} movies`}
				</span>
			{/if}
		</h1>
		<div class="flex gap-1" role="group" aria-label="View">
			<Button
				size="sm"
				variant={view === 'grid' ? 'default' : 'outline'}
				aria-pressed={view === 'grid'}
				onclick={() => setView('grid')}
			>
				Posters
			</Button>
			<Button
				size="sm"
				variant={view === 'table' ? 'default' : 'outline'}
				aria-pressed={view === 'table'}
				onclick={() => setView('table')}
			>
				Table
			</Button>
		</div>
	</div>

	<div class="mt-4 flex flex-wrap items-center gap-2">
		<Input
			class="w-full sm:w-64"
			type="search"
			placeholder="Filter by title or year"
			aria-label="Filter by title or year"
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
		{#if profiles.data && profiles.data.length > 0}
			<Select.Root
				type="single"
				value={filters.profileId === null ? '' : String(filters.profileId)}
				onValueChange={(value) => (filters.profileId = value === '' ? null : Number(value))}
			>
				<Select.Trigger class="w-44" aria-label="Quality profile">
					{profiles.data.find((profile) => profile.id === filters.profileId)?.name ??
						'Any profile'}
				</Select.Trigger>
				<Select.Content>
					<Select.Item value="" label="Any profile" />
					{#each profiles.data as profile (profile.id)}
						<Select.Item value={String(profile.id)} label={profile.name} />
					{/each}
				</Select.Content>
			</Select.Root>
		{/if}
		{#if tags.data && tags.data.length > 0}
			<Select.Root
				type="single"
				value={filters.tagId === null ? '' : String(filters.tagId)}
				onValueChange={(value) => (filters.tagId = value === '' ? null : Number(value))}
			>
				<Select.Trigger class="w-40" aria-label="Tag">
					{tags.data.find((tag) => tag.id === filters.tagId)?.label ?? 'Any tag'}
				</Select.Trigger>
				<Select.Content>
					<Select.Item value="" label="Any tag" />
					{#each tags.data as tag (tag.id)}
						<Select.Item value={String(tag.id)} label={tag.label} />
					{/each}
				</Select.Content>
			</Select.Root>
		{/if}
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

	{#if failure}
		<p role="alert" class="mt-4 text-sm text-destructive">{failure}</p>
	{/if}

	{#if movies.isPending}
		<p class="mt-4 text-muted-foreground">Loading…</p>
	{:else if movies.isError}
		<p class="mt-4 text-destructive">{movies.error.message}</p>
	{:else if movies.data.length === 0}
		<p class="mt-4 text-muted-foreground">
			The library is empty.
			{#if session.can('library:add')}
				<a class="underline" href={resolve('add')}>Add a movie</a>.
			{/if}
		</p>
	{:else if rows.length === 0}
		<p class="mt-4 text-muted-foreground">No movie matches the filters.</p>
	{:else if view === 'grid'}
		<ul class="mt-4 grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6">
			{#each page as movie (movie.id)}
				{@const poster = posterUrl(movie.posterPath)}
				<li class="card flex flex-col gap-2">
					<a href={resolve('/(app)/library/[id]', { id: String(movie.id) })} class="block overflow-hidden rounded-lg border bg-muted">
						{#if poster}
							<img
								src={poster}
								alt={movie.title}
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
								{movie.title}
							</div>
						{/if}
					</a>
					<div class="flex items-start justify-between gap-2">
						<div class="min-w-0">
							<a href={resolve('/(app)/library/[id]', { id: String(movie.id) })} class="block truncate text-sm font-medium" title={movie.title}>
								{movie.title}
							</a>
							<span class="text-xs text-muted-foreground">{movie.year}</span>
						</div>
						<MonitoredSwitch {movie} onerror={(message) => (failure = message)} />
					</div>
				</li>
			{/each}
		</ul>
	{:else}
		<Table.Root class="mt-4">
			<Table.Header>
				<Table.Row>
					<Table.Head>Title</Table.Head>
					<Table.Head>Year</Table.Head>
					<Table.Head>Profiles</Table.Head>
					<Table.Head>Tags</Table.Head>
					<Table.Head class="text-end">Monitored</Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each page as movie (movie.id)}
					<Table.Row>
						<Table.Cell class="font-medium">
							<a class="underline-offset-4 hover:underline" href={resolve('/(app)/library/[id]', { id: String(movie.id) })}>{movie.title}</a>
						</Table.Cell>
						<Table.Cell>{movie.year}</Table.Cell>
						<Table.Cell>{profileLabel(movie)}</Table.Cell>
						<Table.Cell class="space-x-1">
							{#each tagLabels(movie) as label (label)}
								<Badge variant="secondary">{label}</Badge>
							{/each}
						</Table.Cell>
						<Table.Cell class="text-end">
							<MonitoredSwitch {movie} onerror={(message) => (failure = message)} />
						</Table.Cell>
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	{/if}

	{#if page.length < rows.length}
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
