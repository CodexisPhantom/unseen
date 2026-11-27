<script lang="ts">
	import { resolve } from '$app/paths';
	import { createQuery } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import { lookupYear, type MovieLookup } from '#lib/add.js';
	import MovieAddDialog from '#lib/components/movie-add-dialog.svelte';
	import NoAccess from '#lib/components/no-access.svelte';
	import { Badge } from '#lib/components/ui/badge/index.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import { posterUrl, type Movie } from '#lib/library.js';
	import { session } from '#lib/session.svelte.js';

	const add = $derived(session.can('library:add'));
	const canReadRootFolders = $derived(session.can('settings:read'));
	const canReadProfiles = $derived(session.can('profiles:read'));

	let draft = $state('');
	let term = $state('');
	let picked = $state<MovieLookup | null>(null);
	let dialogOpen = $state(false);
	let added = $state<Movie | null>(null);

	// Search shortly after typing stops; Enter searches at once.
	$effect(() => {
		const next = draft.trim();
		const timer = setTimeout(() => (term = next), 400);
		return () => clearTimeout(timer);
	});

	const results = createQuery(() => ({
		queryKey: ['lookup', term],
		enabled: add && term !== '',
		retry: false,
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/movies/lookup', {
				params: { query: { term } }
			});
			if (!data) throw new Error(error?.detail ?? 'The search failed.');
			return data;
		}
	}));
	const movies = createQuery(() => ({
		queryKey: ['movies'],
		enabled: session.can('library:read'),
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/movies');
			if (!data) throw new Error(error?.detail ?? 'The library could not be loaded.');
			return data;
		}
	}));
	const rootFolders = createQuery(() => ({
		queryKey: ['root-folders'],
		enabled: add && canReadRootFolders,
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/root-folders');
			if (!data) throw new Error(error?.detail ?? 'The root folders could not be loaded.');
			return data;
		}
	}));
	const profiles = createQuery(() => ({
		queryKey: ['quality-profiles'],
		enabled: add && canReadProfiles,
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/quality-profiles');
			if (!data) throw new Error(error?.detail ?? 'The profiles could not be loaded.');
			return data;
		}
	}));
	const tags = createQuery(() => ({
		queryKey: ['tags'],
		enabled: add && session.can('library:read'),
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/tags');
			if (!data) throw new Error(error?.detail ?? 'The tags could not be loaded.');
			return data;
		}
	}));

	/** The library movie for each TMDB id already added. */
	const inLibrary = $derived(new Map((movies.data ?? []).map((movie) => [movie.tmdbId, movie])));

	const missing = $derived(
		[
			canReadRootFolders ? null : 'settings:read',
			canReadProfiles ? null : 'profiles:read'
		].filter((permission) => permission !== null)
	);
	const ready = $derived(
		(rootFolders.data?.length ?? 0) > 0 && (profiles.data?.length ?? 0) > 0
	);

	function choose(result: MovieLookup) {
		picked = result;
		dialogOpen = true;
	}
</script>

<svelte:head>
	<title>Add a movie · Unseenarr</title>
</svelte:head>

{#if !add}
	<NoAccess anyOf={['library:add']} />
{:else}
	<h1 class="text-2xl font-semibold">Add a movie</h1>

	<form
		class="mt-4"
		onsubmit={(event) => {
			event.preventDefault();
			term = draft.trim();
		}}
	>
		<Input
			class="w-full sm:max-w-md"
			type="search"
			placeholder="Title, tmdb:603 or imdb:tt0133093"
			aria-label="Search for a movie"
			bind:value={draft}
		/>
	</form>

	{#if missing.length > 0}
		<p class="mt-4 text-sm text-muted-foreground">
			Choosing a root folder and a quality profile needs
			{#each missing as permission, index (permission)}{index > 0 ? ' and ' : ''}<code
					>{permission}</code
				>{/each}. Ask an administrator to grant {missing.length > 1 ? 'them' : 'it'}.
		</p>
	{:else if rootFolders.isSuccess && profiles.isSuccess && !ready}
		<p class="mt-4 text-sm text-muted-foreground">
			Add a root folder and a quality profile in Settings before adding movies.
		</p>
	{/if}

	{#if added}
		<p role="status" class="mt-4 text-sm">
			Added
			<a
				class="underline"
				href={resolve('/(app)/library/[id]', { id: String(added.id) })}
			>
				{added.title}
			</a>
			to the library.
		</p>
	{/if}

	{#if term === ''}
		<p class="mt-4 text-muted-foreground">Type a title to search TMDB.</p>
	{:else if results.isPending}
		<p class="mt-4 text-muted-foreground">Searching…</p>
	{:else if results.isError}
		<p role="alert" class="mt-4 text-destructive">{results.error.message}</p>
	{:else if results.data.length === 0}
		<p class="mt-4 text-muted-foreground">No movie matches “{term}”.</p>
	{:else}
		<ul class="mt-4 grid gap-4">
			{#each results.data as result (result.tmdbId)}
				{@const poster = posterUrl(result.posterPath, 154)}
				{@const existing = inLibrary.get(result.tmdbId)}
				{@const year = lookupYear(result)}
				<li class="flex gap-4 rounded-lg border p-3">
					{#if poster}
						<img
							src={poster}
							alt=""
							loading="lazy"
							decoding="async"
							width="154"
							height="231"
							class="aspect-[2/3] w-20 shrink-0 rounded-md bg-muted object-cover sm:w-24"
						/>
					{:else}
						<div
							class="flex aspect-[2/3] w-20 shrink-0 items-center justify-center rounded-md bg-muted p-1 text-center text-xs text-muted-foreground sm:w-24"
						>
							No poster
						</div>
					{/if}
					<div class="flex min-w-0 flex-1 flex-col gap-2">
						<div>
							<h2 class="font-medium">
								{result.title}
								{#if year}<span class="font-normal text-muted-foreground">({year})</span>{/if}
							</h2>
							{#if result.originalTitle && result.originalTitle !== result.title}
								<p class="text-xs text-muted-foreground">{result.originalTitle}</p>
							{/if}
						</div>
						{#if result.overview}
							<p class="line-clamp-3 text-sm text-muted-foreground">{result.overview}</p>
						{/if}
						<div class="mt-auto flex items-center gap-2">
							{#if existing}
								<Badge variant="secondary">Already in library</Badge>
								<a
									class="text-sm underline"
									href={resolve('/(app)/library/[id]', { id: String(existing.id) })}
								>
									Open
								</a>
							{:else}
								<Button size="sm" disabled={!ready} onclick={() => choose(result)}>Add</Button>
							{/if}
						</div>
					</div>
				</li>
			{/each}
		</ul>
	{/if}

	{#if picked && rootFolders.data && profiles.data}
		<MovieAddDialog
			bind:open={dialogOpen}
			result={picked}
			rootFolders={rootFolders.data}
			profiles={profiles.data}
			tags={tags.data ?? []}
			onadded={(movie) => (added = movie)}
		/>
	{/if}
{/if}
