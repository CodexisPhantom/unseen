<script lang="ts">
	import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import RoleFormDialog from '#lib/components/role-form-dialog.svelte';
	import * as AlertDialog from '#lib/components/ui/alert-dialog/index.js';
	import { Badge } from '#lib/components/ui/badge/index.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import * as Table from '#lib/components/ui/table/index.js';
	import { missingPermissions } from '#lib/roles.js';
	import { session } from '#lib/session.svelte.js';
	import type { components } from '#lib/api/schema.js';

	type Role = components['schemas']['Role'];

	const roles = createQuery(() => ({
		queryKey: ['roles'],
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/roles');
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

	let formOpen = $state(false);
	let editing = $state<Role | null>(null);
	let deleting = $state<Role | null>(null);
	let failure = $state('');

	const queryClient = useQueryClient();
	const remove = createMutation(() => ({
		mutationFn: async (role: Role) => {
			const { error } = await api.DELETE('/api/v1/roles/{id}', {
				params: { path: { id: role.id }, query: { confirm: role.users > 0 } }
			});
			if (error) throw new Error(error.detail ?? error.title);
		},
		onSuccess: () => {
			failure = '';
		},
		onError: (error) => (failure = error.message),
		// A failed delete may mean the role changed: show what is there now.
		onSettled: () => queryClient.invalidateQueries({ queryKey: ['roles'] })
	}));

	const lacking = (role: Role) =>
		missingPermissions(role.permissions, (permission) => session.can(permission));
</script>

<svelte:head>
	<title>Roles · Unseenarr</title>
</svelte:head>

<div class="flex items-center justify-between">
	<h1 class="text-2xl font-semibold">Roles</h1>
	<Button
		disabled={!catalog.data}
		onclick={() => {
			editing = null;
			formOpen = true;
		}}
	>
		New role
	</Button>
</div>

{#if failure}
	<p role="alert" class="mt-4 text-sm text-destructive">{failure}</p>
{/if}

{#if roles.isPending}
	<p class="mt-4 text-muted-foreground">Loading…</p>
{:else if roles.isError}
	<p class="mt-4 text-destructive">{roles.error.message}</p>
{:else}
	<Table.Root class="mt-4">
		<Table.Header>
			<Table.Row>
				<Table.Head>Name</Table.Head>
				<Table.Head>Permissions</Table.Head>
				<Table.Head>Users</Table.Head>
				<Table.Head class="text-end">Actions</Table.Head>
			</Table.Row>
		</Table.Header>
		<Table.Body>
			{#each roles.data as role (role.id)}
				{@const missing = lacking(role)}
				<Table.Row>
					<Table.Cell>
						<div class="font-medium">
							{role.name}
							{#if role.isOwner}<Badge variant="secondary">Built in</Badge>{/if}
						</div>
						{#if role.description}
							<div class="text-xs text-muted-foreground">{role.description}</div>
						{/if}
					</Table.Cell>
					<Table.Cell>{role.isOwner ? 'All' : role.permissions.length}</Table.Cell>
					<Table.Cell>{role.users}</Table.Cell>
					<Table.Cell class="text-end">
						{#if role.isOwner}
							<span class="text-xs text-muted-foreground">Read-only</span>
						{:else}
							<div
								class="flex justify-end gap-2"
								title={missing.length ? `Needs ${missing.join(', ')}, which you do not hold` : undefined}
							>
								<Button
									variant="outline"
									size="sm"
									disabled={missing.length > 0 || !catalog.data}
									onclick={() => {
										editing = role;
										formOpen = true;
									}}
								>
									Edit
								</Button>
								<Button
									variant="outline"
									size="sm"
									disabled={missing.length > 0}
									onclick={() => (deleting = role)}
								>
									Delete
								</Button>
							</div>
						{/if}
					</Table.Cell>
				</Table.Row>
			{/each}
		</Table.Body>
	</Table.Root>
{/if}

<RoleFormDialog bind:open={formOpen} role={editing} catalog={catalog.data ?? []} />

<AlertDialog.Root open={deleting !== null} onOpenChange={(open) => !open && (deleting = null)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Delete {deleting?.name}?</AlertDialog.Title>
			<AlertDialog.Description>
				{#if deleting && deleting.users > 0}
					{deleting.users} user{deleting.users === 1 ? '' : 's'} hold this role and lose its
					permissions at once.
				{:else}
					No user holds this role.
				{/if}
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
