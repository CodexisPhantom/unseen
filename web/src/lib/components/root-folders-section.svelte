<script lang="ts">
	import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import type { components } from '#lib/api/schema.js';
	import FolderBrowserDialog from '#lib/components/folder-browser-dialog.svelte';
	import * as AlertDialog from '#lib/components/ui/alert-dialog/index.js';
	import { Badge } from '#lib/components/ui/badge/index.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import * as Table from '#lib/components/ui/table/index.js';
	import { formatBytes } from '#lib/root-folders.js';
	import { session } from '#lib/session.svelte.js';

	type RootFolder = components['schemas']['RootFolder'];

	const manage = $derived(session.can('settings:manage'));

	const folders = createQuery(() => ({
		queryKey: ['root-folders'],
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/root-folders');
			if (!data) throw new Error(error?.detail ?? 'The root folders could not be loaded.');
			return data;
		}
	}));

	let path = $state('');
	let browsing = $state(false);
	let removing = $state<RootFolder | null>(null);
	let failure = $state('');

	const queryClient = useQueryClient();
	const add = createMutation(() => ({
		mutationFn: async () => {
			const { error } = await api.POST('/api/v1/root-folders', { body: { path } });
			if (error) throw new Error(error.detail ?? error.title);
		},
		onSuccess: () => {
			failure = '';
			path = '';
		},
		onError: (error) => (failure = error.message),
		onSettled: () => queryClient.invalidateQueries({ queryKey: ['root-folders'] })
	}));
	const remove = createMutation(() => ({
		mutationFn: async (folder: RootFolder) => {
			const { error } = await api.DELETE('/api/v1/root-folders/{id}', {
				params: { path: { id: folder.id } }
			});
			if (error) throw new Error(error.detail ?? error.title);
		},
		onSuccess: () => (failure = ''),
		onError: (error) => (failure = error.message),
		onSettled: () => queryClient.invalidateQueries({ queryKey: ['root-folders'] })
	}));
</script>

<section aria-labelledby="root-folders-title">
	<h2 id="root-folders-title" class="text-lg font-semibold">Root folders</h2>
	<p class="mt-1 text-sm text-muted-foreground">
		Movies live in root folders. Use the same paths as in Radarr to adopt an existing library.
		Removing a root folder never touches the files on disk.
	</p>

	{#if failure}
		<p role="alert" class="mt-4 text-sm text-destructive">{failure}</p>
	{/if}

	{#if folders.isPending}
		<p class="mt-4 text-muted-foreground">Loading…</p>
	{:else if folders.isError}
		<p class="mt-4 text-destructive">{folders.error.message}</p>
	{:else if folders.data.length === 0}
		<p class="mt-4 text-muted-foreground">No root folder yet.</p>
	{:else}
		<Table.Root class="mt-4">
			<Table.Header>
				<Table.Row>
					<Table.Head>Path</Table.Head>
					<Table.Head>Free space</Table.Head>
					<Table.Head>Unmapped folders</Table.Head>
					{#if manage}<Table.Head class="text-end">Actions</Table.Head>{/if}
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each folders.data as folder (folder.id)}
					<Table.Row>
						<Table.Cell class="font-mono text-xs">
							{folder.path}
							{#if !folder.accessible}
								<Badge variant="destructive" class="ms-2">Not accessible</Badge>
							{/if}
						</Table.Cell>
						<Table.Cell>{formatBytes(folder.freeSpace)}</Table.Cell>
						<Table.Cell title={folder.unmappedFolders.join('\n')}>
							{folder.unmappedFolders.length}
						</Table.Cell>
						{#if manage}
							<Table.Cell class="text-end">
								<Button variant="outline" size="sm" onclick={() => (removing = folder)}>
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
		<form
			class="mt-4 grid max-w-xl gap-2"
			onsubmit={(event) => {
				event.preventDefault();
				add.mutate();
			}}
		>
			<Label for="root-folder-path">Add a root folder</Label>
			<div class="flex gap-2">
				<Input
					id="root-folder-path"
					required
					placeholder="/data/media/movies"
					class="font-mono"
					bind:value={path}
				/>
				<Button type="button" variant="outline" onclick={() => (browsing = true)}>Browse</Button>
				<Button type="submit" disabled={add.isPending}>Add</Button>
			</div>
			<p class="text-xs text-muted-foreground">
				The folder must exist, Unseen must be able to write to it, and it must not lie inside
				another root folder.
			</p>
		</form>
		<FolderBrowserDialog bind:open={browsing} start={path} onselect={(chosen) => (path = chosen)} />
	{/if}
</section>

<AlertDialog.Root open={removing !== null} onOpenChange={(open) => !open && (removing = null)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Remove this root folder?</AlertDialog.Title>
			<AlertDialog.Description>
				<code class="break-all">{removing?.path}</code> is no longer used for new movies. The files on
				disk stay. A root folder that holds movies cannot be removed.
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
