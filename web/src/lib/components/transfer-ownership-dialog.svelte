<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { createMutation, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import * as Dialog from '#lib/components/ui/dialog/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import { session } from '#lib/session.svelte.js';
	import type { components } from '#lib/api/schema.js';

	type User = components['schemas']['User'];
	type Role = components['schemas']['Role'];

	let {
		open = $bindable(false),
		users,
		roles
	}: { open: boolean; users: User[]; roles: Role[] } = $props();

	let targetUserId = $state('');
	let keepRoleId = $state('');
	let password = $state('');
	let failure = $state('');

	const candidates = $derived(
		users.filter((user) => !user.disabled && user.id !== session.me?.user.id)
	);
	const keepable = $derived(roles.filter((role) => !role.isOwner));
	const queryClient = useQueryClient();

	$effect(() => {
		if (!open) return;
		targetUserId = '';
		keepRoleId = '';
		password = '';
		failure = '';
	});

	const transfer = createMutation(() => ({
		mutationFn: async () => {
			const { error } = await api.POST('/api/v1/owner/transfer', {
				body: {
					targetUserId,
					password,
					...(keepRoleId ? { keepRoleId: Number(keepRoleId) } : {})
				}
			});
			if (error) throw new Error(error.detail ?? error.title);
		},
		onSuccess: async () => {
			open = false;
			await queryClient.invalidateQueries({ queryKey: ['users'] });
			// The session reloads `/me`: the caller is no longer an Owner.
			await invalidateAll();
		},
		onError: (error) => (failure = error.message)
	}));
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>Transfer ownership</Dialog.Title>
			<Dialog.Description>
				The chosen user becomes the Owner and you lose the Owner role. Confirm with your password.
			</Dialog.Description>
		</Dialog.Header>
		<form
			class="grid gap-4"
			onsubmit={(event) => {
				event.preventDefault();
				transfer.mutate();
			}}
		>
			<div class="grid gap-2">
				<Label for="transfer-target">New Owner</Label>
				<select
					id="transfer-target"
					required
					class="h-9 rounded-md border border-input bg-background px-3 text-sm"
					bind:value={targetUserId}
				>
					<option value="" disabled>Choose a user</option>
					{#each candidates as user (user.id)}
						<option value={user.id}>{user.name} ({user.email})</option>
					{/each}
				</select>
			</div>
			<div class="grid gap-2">
				<Label for="transfer-keep">Role you keep</Label>
				<select
					id="transfer-keep"
					class="h-9 rounded-md border border-input bg-background px-3 text-sm"
					bind:value={keepRoleId}
				>
					<option value="">No role</option>
					{#each keepable as role (role.id)}
						<option value={String(role.id)}>{role.name}</option>
					{/each}
				</select>
			</div>
			<div class="grid gap-2">
				<Label for="transfer-password">Your password</Label>
				<Input
					id="transfer-password"
					type="password"
					required
					autocomplete="current-password"
					bind:value={password}
				/>
			</div>
			{#if failure}
				<p role="alert" class="text-sm text-destructive">{failure}</p>
			{/if}
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (open = false)}>Cancel</Button>
				<Button type="submit" disabled={transfer.isPending}>Transfer ownership</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
