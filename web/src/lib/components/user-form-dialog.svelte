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
	import { canAssignRole, effectivePermissions, formatLibraryScope } from '#lib/users.js';
	import type { components } from '#lib/api/schema.js';

	type User = components['schemas']['User'];
	type Role = components['schemas']['Role'];
	type Library = components['schemas']['Library'];

	let {
		open = $bindable(false),
		user,
		roles,
		catalog,
		libraries = []
	}: {
		open: boolean;
		user: User | null;
		roles: Role[];
		catalog: Permission[];
		libraries?: Library[];
	} = $props();

	let name = $state('');
	let email = $state('');
	let password = $state('');
	let disabled = $state(false);
	let roleIds = $state<number[]>([]);
	/** `null` means all libraries; an array restricts the role to those library ids. */
	let roleScopes = $state<Record<number, number[] | null>>({});
	let grants = $state<Permission[]>([]);
	/** `null` means all libraries; an array restricts the direct grant to those library ids. */
	let grantScopes = $state<Partial<Record<Permission, number[] | null>>>({});
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
		roleScopes = Object.fromEntries(
			(user?.roles ?? []).map((role) => [role.id, role.libraryIds ?? null])
		);
		grants = user?.permissions ?? [];
		grantScopes = Object.fromEntries(
			(user?.scopedPermissions ?? []).map((entry) => [
				entry.permission,
				entry.libraryIds ?? null
			])
		);
		failure = '';
		fieldErrors = {};
	});

	function toggleRole(id: number, on: boolean) {
		if (on) {
			roleIds = [...roleIds, id];
			if (!(id in roleScopes)) {
				roleScopes = { ...roleScopes, [id]: null };
			}
		} else {
			roleIds = roleIds.filter((entry) => entry !== id);
		}
	}

	function setRoleAllLibraries(roleId: number, all: boolean) {
		roleScopes = {
			...roleScopes,
			[roleId]: all ? null : libraries.map((lib) => lib.id)
		};
	}

	function toggleRoleLibrary(roleId: number, libraryId: number, on: boolean) {
		const current = roleScopes[roleId] ?? [];
		const next = on
			? [...current, libraryId]
			: current.filter((entry) => entry !== libraryId);
		roleScopes = { ...roleScopes, [roleId]: next };
	}

	function setGrantAllLibraries(permission: Permission, all: boolean) {
		grantScopes = {
			...grantScopes,
			[permission]: all ? null : libraries.map((lib) => lib.id)
		};
	}

	function toggleGrantLibrary(permission: Permission, libraryId: number, on: boolean) {
		const current = grantScopes[permission] ?? [];
		const next = on
			? [...current, libraryId]
			: current.filter((entry) => entry !== libraryId);
		grantScopes = { ...grantScopes, [permission]: next };
	}

	function scopeLabelForRole(roleId: number): string | null {
		return formatLibraryScope(roleScopes[roleId] ?? null, libraries);
	}

	function scopeLabelForGrant(permission: Permission): string | null {
		return formatLibraryScope(grantScopes[permission] ?? null, libraries);
	}

	type Problem = components['schemas']['Problem'];
	const saveError = (problem: Problem) =>
		new Error(problem.detail ?? problem.title, { cause: problem });

	const save = createMutation(() => ({
		mutationFn: async () => {
			const roleAssignments = roleIds.map((roleId) => ({
				roleId,
				libraryIds: roleScopes[roleId] ?? null
			}));
			if (!user) {
				const { error } = await api.POST('/api/v1/users', {
					body: { name, email, password, roleAssignments }
				});
				if (error) throw saveError(error);
				return;
			}
			const { error } = await api.PATCH('/api/v1/users/{id}', {
				params: { path: { id: user.id } },
				body: { name, email, disabled, roleAssignments, ...(password ? { password } : {}) }
			});
			if (error) throw saveError(error);
			const scopedPermissions = grants.map((permission) => ({
				permission,
				libraryIds: grantScopes[permission] ?? null
			}));
			const { error: grantError } = await api.PUT('/api/v1/users/{id}/permissions', {
				params: { path: { id: user.id } },
				body: { scopedPermissions }
			});
			if (grantError) throw saveError(grantError);
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
				You can give only roles, permissions and library scopes you hold. Changes apply on the next
				request of the user.
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
					{@const checked = roleIds.includes(role.id)}
					{@const assignable = canAssignRole(role, caller)}
					{@const scopedLibs = roleScopes[role.id] ?? null}
					<div class="grid gap-1.5">
						<Label class="gap-2 font-normal {assignable ? '' : 'opacity-50'}">
							<Checkbox
								disabled={!assignable}
								{checked}
								onCheckedChange={(on) => toggleRole(role.id, on === true)}
							/>
							{role.name}
							{#if role.isOwner}<Badge variant="secondary">Built in</Badge>{/if}
						</Label>
						{#if checked && !role.isOwner && libraries.length > 0}
							<div class="ms-6 grid gap-1.5 rounded-md border p-2.5 text-xs">
								<Label class="gap-2 font-normal">
									<Checkbox
										disabled={!assignable}
										checked={scopedLibs === null}
										onCheckedChange={(on) => setRoleAllLibraries(role.id, on === true)}
									/>
									All libraries
								</Label>
								{#if scopedLibs !== null}
									<div class="flex flex-wrap gap-3 pt-1">
										{#each libraries as library (library.id)}
											<Label class="gap-1.5 font-normal">
												<Checkbox
													disabled={!assignable}
													checked={scopedLibs.includes(library.id)}
													onCheckedChange={(on) =>
														toggleRoleLibrary(role.id, library.id, on === true)}
												/>
												{library.name}
											</Label>
										{/each}
									</div>
									{#if scopedLibs.length === 0}
										<p class="text-destructive">
											No library selected. This role will not grant access to any library.
										</p>
									{/if}
								{/if}
							</div>
						{/if}
					</div>
				{/each}
				{#if fieldErrors.roleIds || fieldErrors.roleAssignments}
					<p class="text-sm text-destructive">
						{fieldErrors.roleIds ?? fieldErrors.roleAssignments}
					</p>
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
					{#if grants.length > 0 && libraries.length > 0}
						<div class="grid gap-2 rounded-md border p-3">
							<span class="text-xs font-medium text-muted-foreground">
								Library scope for direct permissions
							</span>
							{#each grants as permission (permission)}
								{@const scopedLibs = grantScopes[permission] ?? null}
								<div class="grid gap-1 text-xs">
									<div class="flex flex-wrap items-center gap-3">
										<span class="font-mono">{permission}</span>
										<Label class="gap-1.5 font-normal">
											<Checkbox
												checked={scopedLibs === null}
												onCheckedChange={(on) => setGrantAllLibraries(permission, on === true)}
											/>
											All libraries
										</Label>
									</div>
									{#if scopedLibs !== null}
										<div class="ms-4 flex flex-wrap gap-3">
											{#each libraries as library (library.id)}
												<Label class="gap-1.5 font-normal">
													<Checkbox
														checked={scopedLibs.includes(library.id)}
														onCheckedChange={(on) =>
															toggleGrantLibrary(permission, library.id, on === true)}
													/>
													{library.name}
												</Label>
											{/each}
										</div>
									{/if}
								</div>
							{/each}
						</div>
					{/if}
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
										{@const matchedRole = roles.find((r) => r.name === source)}
										{@const scope =
											source === 'direct'
												? scopeLabelForGrant(entry.permission)
												: matchedRole
													? scopeLabelForRole(matchedRole.id)
													: null}
										<Badge variant="outline">
											{source}{#if scope}
												· {scope}
											{/if}
										</Badge>
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
				<Button type="submit" disabled={save.isPending}>
					{save.isPending ? 'Saving…' : 'Save'}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
