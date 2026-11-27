<script lang="ts">
	import { resolve } from '$app/paths';
	import { createQuery } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import NoAccess from '#lib/components/no-access.svelte';
	import QueueRemoveDialog from '#lib/components/queue-remove-dialog.svelte';
	import { Badge } from '#lib/components/ui/badge/index.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import * as Table from '#lib/components/ui/table/index.js';
	import { qualityName } from '#lib/profiles.js';
	import {
		formatEta,
		formatSpeed,
		progressPercent,
		stateBadgeVariant,
		stateLabel,
		type QueueItem
	} from '#lib/queue.js';
	import { formatBytes } from '#lib/root-folders.js';
	import { session } from '#lib/session.svelte.js';

	const read = $derived(session.can('queue:read'));
	const manage = $derived(session.can('queue:manage'));

	const queue = createQuery(() => ({
		queryKey: ['queue'],
		enabled: read,
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/queue');
			if (!data) throw new Error(error?.detail ?? 'The activity queue could not be loaded.');
			return data;
		}
	}));

	let removing = $state<QueueItem | null>(null);
	let removeOpen = $state(false);

	function openRemove(item: QueueItem) {
		removing = item;
		removeOpen = true;
	}
</script>

<svelte:head>
	<title>Activity · Unseenarr</title>
</svelte:head>

{#if !read}
	<NoAccess anyOf={['queue:read']} />
{:else}
	<div class="flex flex-wrap items-center justify-between gap-2">
		<h1 class="text-2xl font-semibold">
			Activity
			{#if queue.data}
				<span class="text-base font-normal text-muted-foreground">
					{queue.data.length === 1 ? '1 active download' : `${queue.data.length} active downloads`}
				</span>
			{/if}
		</h1>
	</div>

	{#if queue.isPending}
		<p class="mt-4 text-muted-foreground">Loading…</p>
	{:else if queue.isError}
		<p role="alert" class="mt-4 text-destructive">{queue.error.message}</p>
	{:else if queue.data.length === 0}
		<p class="mt-4 text-muted-foreground">No active downloads.</p>
	{:else}
		<Table.Root class="mt-4">
			<Table.Header>
				<Table.Row>
					<Table.Head>Movie</Table.Head>
					<Table.Head>Release</Table.Head>
					<Table.Head>Client</Table.Head>
					<Table.Head class="w-44">Progress</Table.Head>
					<Table.Head class="text-end">Speed</Table.Head>
					<Table.Head class="text-end">ETA</Table.Head>
					<Table.Head>State</Table.Head>
					{#if manage}
						<Table.Head class="text-end">Actions</Table.Head>
					{/if}
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each queue.data as item (item.id)}
					{@const percent = progressPercent(item.progress)}
					<Table.Row>
						<Table.Cell class="font-medium">
							<a
								class="underline-offset-4 hover:underline"
								href={resolve('/(app)/library/[id]', { id: String(item.movieId) })}
							>
								{item.movieTitle}
							</a>
							<span class="text-xs text-muted-foreground">({item.movieYear})</span>
						</Table.Cell>
						<Table.Cell class="max-w-xs">
							<div class="truncate" title={item.releaseTitle}>{item.releaseTitle}</div>
							<div class="mt-0.5 flex items-center gap-1.5 text-xs text-muted-foreground">
								<Badge variant="outline" class="text-xs">{qualityName(item.qualityId)}</Badge>
								<span>{formatBytes(item.size)}</span>
							</div>
						</Table.Cell>
						<Table.Cell>{item.downloadClientName ?? '—'}</Table.Cell>
						<Table.Cell>
							<div class="flex items-center gap-2">
								<div
									class="h-2 flex-1 overflow-hidden rounded-full bg-secondary"
									role="progressbar"
									aria-label={`Progress for ${item.movieTitle}`}
									aria-valuemin={0}
									aria-valuemax={100}
									aria-valuenow={percent}
								>
									<div
										class="h-full bg-primary transition-[width] duration-300"
										style:width={`${percent}%`}
									></div>
								</div>
								<span class="w-10 text-end text-xs tabular-nums text-muted-foreground">
									{percent}%
								</span>
							</div>
						</Table.Cell>
						<Table.Cell class="text-end tabular-nums">{formatSpeed(item.speed)}</Table.Cell>
						<Table.Cell class="text-end tabular-nums">{formatEta(item.eta)}</Table.Cell>
						<Table.Cell>
							<Badge variant={stateBadgeVariant(item.state)}>{stateLabel(item.state)}</Badge>
							{#if item.error}
								<p class="mt-1 max-w-xs truncate text-xs text-destructive" title={item.error}>
									{item.error}
								</p>
							{/if}
						</Table.Cell>
						{#if manage}
							<Table.Cell class="text-end">
								<Button variant="outline" size="sm" onclick={() => openRemove(item)}>Remove</Button>
							</Table.Cell>
						{/if}
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	{/if}

	{#if manage}
		<QueueRemoveDialog bind:open={removeOpen} item={removing} />
	{/if}
{/if}
