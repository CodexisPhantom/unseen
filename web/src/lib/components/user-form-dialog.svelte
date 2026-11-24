<script lang="ts">
	import { createMutation, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import PermissionPicker from '#lib/components/permission-picker.svelte';
	import { Badge } from '#lib/components/ui/badge/index.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { Checkbox } from '#lib/components/ui/checkbox/index.js';
	import * as Dialog from '#lib/components/ui/dialog/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import { session, type Permission } from '#lib/session.svelte.js';
	import { canAssignRole, effectivePermissions } from '#lib/users.js';
	import type { components } from '#lib/api/schema.js';

	type User = components['schemas']['User'];
	type Role = components['schemas']['Role'];

	let {
		open = $bindable(false),
		user,
		roles,
		catalog
	}: { open: boolean; user: User | null; roles: Role[]; catalog: Permission[] } = $props();

	let name = $state('');
	let email = $state('');
	let password = $state('');
	let disabled = $state(false);
	let roleIds = $state<number[]>([]);
	let grants = $state<Permission[]>([]);
	let failure = $state('');
	let fieldErrors = $state<Record<string, string>>({});

	const queryClient = useQueryClient();
	const effective = $derived(effectivePermissions(roleIds, grants, roles, catalog));
	const caller = {
		get isOwner() {
			return session.me?.isOwner ?? false;
		},
		can: (permission: Permission) => session.can(permission)
	};

	// Start from the user being edited, or from an empty form.
	$effect(() => {
		if (!open) return;
		name = user?.name ?? '';
		email = user?.email ?? '';
		password = '';
		disabled = user?.disabled ?? false;
		roleIds = user?.roles.map((role) => role.id) ?? [];
		grants = user?.permissions ?? [];
		failure = '';
		fieldErrors = {};
	});

	function toggleRole(id: number, on: boolean) {
		roleIds = on ? [...roleIds, id] : roleIds.filter((entry) => entry !== id);
	}

	type Problem = components['schemas']['Problem'];
	const saveError = (problem: Problem) =>
		new Error(problem.detail ?? problem.title, { cause: problem });

	const save = createMutation(() => ({
		mutationFn: async () => {
			if (!user) {
				const { error } = await api.POST('/api/v1/users', {
					body: { name, email, password, roleIds }
				});
				if (error) throw saveError(error);
				return;
			}
			const { error } = await api.PATCH('/api/v1/users/{id}', {
				params: { path: { id: user.id } },
				body: { name, email, disabled, roleIds, ...(password ? { password } : {}) }
			});
			if (error) throw saveError(error);
			const same =
				grants.length === user.permissions.length &&
				grants.every((permission) => user.permissions.includes(permission));
			if (!same) {
				const { error } = await api.PUT('/api/v1/users/{id}/permissions', {
					params: { path: { id: user.id } },
					body: { permissions: grants }
				});
				if (error) throw saveError(error);
			}
		},
		onSuccess: async () => {
			await queryClient.invalidateQueries({ queryKey: ['users'] });
			open = false;
		},
		onError: (error) => {
			// An error that names a field shows next to it; the rest shows above the buttons.
			const problem = (error.cause as Problem | undefined) ?? null;
			fieldErrors = Object.fromEntries((problem?.errors ?? []).map((e) => [e.field, e.message]));
			failure = problem?.errors?.length ? '' : error.message;
		}
	}));
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-xl">
		<Dialog.Header>
			<Dialog.Title>{user ? `Edit ${user.name}` : 'New user'}</Dialog.Title>
			<Dialog.Description>
				You can give only roles and permissions you hold. Changes apply on the next request of the
				user.
			</Dialog.Description>
		</Dialog.Header>
		<form
			class="grid gap-4"
			onsubmit={(event) => {
				event.preventDefault();
				save.mutate();
			}}
		>
			<div class="grid gap-2">
				<Label for="user-name">Name</Label>
				<Input id="user-name" required bind:value={name} />
				{#if fieldErrors.name}<p class="text-sm text-destructive">{fieldErrors.name}</p>{/if}
			</div>
			<div class="grid gap-2">
				<Label for="user-email">Email</Label>
				<Input id="user-email" type="email" required autocomplete="off" bind:value={email} />
				{#if fieldErrors.email}<p class="text-sm text-destructive">{fieldErrors.email}</p>{/if}
			</div>
			<div class="grid gap-2">
				<Label for="user-password">{user ? 'New password' : 'Password'}</Label>
				<Input
					id="user-password"
					type="password"
					required={!user}
					autocomplete="new-password"
					bind:value={password}
				/>
				<p class="text-xs text-muted-foreground">
					{user
						? 'Leave empty to keep the password. A new password signs the user out.'
						: '10 to 128 characters.'}
				</p>
				{#if fieldErrors.password}
					<p class="text-sm text-destructive">{fieldErrors.password}</p>
				{/if}
			</div>

			<fieldset class="grid gap-2">
				<legend class="mb-1 text-sm font-medium">Roles</legend>
				{#each roles as role (role.id)}
					<Label class="gap-2 font-normal {canAssignRole(role, caller) ? '' : 'opacity-50'}">
						<Checkbox
							disabled={!canAssignRole(role, caller)}
							checked={roleIds.includes(role.id)}
							onCheckedChange={(on) => toggleRole(role.id, on === true)}
						/>
						{role.name}
						{#if role.isOwner}<Badge variant="secondary">Built in</Badge>{/if}
					</Label>
				{/each}
				{#if fieldErrors.roleIds}
					<p class="text-sm text-destructive">{fieldErrors.roleIds}</p>
				{/if}
			</fieldset>

			{#if user}
				<Label class="gap-2 font-normal">
					<Checkbox bind:checked={disabled} />
					Disabled: the user is signed out and cannot sign in
				</Label>

				<fieldset class="grid gap-3">
					<legend class="mb-1 text-sm font-medium">Direct permissions</legend>
					<PermissionPicker {catalog} bind:selected={grants} />
				</fieldset>

				<div class="grid gap-2">
					<span class="text-sm font-medium">Effective permissions</span>
					{#if effective.length === 0}
						<p class="text-sm text-muted-foreground">None. The user sees nothing.</p>
					{:else}
						<ul class="grid gap-1 text-xs">
							{#each effective as entry (entry.permission)}
								<li class="flex flex-wrap items-center gap-2">
									<span class="font-mono">{entry.permission}</span>
									{#each entry.sources as source (source)}
										<Badge variant="outline">{source}</Badge>
									{/each}
								</li>
							{/each}
						</ul>
					{/if}
				</div>
			{/if}

			{#if failure}
				<p role="alert" class="text-sm text-destructive">{failure}</p>
			{/if}
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (open = false)}>Cancel</Button>
				<Button type="submit" disabled={save.isPending}>Save</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
