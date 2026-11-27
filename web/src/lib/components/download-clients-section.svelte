<script lang="ts">
	import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import DownloadClientFormDialog from '#lib/components/download-client-form-dialog.svelte';
	import * as AlertDialog from '#lib/components/ui/alert-dialog/index.js';
	import { Badge } from '#lib/components/ui/badge/index.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import * as Table from '#lib/components/ui/table/index.js';
	import type { DownloadClient } from '#lib/download-clients.js';
	import { kindName } from '#lib/download-clients.js';
	import { session } from '#lib/session.svelte.js';

	const manage = $derived(session.can('clients:manage'));

	const clients = createQuery(() => ({
		queryKey: ['download-clients'],
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/download-clients');
			if (!data) throw new Error(error?.detail ?? 'The download clients could not be loaded.');
			return data;
		}
	}));

	let editing = $state<DownloadClient | null>(null);
	let formOpen = $state(false);
	let removing = $state<DownloadClient | null>(null);
	let failure = $state('');
	/** The last test result per client id: `ok` with the version, or the problem text. */
	let results = $state<Record<number, { ok: boolean; text: string }>>({});

	const queryClient = useQueryClient();
	const test = createMutation(() => ({
		mutationFn: async (client: DownloadClient) => {
			const { data, error } = await api.POST('/api/v1/download-clients/{id}/test', {
				params: { path: { id: client.id } }
			});
			results[client.id] = data
				? { ok: true, text: `Connected, ${kindName(client.kind)} ${data.version}` }
				: { ok: false, text: error?.detail ?? error?.title ?? 'The test failed.' };
		}
	}));
	const remove = createMutation(() => ({
		mutationFn: async (client: DownloadClient) => {
			const { error } = await api.DELETE('/api/v1/download-clients/{id}', {
				params: { path: { id: client.id } }
			});
			if (error) throw new Error(error.detail ?? error.title);
		},
		onSuccess: () => (failure = ''),
		onError: (error) => (failure = error.message),
		onSettled: () => queryClient.invalidateQueries({ queryKey: ['download-clients'] })
	}));

	function openForm(client: DownloadClient | null) {
		editing = client;
		formOpen = true;
	}
</script>

<section aria-labelledby="download-clients-title">
	<div class="flex items-center justify-between gap-4">
		<h2 id="download-clients-title" class="text-lg font-semibold">Download clients</h2>
		{#if manage}
			<Button size="sm" onclick={() => openForm(null)}>Add client</Button>
		{/if}
	</div>
	<p class="mt-1 text-sm text-muted-foreground">
		Unseenarr sends releases to qBittorrent or SABnzbd and follows them by category.
	</p>

	{#if failure}
		<p role="alert" class="mt-4 text-sm text-destructive">{failure}</p>
	{/if}

	{#if clients.isPending}
		<p class="mt-4 text-muted-foreground">Loading…</p>
	{:else if clients.isError}
		<p class="mt-4 text-destructive">{clients.error.message}</p>
	{:else if clients.data.length === 0}
		<p class="mt-4 text-muted-foreground">No download client yet.</p>
	{:else}
		<Table.Root class="mt-4">
			<Table.Header>
				<Table.Row>
					<Table.Head>Name</Table.Head>
					<Table.Head>Address</Table.Head>
					<Table.Head>Category</Table.Head>
					{#if manage}<Table.Head class="text-end">Actions</Table.Head>{/if}
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each clients.data as client (client.id)}
					<Table.Row>
						<Table.Cell>
							{client.name}
							{#if !client.enabled}<Badge variant="secondary" class="ms-2">Disabled</Badge>{/if}
							{#if client.passwordMustBeReentered || client.apiKeyMustBeReentered}
								<Badge variant="destructive" class="ms-2">Secret must be re-entered</Badge>
							{/if}
							{#if results[client.id]}
								<p
									role="status"
									class="mt-1 text-xs {results[client.id].ok
										? 'text-muted-foreground'
										: 'text-destructive'}"
								>
									{results[client.id].text}
								</p>
							{/if}
						</Table.Cell>
						<Table.Cell class="font-mono text-xs">{client.url}</Table.Cell>
						<Table.Cell class="font-mono text-xs">{client.category}</Table.Cell>
						{#if manage}
							<Table.Cell class="space-x-2 text-end">
								<Button
									variant="outline"
									size="sm"
									disabled={test.isPending}
									onclick={() => test.mutate(client)}
								>
									Test
								</Button>
								<Button variant="outline" size="sm" onclick={() => openForm(client)}>Edit</Button>
								<Button variant="outline" size="sm" onclick={() => (removing = client)}>
									Remove
								</Button>
							</Table.Cell>
						{/if}
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	{/if}
</section>

{#if manage}
	<DownloadClientFormDialog bind:open={formOpen} client={editing} />
{/if}

<AlertDialog.Root open={removing !== null} onOpenChange={(open) => !open && (removing = null)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Remove this download client?</AlertDialog.Title>
			<AlertDialog.Description>
				{removing?.name} is no longer used for new downloads. Downloads already in the client stay.
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
