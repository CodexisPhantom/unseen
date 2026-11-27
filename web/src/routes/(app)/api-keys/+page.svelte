<script lang="ts">
	import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import { lastUsed, maskedKey } from '#lib/api-keys.js';
	import ApiKeyFormDialog from '#lib/components/api-key-form-dialog.svelte';
	import * as AlertDialog from '#lib/components/ui/alert-dialog/index.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import * as Table from '#lib/components/ui/table/index.js';
	import { session } from '#lib/session.svelte.js';
	import type { components } from '#lib/api/schema.js';

	type ApiKey = components['schemas']['ApiKey'];

	const own = createQuery(() => ({
		queryKey: ['api-keys', 'own'],
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/me/api-keys');
			if (!data) throw new Error(error?.detail ?? 'The keys could not be loaded.');
			return data;
		}
	}));
	const manage = $derived(session.can('apikeys:manage'));
	const everyone = createQuery(() => ({
		queryKey: ['api-keys', 'all'],
		enabled: manage,
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/api-keys');
			if (!data) throw new Error(error?.detail ?? 'The keys could not be loaded.');
			return data;
		}
	}));
	const catalog = createQuery(() => ({
		queryKey: ['permissions'],
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/permissions');
			if (!data) throw new Error(error?.detail ?? 'The permissions could not be loaded.');
			return data;
		}
	}));

	let formOpen = $state(false);
	let revoking = $state<{ key: ApiKey; others: boolean } | null>(null);
	let failure = $state('');

	const queryClient = useQueryClient();
	const revoke = createMutation(() => ({
		mutationFn: async ({ key, others }: { key: ApiKey; others: boolean }) => {
			const params = { path: { id: key.id } };
			const { error } = others
				? await api.DELETE('/api/v1/api-keys/{id}', { params })
				: await api.DELETE('/api/v1/me/api-keys/{id}', { params });
			if (error) throw new Error(error.detail ?? error.title);
		},
		onSuccess: () => (failure = ''),
		onError: (error) => (failure = error.message),
		onSettled: () => queryClient.invalidateQueries({ queryKey: ['api-keys'] })
	}));

	const others = $derived(
		(everyone.data ?? []).filter((key) => key.userId !== session.me?.user.id)
	);
</script>

{#snippet keys(list: ApiKey[], isOthers: boolean)}
	<Table.Root class="mt-4">
		<Table.Header>
			<Table.Row>
				<Table.Head>Name</Table.Head>
				{#if isOthers}<Table.Head>User</Table.Head>{/if}
				<Table.Head>Key</Table.Head>
				<Table.Head>Permissions</Table.Head>
				<Table.Head>Last used</Table.Head>
				<Table.Head class="text-end">Actions</Table.Head>
			</Table.Row>
		</Table.Header>
		<Table.Body>
			{#each list as key (key.id)}
				<Table.Row>
					<Table.Cell class="font-medium">{key.name}</Table.Cell>
					{#if isOthers}<Table.Cell>{key.userName}</Table.Cell>{/if}
					<Table.Cell class="font-mono text-xs">{maskedKey(key.prefix)}</Table.Cell>
					<Table.Cell title={key.permissions.join(', ')}>{key.permissions.length}</Table.Cell>
					<Table.Cell>{lastUsed(key.lastUsedAt)}</Table.Cell>
					<Table.Cell class="text-end">
						<Button variant="outline" size="sm" onclick={() => (revoking = { key, others: isOthers })}>
							Revoke
						</Button>
					</Table.Cell>
				</Table.Row>
			{/each}
		</Table.Body>
	</Table.Root>
{/snippet}

<svelte:head>
	<title>API keys · Unseenarr</title>
</svelte:head>

<div class="flex items-center justify-between">
	<h1 class="text-2xl font-semibold">API keys</h1>
	<Button disabled={!catalog.data} onclick={() => (formOpen = true)}>New key</Button>
</div>
<p class="mt-1 text-sm text-muted-foreground">
	Companion tools sign in with a key. A key can do only what you ticked for it, and never more
	than you can do yourself.
</p>

{#if failure}
	<p role="alert" class="mt-4 text-sm text-destructive">{failure}</p>
{/if}

{#if own.isPending}
	<p class="mt-4 text-muted-foreground">Loading…</p>
{:else if own.isError}
	<p class="mt-4 text-destructive">{own.error.message}</p>
{:else if own.data.length === 0}
	<p class="mt-4 text-muted-foreground">You have no API keys.</p>
{:else}
	{@render keys(own.data, false)}
{/if}

{#if manage}
	<h2 class="mt-8 text-lg font-semibold">Keys of other users</h2>
	{#if everyone.isPending}
		<p class="mt-4 text-muted-foreground">Loading…</p>
	{:else if everyone.isError}
		<p class="mt-4 text-destructive">{everyone.error.message}</p>
	{:else if others.length === 0}
		<p class="mt-4 text-muted-foreground">No other user has a key.</p>
	{:else}
		{@render keys(others, true)}
	{/if}
{/if}

<ApiKeyFormDialog bind:open={formOpen} catalog={catalog.data ?? []} />

<AlertDialog.Root open={revoking !== null} onOpenChange={(open) => !open && (revoking = null)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Revoke {revoking?.key.name}?</AlertDialog.Title>
			<AlertDialog.Description>
				Every tool that uses this key stops working at once. This cannot be undone.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
			<AlertDialog.Action
				onclick={() => {
					if (revoking) revoke.mutate(revoking);
					revoking = null;
				}}
			>
				Revoke
			</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
