<script lang="ts">
	import { createQuery } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import NoAccess from '#lib/components/no-access.svelte';
	import QualitySizeRow from '#lib/components/quality-size-row.svelte';
	import * as Table from '#lib/components/ui/table/index.js';
	import { session } from '#lib/session.svelte.js';

	const read = $derived(session.can('profiles:read'));
	const manage = $derived(session.can('profiles:manage'));

	const definitions = createQuery(() => ({
		queryKey: ['quality-definitions'],
		enabled: read,
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/quality-definitions');
			if (!data) throw new Error(error?.detail ?? 'The quality sizes could not be loaded.');
			return data;
		}
	}));
</script>

<svelte:head>
	<title>Quality sizes · Unseen</title>
</svelte:head>

{#if !read}
	<NoAccess anyOf={['profiles:read']} />
{:else}
	<h1 class="text-2xl font-semibold">Quality sizes</h1>
	<p class="mt-1 max-w-2xl text-sm text-muted-foreground">
		The size a release of each quality may have, in megabytes per minute of the movie's runtime.
		The defaults are Radarr's. A release below the minimum or above the maximum is rejected. An
		empty maximum is no limit. A movie without a runtime is not checked.
	</p>

	{#if definitions.isPending}
		<p class="mt-4 text-muted-foreground">Loading…</p>
	{:else if definitions.isError}
		<p class="mt-4 text-destructive">{definitions.error.message}</p>
	{:else}
		<Table.Root class="mt-4">
			<Table.Header>
				<Table.Row>
					<Table.Head>Quality</Table.Head>
					<Table.Head>Minimum</Table.Head>
					<Table.Head>Preferred</Table.Head>
					<Table.Head>Maximum</Table.Head>
					{#if manage}<Table.Head class="text-end">Actions</Table.Head>{/if}
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each definitions.data as definition (definition.qualityId)}
					<QualitySizeRow {definition} {manage} />
				{/each}
			</Table.Body>
		</Table.Root>
	{/if}
{/if}
