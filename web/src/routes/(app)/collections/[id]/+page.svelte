<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { loadChoices, pickId, saveChoices } from '#lib/add.js';
	import { api } from '#lib/api/client.js';
	import {
		CollectionApiError,
		addedMovieCount,
		buildDefaultsPatch,
		buildMonitorPatch,
		collectionCountLabel,
		draftChanged,
		draftOf,
		isTmdbProblem,
		missingReleaseState,
		missingYear,
		monitorProblem,
		releaseStateNames,
		sortCollectionMovies,
		sortMissingMovies,
		toCollectionError,
		type Collection,
		type CollectionDetail,
		type CollectionDraft,
		type CollectionPatch
	} from '#lib/collections.js';
	import NoAccess from '#lib/components/no-access.svelte';
	import { Badge } from '#lib/components/ui/badge/index.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import * as Select from '#lib/components/ui/select/index.js';
	import { Switch } from '#lib/components/ui/switch/index.js';
	import { availabilityNames, formatDate, posterUrl, profileState } from '#lib/library.js';
	import { session } from '#lib/session.svelte.js';

	const id = $derived(Number(page.params.id));
	const validId = $derived(Number.isInteger(id) && id > 0);
	const read = $derived(session.can('library:read'));
	const edit = $derived(session.can('library:edit'));
	const canReadRootFolders = $derived(session.can('settings:read'));
	const canReadProfiles = $derived(session.can('profiles:read'));

	const queryClient = useQueryClient();

	const detail = createQuery(() => ({
		queryKey: ['collection', id],
		enabled: read && validId,
		retry: false,
		queryFn: async () => {
			const { data, error, response } = await api.GET('/api/v1/collections/{id}', {
				params: { path: { id } }
			});
			if (!data) {
				throw toCollectionError(
					error,
					response.status,
					response.status === 404
						? 'This collection was not found.'
						: 'The collection could not be loaded.'
				);
			}
			return data;
		}
	}));

	const detailTmdbError = $derived(
		detail.isError &&
			detail.error instanceof CollectionApiError &&
			isTmdbProblem(detail.error.code, detail.error.status)
	);
	const tmdbNotConfigured = $derived(
		detail.isError &&
			detail.error instanceof CollectionApiError &&
			detail.error.code === 'tmdb_not_configured'
	);

	// Fallback to local records when TMDB fails so the collection and its library movies stay usable.
	const fallbackCollections = createQuery(() => ({
		queryKey: ['collections'],
		enabled: read && validId && detailTmdbError,
		queryFn: async () => {
			const { data, error, response } = await api.GET('/api/v1/collections');
			if (!data) {
				throw toCollectionError(error, response.status, 'The collections could not be loaded.');
			}
			return data;
		}
	}));
	const fallbackMovies = createQuery(() => ({
		queryKey: ['movies'],
		enabled: read && validId && detailTmdbError,
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/movies');
			if (!data) throw new Error(error?.detail ?? 'The library could not be loaded.');
			return data;
		}
	}));

	const rootFolders = createQuery(() => ({
		queryKey: ['root-folders'],
		enabled: read && canReadRootFolders,
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/root-folders');
			if (!data) throw new Error(error?.detail ?? 'The root folders could not be loaded.');
			return data;
		}
	}));
	const profiles = createQuery(() => ({
		queryKey: ['quality-profiles'],
		enabled: read && canReadProfiles,
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/quality-profiles');
			if (!data) throw new Error(error?.detail ?? 'The profiles could not be loaded.');
			return data;
		}
	}));

	const fallbackCollection = $derived(
		fallbackCollections.data?.find((candidate) => candidate.id === id) ?? null
	);
	const current = $derived<Collection | null>(detail.data ?? fallbackCollection);
	const libraryMovies = $derived(
		sortCollectionMovies(
			detail.data
				? detail.data.movies
				: (fallbackMovies.data ?? []).filter((movie) => movie.collectionId === id)
		)
	);
	const missingMovies = $derived(sortMissingMovies(detail.data?.missing ?? []));

	let syncedId = $state<number | null>(null);
	let syncedUpdatedAt = $state<string | null>(null);
	let monitored = $state(false);
	let draft = $state<CollectionDraft>({
		rootFolderId: null,
		qualityProfileId: null,
		minimumAvailability: 'released'
	});
	let failure = $state('');
	let notice = $state('');

	function defaultChoices() {
		const remembered = loadChoices();
		return {
			rootFolderId: pickId(
				remembered.rootFolderId,
				(rootFolders.data ?? []).map((folder) => folder.id)
			),
			qualityProfileId: pickId(
				remembered.qualityProfileId,
				(profiles.data ?? []).map((profile) => profile.id)
			)
		};
	}

	// Sync editable state whenever a new collection loads or updates on the server.
	$effect(() => {
		if (!current) return;
		if (syncedId !== current.id || syncedUpdatedAt !== current.updatedAt) {
			if (syncedId !== current.id) {
				failure = '';
				notice = '';
			}
			syncedId = current.id;
			syncedUpdatedAt = current.updatedAt;
			monitored = current.monitored;
			draft = draftOf(current, defaultChoices());
		}
	});

	// Fill unset defaults once root folders or profiles finish loading.
	$effect(() => {
		if (!current) return;
		if (current.rootFolderId == null && draft.rootFolderId === null && rootFolders.data?.length) {
			draft.rootFolderId = pickId(
				loadChoices().rootFolderId,
				rootFolders.data.map((folder) => folder.id)
			);
		}
	});
	$effect(() => {
		if (!current) return;
		if (
			current.qualityProfileId == null &&
			draft.qualityProfileId === null &&
			profiles.data?.length
		) {
			draft.qualityProfileId = pickId(
				loadChoices().qualityProfileId,
				profiles.data.map((profile) => profile.id)
			);
		}
	});

	const missingPermissions = $derived(
		[
			canReadRootFolders ? null : 'settings:read',
			canReadProfiles ? null : 'profiles:read'
		].filter((permission) => permission !== null)
	);
	const setupReady = $derived(
		(rootFolders.data?.length ?? 0) > 0 && (profiles.data?.length ?? 0) > 0
	);
	const dirty = $derived(current ? draftChanged(current, draft) : false);

	const updateCollection = createMutation(() => ({
		mutationFn: async (input: {
			patch: CollectionPatch;
			previousMonitored: boolean;
			previousMovieCount: number;
			mode: 'monitor' | 'defaults';
		}) => {
			const { data, error, response } = await api.PATCH('/api/v1/collections/{id}', {
				params: { path: { id } },
				body: input.patch
			});
			if (!data) {
				throw toCollectionError(error, response.status, 'The collection could not be updated.');
			}
			return { updated: data, ...input };
		},
		onSuccess: async ({ updated, previousMovieCount, mode }) => {
			failure = '';
			const cached = queryClient.getQueryData<CollectionDetail>(['collection', id]);
			if (cached) {
				queryClient.setQueryData<CollectionDetail>(['collection', id], { ...cached, ...updated });
			}
			syncedId = updated.id;
			syncedUpdatedAt = updated.updatedAt;
			monitored = updated.monitored;
			draft = draftOf(updated, defaultChoices());
			if (updated.rootFolderId != null && updated.qualityProfileId != null) {
				saveChoices({
					rootFolderId: updated.rootFolderId,
					qualityProfileId: updated.qualityProfileId
				});
			}
			if (mode === 'defaults') {
				notice = 'Collection defaults saved.';
			} else if (updated.monitored) {
				const added = addedMovieCount(previousMovieCount, updated.movieCount);
				notice =
					added > 0
						? `Collection is now monitored. Added ${added} ${added === 1 ? 'movie' : 'movies'} to the library.`
						: 'Collection is now monitored.';
			} else {
				notice = 'Collection is no longer monitored.';
			}
			await Promise.all([
				queryClient.invalidateQueries({ queryKey: ['collections'] }),
				queryClient.invalidateQueries({ queryKey: ['collection', id] }),
				...(updated.monitored ? [queryClient.invalidateQueries({ queryKey: ['movies'] })] : [])
			]);
		},
		onError: (error, variables) => {
			notice = '';
			failure = error.message;
			monitored = variables.previousMonitored;
		}
	}));

	function onToggleMonitor(next: boolean) {
		if (!current) return;
		notice = '';
		if (next) {
			const problem = monitorProblem(draft);
			if (problem) {
				failure = problem;
				monitored = current.monitored;
				return;
			}
		}
		failure = '';
		updateCollection.mutate({
			patch: buildMonitorPatch(next, draft),
			previousMonitored: current.monitored,
			previousMovieCount: current.movieCount,
			mode: 'monitor'
		});
	}

	function onSaveDefaults(event: SubmitEvent) {
		event.preventDefault();
		if (!current) return;
		notice = '';
		failure = '';
		updateCollection.mutate({
			patch: buildDefaultsPatch(draft),
			previousMonitored: current.monitored,
			previousMovieCount: current.movieCount,
			mode: 'defaults'
		});
	}

	function rootFolderLabel(rootFolderId: number | null | undefined): string {
		if (rootFolderId == null) return 'Not set';
		const found = rootFolders.data?.find((folder) => folder.id === rootFolderId);
		if (!found) return `Folder #${rootFolderId}`;
		return found.accessible ? found.path : `${found.path} (not accessible)`;
	}

	function profileLabel(qualityProfileId: number | null | undefined): string {
		if (qualityProfileId == null) return 'Not set';
		return (
			profiles.data?.find((profile) => profile.id === qualityProfileId)?.name ??
			`Profile #${qualityProfileId}`
		);
	}
