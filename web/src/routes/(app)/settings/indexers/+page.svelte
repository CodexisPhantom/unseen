<script lang="ts">
	import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import IndexerFormDialog from '#lib/components/indexer-form-dialog.svelte';
	import NoAccess from '#lib/components/no-access.svelte';
	import * as AlertDialog from '#lib/components/ui/alert-dialog/index.js';
	import { Badge } from '#lib/components/ui/badge/index.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import * as Table from '#lib/components/ui/table/index.js';
	import { testSummary, type Indexer } from '#lib/indexers.js';
	import { session } from '#lib/session.svelte.js';

	const read = $derived(session.can('indexers:read'));
	const manage = $derived(session.can('indexers:manage'));

	const indexers = createQuery(() => ({
		queryKey: ['indexers'],
		enabled: read,
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/indexers');
			if (!data) throw new Error(error?.detail ?? 'The indexers could not be loaded.');
			return data;
		}
	}));

	let editing = $state<Indexer | null>(null);
	let formOpen = $state(false);
	let removing = $state<Indexer | null>(null);
	let failure = $state('');
	/** The last test result per indexer id: what it learned, or the problem text. */
	let results = $state<Record<number, { ok: boolean; text: string }>>({});
	let testing = $state<number | null>(null);

	const queryClient = useQueryClient();
	const test = createMutation(() => ({
		mutationFn: async (indexer: Indexer) => {
			testing = indexer.id;
			const { data, error } = await api.POST('/api/v1/indexers/{id}/test', {
				params: { path: { id: indexer.id } }
			});
			results[indexer.id] = data
				? { ok: true, text: testSummary(data) }
				: { ok: false, text: error?.detail ?? error?.title ?? 'The test failed.' };
		},
		onSettled: () => (testing = null)
	}));
	const remove = createMutation(() => ({
		mutationFn: async (indexer: Indexer) => {
			const { error } = await api.DELETE('/api/v1/indexers/{id}', {
				params: { path: { id: indexer.id } }
			});
			if (error) throw new Error(error.detail ?? error.title);
		},
		onSuccess: () => (failure = ''),
		onError: (error) => (failure = error.message),
		onSettled: () => queryClient.invalidateQueries({ queryKey: ['indexers'] })
	}));

	function openForm(indexer: Indexer | null) {
		editing = indexer;
		formOpen = true;
	}
</script>

<svelte:head>
	<title>Indexers · Unseen</title>
</svelte:head>

{#if !read}
	<NoAccess anyOf={['indexers:read']} />
{:else}
	<div class="flex items-center justify-between gap-4">
		<h1 class="text-2xl font-semibold">Indexers</h1>
		{#if manage}
			<Button onclick={() => openForm(null)}>Add indexer</Button>
		{/if}
	</div>
	<p class="mt-1 text-sm text-muted-foreground">
		Unseen searches these Torznab indexers for releases. A lower priority number wins when two
		releases tie.
	</p>

	{#if failure}
		<p role="alert" class="mt-4 text-sm text-destructive">{failure}</p>
	{/if}

	{#if indexers.isPending}
		<p class="mt-4 text-muted-foreground">Loading…</p>
	{:else if indexers.isError}
		<p class="mt-4 text-destructive">{indexers.error.message}</p>
	{:else if indexers.data.length === 0}
		<p class="mt-4 text-muted-foreground">No indexer yet.</p>
	{:else}
		<Table.Root class="mt-4">
			<Table.Header>
				<Table.Row>
					<Table.Head>Name</Table.Head>
					<Table.Head>URL</Table.Head>
					<Table.Head>Categories</Table.Head>
					<Table.Head>Priority</Table.Head>
					{#if manage}<Table.Head class="text-end">Actions</Table.Head>{/if}
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each indexers.data as indexer (indexer.id)}
					<Table.Row>
						<Table.Cell>
							{indexer.name}
							{#if !indexer.enabled}<Badge variant="secondary" class="ms-2">Disabled</Badge>{/if}
							{#if indexer.apiKeyMustBeReentered}
								<Badge variant="destructive" class="ms-2">API key must be re-entered</Badge>
							{/if}
							{#if results[indexer.id]}
								<p
									role="status"
									class="mt-1 text-xs {results[indexer.id].ok
										? 'text-muted-foreground'
										: 'text-destructive'}"
								>
									{results[indexer.id].text}
								</p>
							{/if}
						</Table.Cell>
						<Table.Cell class="font-mono text-xs">{indexer.url}</Table.Cell>
						<Table.Cell class="font-mono text-xs">{indexer.categories.join(', ')}</Table.Cell>
						<Table.Cell>{indexer.priority}</Table.Cell>
						{#if manage}
							<Table.Cell class="space-x-2 text-end">
								<Button
									variant="outline"
									size="sm"
									disabled={test.isPending}
									onclick={() => test.mutate(indexer)}
								>
									{testing === indexer.id ? 'Testing…' : 'Test'}
								</Button>
								<Button variant="outline" size="sm" onclick={() => openForm(indexer)}>Edit</Button>
								<Button variant="outline" size="sm" onclick={() => (removing = indexer)}>
									Remove
								</Button>
							</Table.Cell>
						{/if}
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	{/if}

	{#if manage}
		<IndexerFormDialog bind:open={formOpen} indexer={editing} />
	{/if}

	<AlertDialog.Root open={removing !== null} onOpenChange={(open) => !open && (removing = null)}>
		<AlertDialog.Content>
			<AlertDialog.Header>
				<AlertDialog.Title>Remove this indexer?</AlertDialog.Title>
				<AlertDialog.Description>
					Unseen no longer searches {removing?.name}.
				</AlertDialog.Description>
			</AlertDialog.Header>
			<AlertDialog.Footer>
				<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
				<AlertDialog.Action
					onclick={() => {
						if (removing) remove.mutate(removing);
						removing = null;
					}}
				>
					Remove
				</AlertDialog.Action>
			</AlertDialog.Footer>
		</AlertDialog.Content>
	</AlertDialog.Root>
{/if}
