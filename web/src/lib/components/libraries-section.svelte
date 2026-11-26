<script lang="ts">
	import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import type { components } from '#lib/api/schema.js';
	import * as AlertDialog from '#lib/components/ui/alert-dialog/index.js';
	import { Badge } from '#lib/components/ui/badge/index.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import * as Dialog from '#lib/components/ui/dialog/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import * as Table from '#lib/components/ui/table/index.js';
	import { session } from '#lib/session.svelte.js';

	type Library = components['schemas']['Library'];

	const manage = $derived(session.can('settings:manage'));
	const canCreate = $derived(
		session.me?.isOwner ||
			session.me?.scopedPermissions.some(
				(entry) => entry.permission === 'settings:manage' && entry.libraryIds === null
			) ||
			false
	);

	const libraries = createQuery(() => ({
		queryKey: ['libraries'],
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/libraries');
			if (!data) throw new Error(error?.detail ?? 'The libraries could not be loaded.');
			return data;
		}
	}));

	let newName = $state('');
	let renaming = $state<Library | null>(null);
	let renameValue = $state('');
	let removing = $state<Library | null>(null);
	let failure = $state('');

	const queryClient = useQueryClient();
	const invalidate = () =>
		Promise.all([
			queryClient.invalidateQueries({ queryKey: ['libraries'] }),
			queryClient.invalidateQueries({ queryKey: ['assignable-libraries'] }),
			queryClient.invalidateQueries({ queryKey: ['root-folders'] })
		]);

	const create = createMutation(() => ({
		mutationFn: async () => {
			const { error } = await api.POST('/api/v1/libraries', { body: { name: newName } });
			if (error) throw new Error(error.detail ?? error.title);
		},
		onSuccess: () => {
			failure = '';
			newName = '';
		},
		onError: (error) => (failure = error.message),
		onSettled: () => invalidate()
	}));

	const rename = createMutation(() => ({
		mutationFn: async ({ id, name }: { id: number; name: string }) => {
			const { error } = await api.PATCH('/api/v1/libraries/{id}', {
				params: { path: { id } },
				body: { name }
			});
			if (error) throw new Error(error.detail ?? error.title);
		},
		onSuccess: () => {
			failure = '';
			renaming = null;
		},
		onError: (error) => (failure = error.message),
		onSettled: () => invalidate()
	}));

	const remove = createMutation(() => ({
		mutationFn: async (library: Library) => {
			const { error } = await api.DELETE('/api/v1/libraries/{id}', {
				params: { path: { id: library.id } }
			});
			if (error) throw new Error(error.detail ?? error.title);
		},
		onSuccess: () => (failure = ''),
		onError: (error) => (failure = error.message),
		onSettled: () => invalidate()
	}));
</script>

<section aria-labelledby="libraries-title">
	<h2 id="libraries-title" class="text-lg font-semibold">Libraries</h2>
	<p class="mt-1 text-sm text-muted-foreground">
		Libraries group root folders so accounts can be scoped to part of your media, such as a Kids
		library.
	</p>

	{#if failure}
		<p role="alert" class="mt-4 text-sm text-destructive">{failure}</p>
	{/if}

	{#if libraries.isPending}
		<p class="mt-4 text-muted-foreground">Loading…</p>
	{:else if libraries.isError}
		<p class="mt-4 text-destructive">{libraries.error.message}</p>
	{:else if libraries.data.length === 0}
		<p class="mt-4 text-muted-foreground">No libraries available.</p>
	{:else}
		<Table.Root class="mt-4">
			<Table.Header>
				<Table.Row>
					<Table.Head>Name</Table.Head>
					<Table.Head>Root folders</Table.Head>
					{#if manage}<Table.Head class="text-end">Actions</Table.Head>{/if}
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each libraries.data as library (library.id)}
					<Table.Row>
						<Table.Cell class="font-medium">{library.name}</Table.Cell>
						<Table.Cell>
							<Badge variant="secondary">{library.rootFolderIds.length}</Badge>
						</Table.Cell>
						{#if manage}
							<Table.Cell class="text-end">
								<div class="flex justify-end gap-2">
									<Button
										variant="outline"
										size="sm"
										onclick={() => {
											renaming = library;
											renameValue = library.name;
											failure = '';
										}}
									>
										Rename
									</Button>
									<Button
										variant="outline"
										size="sm"
										disabled={library.rootFolderIds.length > 0}
										title={library.rootFolderIds.length > 0
											? 'Move or remove all root folders before deleting this library'
											: undefined}
										onclick={() => (removing = library)}
									>
										Delete
									</Button>
								</div>
							</Table.Cell>
						{/if}
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	{/if}

	{#if canCreate}
		<form
			class="mt-4 grid max-w-xl gap-2"
			onsubmit={(event) => {
				event.preventDefault();
				create.mutate();
			}}
		>
			<Label for="new-library-name">Create a library</Label>
			<div class="flex gap-2">
				<Input id="new-library-name" required placeholder="Kids" bind:value={newName} />
				<Button type="submit" disabled={create.isPending}>
					{create.isPending ? 'Creating…' : 'Create'}
				</Button>
			</div>
		</form>
	{/if}
</section>

<Dialog.Root
	open={renaming !== null}
	onOpenChange={(open) => {
		if (!open) renaming = null;
	}}
>
	<Dialog.Content class="sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>Rename {renaming?.name}</Dialog.Title>
			<Dialog.Description>
				Users and root folders stay assigned to this library.
			</Dialog.Description>
		</Dialog.Header>
		<form
			class="grid gap-4"
			onsubmit={(event) => {
				event.preventDefault();
				if (renaming) rename.mutate({ id: renaming.id, name: renameValue });
			}}
		>
			<div class="grid gap-2">
				<Label for="rename-library-name">Name</Label>
				<Input id="rename-library-name" required bind:value={renameValue} />
			</div>
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (renaming = null)}>Cancel</Button>
				<Button type="submit" disabled={rename.isPending}>
					{rename.isPending ? 'Saving…' : 'Save'}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<AlertDialog.Root open={removing !== null} onOpenChange={(open) => !open && (removing = null)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Delete {removing?.name}?</AlertDialog.Title>
			<AlertDialog.Description>
				Scoped user grants for this library will lose access to it. A library that still holds root
				folders cannot be deleted.
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
				Delete
			</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