</script>

<svelte:head>
	<title>{current?.title ?? 'Collection'} · Unseen</title>
</svelte:head>

{#if !read}
	<NoAccess anyOf={['library:read']} />
{:else}
	<p class="text-sm"><a class="underline" href={resolve('collections')}>← Collections</a></p>

	{#if !validId}
		<p role="alert" class="mt-4 text-destructive">This collection was not found.</p>
	{:else if (detail.isPending || (detailTmdbError && fallbackCollections.isPending)) && !current}
		<p class="mt-4 text-muted-foreground">Loading…</p>
	{:else if !current && detail.isError}
		<div class="mt-4 space-y-2">
			<p role="alert" class="text-destructive">{detail.error.message}</p>
			{#if detailTmdbError}
				<div class="flex flex-wrap items-center gap-3 text-sm">
					<Button variant="outline" size="sm" onclick={() => detail.refetch()}>Retry</Button>
					{#if tmdbNotConfigured && canReadRootFolders}
						<a class="underline" href={resolve('settings')}>Open Settings</a>
					{/if}
				</div>
			{/if}
		</div>
	{:else if current}
		{@const c = current}
		{@const poster = posterUrl(c.posterPath)}
		<div class="mt-4 flex flex-col gap-6 sm:flex-row">
			{#if poster}
				<img
					src={poster}
					alt={c.title}
					width="342"
					height="513"
					class="aspect-[2/3] w-48 shrink-0 self-start rounded-lg border object-cover"
				/>
			{/if}
			<div class="min-w-0 flex-1">
				<div class="flex flex-wrap items-center gap-2">
					<h1 class="text-2xl font-semibold">{c.title}</h1>
					<Badge variant={c.monitored ? 'secondary' : 'outline'}>
						{c.monitored ? 'Monitored' : 'Unmonitored'}
					</Badge>
				</div>
				<p class="mt-1 text-sm text-muted-foreground">
					{collectionCountLabel(libraryMovies.length, detail.data ? missingMovies.length : null)}
				</p>
				{#if c.overview}
					<p class="mt-3 max-w-prose text-sm">{c.overview}</p>
				{/if}

				{#if edit}
					<section class="mt-5 rounded-lg border p-4" aria-labelledby="collection-monitoring-heading">
						<div class="flex flex-wrap items-center justify-between gap-3">
							<div>
								<h2 id="collection-monitoring-heading" class="text-sm font-semibold">
									Monitoring
								</h2>
								<p class="text-xs text-muted-foreground">
									Monitoring adds missing released and announced movies using the defaults below.
								</p>
							</div>
							<div class="flex items-center gap-2">
								<Label for="collection-monitored">Monitor collection</Label>
								<Switch
									id="collection-monitored"
									bind:checked={monitored}
									disabled={updateCollection.isPending}
									onCheckedChange={onToggleMonitor}
								/>
							</div>
						</div>

						{#if missingPermissions.length > 0}
							<p class="mt-3 text-xs text-muted-foreground">
								Choosing a root folder and a quality profile needs
								{#each missingPermissions as permission, index (permission)}{index > 0
										? ' and '
										: ''}<code>{permission}</code>{/each}. Ask an administrator to grant
								{missingPermissions.length > 1 ? 'them' : 'it'}.
							</p>
						{:else if rootFolders.isSuccess && profiles.isSuccess && !setupReady}
							<p class="mt-3 text-xs text-muted-foreground">
								Add a root folder and a quality profile in
								<a class="underline" href={resolve('settings')}>Settings</a>
								before monitoring this collection.
							</p>
						{/if}

						<form class="mt-4 grid gap-4 sm:grid-cols-3" onsubmit={onSaveDefaults}>
							<div class="grid gap-1.5">
								<Label for="collection-root-folder">Root folder</Label>
								{#if canReadRootFolders}
									<Select.Root
										type="single"
										value={draft.rootFolderId === null ? '' : String(draft.rootFolderId)}
										disabled={updateCollection.isPending || (rootFolders.data?.length ?? 0) === 0}
										onValueChange={(value) => {
											draft.rootFolderId = value === '' ? null : Number(value);
											failure = '';
										}}
									>
										<Select.Trigger id="collection-root-folder" class="w-full font-mono text-xs">
											{draft.rootFolderId !== null
												? rootFolderLabel(draft.rootFolderId)
												: rootFolders.isPending
													? 'Loading…'
													: 'Choose a folder'}
										</Select.Trigger>
										<Select.Content>
											{#each rootFolders.data ?? [] as folder (folder.id)}
												<Select.Item
													value={String(folder.id)}
													label={folder.accessible
														? folder.path
														: `${folder.path} (not accessible)`}
												/>
											{/each}
										</Select.Content>
									</Select.Root>
								{:else}
									<p class="text-sm text-muted-foreground">{rootFolderLabel(draft.rootFolderId)}</p>
								{/if}
							</div>

							<div class="grid gap-1.5">
								<Label for="collection-profile">Quality profile</Label>
								{#if canReadProfiles}
									<Select.Root
										type="single"
										value={draft.qualityProfileId === null ? '' : String(draft.qualityProfileId)}
										disabled={updateCollection.isPending || (profiles.data?.length ?? 0) === 0}
										onValueChange={(value) => {
											draft.qualityProfileId = value === '' ? null : Number(value);
											failure = '';
										}}
									>
										<Select.Trigger id="collection-profile" class="w-full">
											{draft.qualityProfileId !== null
												? profileLabel(draft.qualityProfileId)
												: profiles.isPending
													? 'Loading…'
													: 'Choose a profile'}
										</Select.Trigger>
										<Select.Content>
											{#each profiles.data ?? [] as profile (profile.id)}
												<Select.Item value={String(profile.id)} label={profile.name} />
											{/each}
										</Select.Content>
									</Select.Root>
								{:else}
									<p class="text-sm text-muted-foreground">
										{profileLabel(draft.qualityProfileId)}
									</p>
								{/if}
							</div>

							<div class="grid gap-1.5">
								<Label for="collection-availability">Minimum availability</Label>
								<Select.Root
									type="single"
									bind:value={draft.minimumAvailability}
									disabled={updateCollection.isPending}
								>
									<Select.Trigger id="collection-availability" class="w-full">
										{availabilityNames[draft.minimumAvailability] ?? draft.minimumAvailability}
									</Select.Trigger>
									<Select.Content>
										{#each Object.entries(availabilityNames) as [value, label] (value)}
											<Select.Item {value} {label} />
										{/each}
									</Select.Content>
								</Select.Root>
							</div>

							{#if dirty || updateCollection.isPending}
								<div class="flex items-center gap-3 sm:col-span-3">
									<Button type="submit" size="sm" disabled={!dirty || updateCollection.isPending}>
										{updateCollection.isPending ? 'Saving…' : 'Save defaults'}
									</Button>
								</div>
							{/if}
						</form>

						{#if rootFolders.isError}
							<p role="alert" class="mt-3 text-sm text-destructive">{rootFolders.error.message}</p>
						{/if}
						{#if profiles.isError}
							<p role="alert" class="mt-3 text-sm text-destructive">{profiles.error.message}</p>
						{/if}
						{#if failure}
							<p role="alert" class="mt-3 text-sm text-destructive">{failure}</p>
						{/if}
						{#if notice}
							<p role="status" class="mt-3 text-sm text-muted-foreground">{notice}</p>
						{/if}
					</section>
				{:else}
					<dl class="mt-4 grid grid-cols-[auto_1fr] gap-x-6 gap-y-1 text-sm">
						<dt class="text-muted-foreground">Monitoring</dt>
						<dd>{c.monitored ? 'Monitored' : 'Not monitored'}</dd>
						{#if c.rootFolderId != null}
							<dt class="text-muted-foreground">Root folder</dt>
							<dd class="font-mono text-xs">{rootFolderLabel(c.rootFolderId)}</dd>
						{/if}
						{#if c.qualityProfileId != null}
							<dt class="text-muted-foreground">Quality profile</dt>
							<dd>{profileLabel(c.qualityProfileId)}</dd>
						{/if}
						<dt class="text-muted-foreground">Minimum availability</dt>
						<dd>{availabilityNames[c.minimumAvailability] ?? c.minimumAvailability}</dd>
					</dl>
				{/if}
			</div>
		</div>

		<section class="mt-8" aria-labelledby="in-library-heading">
			<h2 id="in-library-heading" class="text-lg font-semibold">
				In library
				<span class="text-sm font-normal text-muted-foreground">({libraryMovies.length})</span>
			</h2>
			{#if libraryMovies.length === 0}
				<p class="mt-2 text-sm text-muted-foreground">
					No movie from this collection is in the library.
				</p>
			{:else}
				<ul class="mt-3 grid gap-3 sm:grid-cols-2">
					{#each libraryMovies as movie (movie.id)}
						{@const moviePoster = posterUrl(movie.posterPath, 154)}
						{@const primary = movie.profiles[0]}
						<li class="flex gap-3 rounded-lg border p-3">
							<a
								href={resolve('/(app)/library/[id]', { id: String(movie.id) })}
								class="block shrink-0 overflow-hidden rounded-md bg-muted"
							>
								{#if moviePoster}
									<img
										src={moviePoster}
										alt={movie.title}
										loading="lazy"
										decoding="async"
										width="154"
										height="231"
										class="aspect-[2/3] w-16 object-cover"
									/>
								{:else}
									<div
										class="flex aspect-[2/3] w-16 items-center justify-center p-1 text-center text-xs text-muted-foreground"
									>
										No poster
									</div>
								{/if}
							</a>
							<div class="flex min-w-0 flex-1 flex-col gap-1">
								<div class="flex flex-wrap items-baseline gap-1.5">
									<a
										href={resolve('/(app)/library/[id]', { id: String(movie.id) })}
										class="font-medium underline-offset-4 hover:underline"
									>
										{movie.title}
									</a>
									<span class="text-xs text-muted-foreground">({movie.year})</span>
								</div>
								<div class="flex flex-wrap items-center gap-1.5">
									{#if primary}
										<Badge variant={primary.fileId != null ? 'secondary' : 'outline'}>
											{profileState(primary, movie.monitored)}
										</Badge>
									{/if}
									<Badge variant="outline" class="capitalize">{movie.status}</Badge>
								</div>
								{#if movie.overview}
									<p class="line-clamp-2 text-xs text-muted-foreground">{movie.overview}</p>
								{/if}
							</div>
						</li>
					{/each}
				</ul>
			{/if}
		</section>

		<section class="mt-8" aria-labelledby="missing-heading">
			<h2 id="missing-heading" class="text-lg font-semibold">
				Missing
				{#if detail.data}
					<span class="text-sm font-normal text-muted-foreground">({missingMovies.length})</span>
				{/if}
			</h2>
			{#if detail.isError}
				<div role="alert" class="mt-2 space-y-2 text-sm">
					<p class="text-destructive">
						Missing movies could not be loaded from TMDB: {detail.error.message}
					</p>
					<div class="flex flex-wrap items-center gap-3">
						<Button variant="outline" size="sm" onclick={() => detail.refetch()}>Retry</Button>
						{#if tmdbNotConfigured && canReadRootFolders}
							<a class="underline" href={resolve('settings')}>Open Settings</a>
						{/if}
					</div>
				</div>
			{:else if missingMovies.length === 0}
				<p class="mt-2 text-sm text-muted-foreground">
					Every movie in this collection is already in the library.
				</p>
			{:else}
				<ul class="mt-3 grid gap-3 sm:grid-cols-2">
					{#each missingMovies as item (item.tmdbId)}
						{@const itemPoster = posterUrl(item.posterPath, 154)}
						{@const state = missingReleaseState(item)}
						{@const year = missingYear(item)}
						<li class="flex gap-3 rounded-lg border p-3">
							{#if itemPoster}
								<img
									src={itemPoster}
									alt={item.title}
									loading="lazy"
									decoding="async"
									width="154"
									height="231"
									class="aspect-[2/3] w-16 shrink-0 rounded-md bg-muted object-cover"
								/>
							{:else}
								<div
									class="flex aspect-[2/3] w-16 shrink-0 items-center justify-center rounded-md bg-muted p-1 text-center text-xs text-muted-foreground"
								>
									No poster
								</div>
							{/if}
							<div class="flex min-w-0 flex-1 flex-col gap-1">
								<div class="flex flex-wrap items-baseline gap-1.5">
									<span class="font-medium">{item.title}</span>
									{#if year}
										<span class="text-xs text-muted-foreground">({year})</span>
									{/if}
								</div>
								<div class="flex flex-wrap items-center gap-2">
									<Badge variant={state === 'released' ? 'secondary' : 'outline'}>
										{releaseStateNames[state]}
									</Badge>
									{#if item.releaseDate}
										<span class="text-xs text-muted-foreground">
											{formatDate(item.releaseDate)}
										</span>
									{/if}
								</div>
								{#if item.overview}
									<p class="line-clamp-2 text-xs text-muted-foreground">{item.overview}</p>
								{/if}
							</div>
						</li>
					{/each}
				</ul>
			{/if}
		</section>
	{/if}
{/if}
