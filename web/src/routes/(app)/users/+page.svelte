<script lang="ts">
	import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import TransferOwnershipDialog from '#lib/components/transfer-ownership-dialog.svelte';
	import UserFormDialog from '#lib/components/user-form-dialog.svelte';
	import * as AlertDialog from '#lib/components/ui/alert-dialog/index.js';
	import { Badge } from '#lib/components/ui/badge/index.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import * as Table from '#lib/components/ui/table/index.js';
	import { session } from '#lib/session.svelte.js';
	import { canChangeUser, formatLibraryScope } from '#lib/users.js';
	import type { components } from '#lib/api/schema.js';

	type User = components['schemas']['User'];

	const users = createQuery(() => ({
		queryKey: ['users'],
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/users');
			if (!data) throw new Error(error?.detail ?? 'The users could not be loaded.');
			return data;
		}
	}));
	const roles = createQuery(() => ({
		queryKey: ['assignable-roles'],
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/assignable-roles');
			if (!data) throw new Error(error?.detail ?? 'The roles could not be loaded.');
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
	const libraries = createQuery(() => ({
		queryKey: ['assignable-libraries'],
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/assignable-libraries');
			if (!data) throw new Error(error?.detail ?? 'The libraries could not be loaded.');
			return data;
		}
	}));

	let formOpen = $state(false);
	let editing = $state<User | null>(null);
	let deleting = $state<User | null>(null);
	let transferOpen = $state(false);
	let failure = $state('');

	const ready = $derived(!!roles.data && !!catalog.data);
	const caller = {
		get isOwner() {
			return session.me?.isOwner ?? false;
		}
	};

	const queryClient = useQueryClient();
	const remove = createMutation(() => ({
		mutationFn: async (user: User) => {
			const { error } = await api.DELETE('/api/v1/users/{id}', {
				params: { path: { id: user.id } }
			});
			if (error) throw new Error(error.detail ?? error.title);
		},
		onSuccess: () => (failure = ''),
		onError: (error) => (failure = error.message),
		onSettled: () => queryClient.invalidateQueries({ queryKey: ['users'] })
	}));
</script>

<svelte:head>
	<title>Users · Unseen</title>
</svelte:head>

<div class="flex items-center justify-between">
	<h1 class="text-2xl font-semibold">Users</h1>
	<div class="flex gap-2">
		{#if session.me?.isOwner}
			<Button variant="outline" disabled={!users.data} onclick={() => (transferOpen = true)}>
				Transfer ownership
			</Button>
		{/if}
		<Button
			disabled={!ready}
			onclick={() => {
				editing = null;
				formOpen = true;
			}}
		>
			New user
		</Button>
	</div>
</div>

{#if failure}
	<p role="alert" class="mt-4 text-sm text-destructive">{failure}</p>
{/if}

{#if users.isPending}
	<p class="mt-4 text-muted-foreground">Loading…</p>
{:else if users.isError}
	<p class="mt-4 text-destructive">{users.error.message}</p>
{:else}
	<Table.Root class="mt-4">
		<Table.Header>
			<Table.Row>
				<Table.Head>User</Table.Head>
				<Table.Head>Roles</Table.Head>
				<Table.Head>Status</Table.Head>
				<Table.Head class="text-end">Actions</Table.Head>
			</Table.Row>
		</Table.Header>
		<Table.Body>
			{#each users.data as user (user.id)}
				{@const changeable = canChangeUser(user, caller)}
				<Table.Row>
					<Table.Cell>
						<div class="font-medium">{user.name}</div>
						<div class="text-xs text-muted-foreground">{user.email}</div>
					</Table.Cell>
					<Table.Cell>
						<div class="flex flex-wrap gap-1">
							{#each user.roles as role (role.id)}
								{@const scope = formatLibraryScope(role.libraryIds, libraries.data ?? [])}
								<Badge variant={role.isOwner ? 'default' : 'secondary'}>
									{role.name}{#if scope}
										· {scope}
									{/if}
								</Badge>
							{:else}
								<span class="text-xs text-muted-foreground">None</span>
							{/each}
						</div>
					</Table.Cell>
					<Table.Cell>
						{#if user.disabled}<Badge variant="destructive">Disabled</Badge>{:else}Active{/if}
					</Table.Cell>
					<Table.Cell class="text-end">
						<div
							class="flex justify-end gap-2"
							title={changeable ? undefined : 'Only an Owner can change an Owner'}
						>
							<Button
								variant="outline"
								size="sm"
								disabled={!changeable || !ready}
								onclick={() => {
									editing = user;
									formOpen = true;
								}}
							>
								Edit
							</Button>
							<Button
								variant="outline"
								size="sm"
								disabled={!changeable}
								onclick={() => (deleting = user)}
							>
								Delete
							</Button>
						</div>
					</Table.Cell>
				</Table.Row>
			{/each}
		</Table.Body>
	</Table.Root>
{/if}

<UserFormDialog
	bind:open={formOpen}
	user={editing}
	roles={roles.data ?? []}
	catalog={catalog.data ?? []}
	libraries={libraries.data ?? []}
/>

<TransferOwnershipDialog
	bind:open={transferOpen}
	users={users.data ?? []}
	roles={roles.data ?? []}
/>

<AlertDialog.Root open={deleting !== null} onOpenChange={(open) => !open && (deleting = null)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Delete {deleting?.name}?</AlertDialog.Title>
			<AlertDialog.Description>
				The account, its sessions and its API keys are removed. This cannot be undone.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
			<AlertDialog.Action
				onclick={() => {
					if (deleting) remove.mutate(deleting);
					deleting = null;
				}}
			>
				Delete
			</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
