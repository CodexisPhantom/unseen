<script lang="ts">
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import { createQuery } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import MonitoredSwitch from '#lib/components/monitored-switch.svelte';
	import MovieDeleteDialog from '#lib/components/movie-delete-dialog.svelte';
	import MovieEditDialog from '#lib/components/movie-edit-dialog.svelte';
	import NoAccess from '#lib/components/no-access.svelte';
	import { Badge } from '#lib/components/ui/badge/index.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import * as Table from '#lib/components/ui/table/index.js';
	import {
		availabilityNames,
		formatDate,
		formatRuntime,
		posterUrl,
		profileState
	} from '#lib/library.js';
	import { qualityName } from '#lib/profiles.js';
	import { formatBytes } from '#lib/root-folders.js';
	import { session } from '#lib/session.svelte.js';

	const id = $derived(Number(page.params.id));
	const read = $derived(session.can('library:read'));
	const edit = $derived(session.can('library:edit'));
	const remove = $derived(session.can('library:delete'));

	const movie = createQuery(() => ({
		queryKey: ['movie', id],
		enabled: read && Number.isInteger(id),
		retry: false,
		queryFn: async () => {
			const { data, error, response } = await api.GET('/api/v1/movies/{id}', {
				params: { path: { id } }
			});
			if (!data) {
				throw new Error(
					response.status === 404 ? 'This movie is not in the library.' : (error?.detail ?? 'The movie could not be loaded.')
				);
			}
			return data;
		}
	}));
	const files = createQuery(() => ({
		queryKey: ['movie', id, 'files'],
		enabled: read && movie.isSuccess,
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/movies/{id}/files', {
				params: { path: { id } }
			});
			if (!data) throw new Error(error?.detail ?? 'The files could not be loaded.');
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

	let editOpen = $state(false);
	let deleteOpen = $state(false);
	let failure = $state('');

	function profileName(qualityProfileId: number): string {
		return (
			profiles.data?.find((profile) => profile.id === qualityProfileId)?.name ??
			`Profile ${qualityProfileId}`
		);
	}

	const tagLabels = $derived(
		(movie.data?.tags ?? []).map((tagId) => tags.data?.find((tag) => tag.id === tagId)?.label ?? `#${tagId}`)
	);
	const unmanaged = $derived((files.data ?? []).filter((file) => !file.managed));
	const fileOf = (fileId: number | null | undefined) => files.data?.find((file) => file.id === fileId);
</script>

<svelte:head>
	<title>{movie.data?.title ?? 'Movie'} · Unseen</title>
</svelte:head>

{#if !read}
	<NoAccess anyOf={['library:read']} />
{:else}
	<p class="text-sm"><a class="underline" href={resolve('library')}>← Library</a></p>

	{#if movie.isPending}
		<p class="mt-4 text-muted-foreground">Loading…</p>
	{:else if movie.isError}
		<p class="mt-4 text-destructive">{movie.error.message}</p>
	{:else}
		{@const m = movie.data}
		{@const poster = posterUrl(m.posterPath)}
		<div class="mt-4 flex flex-col gap-6 sm:flex-row">
			{#if poster}
				<img
					src={poster}
					alt={m.title}
					width="342"
					height="513"
					class="aspect-[2/3] w-48 shrink-0 self-start rounded-lg border object-cover"
				/>
			{/if}
			<div class="min-w-0 flex-1">
				<div class="flex flex-wrap items-start justify-between gap-2">
					<h1 class="text-2xl font-semibold">
						{m.title} <span class="font-normal text-muted-foreground">({m.year})</span>
					</h1>
					<div class="flex gap-2">
						{#if edit}
							<Button variant="outline" size="sm" onclick={() => (editOpen = true)}>Edit</Button>
						{/if}
						{#if remove}
							<Button variant="outline" size="sm" onclick={() => (deleteOpen = true)}>Delete</Button>
						{/if}
					</div>
				</div>
				{#if m.originalTitle && m.originalTitle !== m.title}
					<p class="text-sm text-muted-foreground">{m.originalTitle}</p>
				{/if}
				{#if failure}
					<p role="alert" class="mt-2 text-sm text-destructive">{failure}</p>
				{/if}

				<dl class="mt-4 grid grid-cols-[auto_1fr] gap-x-6 gap-y-1 text-sm">
					<dt class="text-muted-foreground">Monitored</dt>
					<dd><MonitoredSwitch movie={m} onerror={(message) => (failure = message)} /></dd>
					<dt class="text-muted-foreground">Status</dt>
					<dd class="capitalize">{m.status}</dd>
					<dt class="text-muted-foreground">Minimum availability</dt>
					<dd>{availabilityNames[m.minimumAvailability] ?? m.minimumAvailability}</dd>
					<dt class="text-muted-foreground">Runtime</dt>
					<dd>{formatRuntime(m.runtime)}</dd>
					<dt class="text-muted-foreground">In cinemas</dt>
					<dd>{formatDate(m.inCinemas)}</dd>
					<dt class="text-muted-foreground">Digital release</dt>
					<dd>{formatDate(m.digitalRelease)}</dd>
					<dt class="text-muted-foreground">Physical release</dt>
					<dd>{formatDate(m.physicalRelease)}</dd>
					<dt class="text-muted-foreground">Folder</dt>
					<dd class="break-all"><code>{m.folderName}</code></dd>
					<dt class="text-muted-foreground">Tags</dt>
					<dd class="space-x-1">
						{#each tagLabels as label (label)}<Badge variant="secondary">{label}</Badge>{:else}—{/each}
					</dd>
				</dl>
				{#if m.overview}
					<p class="mt-4 max-w-prose text-sm">{m.overview}</p>
				{/if}
			</div>
		</div>

		<h2 class="mt-8 text-lg font-semibold">Quality profiles</h2>
		<Table.Root class="mt-2">
			<Table.Header>
				<Table.Row>
					<Table.Head>Profile</Table.Head>
					<Table.Head>State</Table.Head>
					<Table.Head>File</Table.Head>
					<Table.Head>Quality</Table.Head>
					<Table.Head class="text-end">Size</Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each m.profiles as profile (profile.id)}
					{@const file = fileOf(profile.fileId)}
					<Table.Row>
						<Table.Cell class="font-medium">
							{profileName(profile.qualityProfileId)}
							{#if profile.isPrimary}<Badge variant="secondary">Primary</Badge>{/if}
						</Table.Cell>
						<Table.Cell>{profileState(profile, m.monitored)}</Table.Cell>
						<Table.Cell class="break-all">{file?.relativePath ?? '—'}</Table.Cell>
						<Table.Cell>{file ? qualityName(file.qualityId) : '—'}</Table.Cell>
						<Table.Cell class="text-end">{file ? formatBytes(file.size) : '—'}</Table.Cell>
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>

		{#if files.isError}
			<p class="mt-2 text-sm text-destructive">{files.error.message}</p>
		{/if}

		{#if unmanaged.length > 0}
			<h2 class="mt-8 text-lg font-semibold">Unmanaged files</h2>
			<p class="text-sm text-muted-foreground">
				These files belong to no profile. Unseen does not touch them.
			</p>
			<Table.Root class="mt-2">
				<Table.Header>
					<Table.Row>
						<Table.Head>File</Table.Head>
						<Table.Head>Quality</Table.Head>
						<Table.Head class="text-end">Size</Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each unmanaged as file (file.id)}
						<Table.Row>
							<Table.Cell class="break-all">{file.relativePath}</Table.Cell>
							<Table.Cell>{qualityName(file.qualityId)}</Table.Cell>
							<Table.Cell class="text-end">{formatBytes(file.size)}</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		{/if}

		{#if edit}
			<MovieEditDialog
				bind:open={editOpen}
				movie={m}
				tags={tags.data ?? []}
				profileNames={m.profiles.map((profile) => ({
					id: profile.id,
					name: profileName(profile.qualityProfileId)
				}))}
			/>
		{/if}
		{#if remove}
			<MovieDeleteDialog bind:open={deleteOpen} movie={m} />
		{/if}
	{/if}
{/if}
