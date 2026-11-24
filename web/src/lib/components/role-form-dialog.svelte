<script lang="ts">
	import { createMutation, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import * as Dialog from '#lib/components/ui/dialog/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import { Textarea } from '#lib/components/ui/textarea/index.js';
	import PermissionPicker from '#lib/components/permission-picker.svelte';
	import { roleTemplates } from '#lib/roles.js';
	import { session, type Permission } from '#lib/session.svelte.js';
	import type { components } from '#lib/api/schema.js';

	type Role = components['schemas']['Role'];

	let {
		open = $bindable(false),
		role,
		catalog
	}: { open: boolean; role: Role | null; catalog: Permission[] } = $props();

	let name = $state('');
	let description = $state('');
	let selected = $state<Permission[]>([]);
	let failure = $state('');

	const queryClient = useQueryClient();

	// Start from the role being edited, or from an empty form.
	$effect(() => {
		if (!open) return;
		name = role?.name ?? '';
		description = role?.description ?? '';
		selected = role?.permissions ?? [];
		failure = '';
	});

	const save = createMutation(() => ({
		mutationFn: async () => {
			const body = { name, description, permissions: selected };
			const { error } = role
				? await api.PATCH('/api/v1/roles/{id}', { params: { path: { id: role.id } }, body })
				: await api.POST('/api/v1/roles', { body });
			if (error) throw new Error(error.detail ?? error.title);
		},
		onSuccess: async () => {
			await queryClient.invalidateQueries({ queryKey: ['roles'] });
			open = false;
		},
		onError: (error) => (failure = error.message)
	}));
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-xl">
		<Dialog.Header>
			<Dialog.Title>{role ? `Edit ${role.name}` : 'New role'}</Dialog.Title>
			<Dialog.Description>
				A role holds only permissions you hold yourself. Changes apply on the next request of
				each user with the role.
			</Dialog.Description>
		</Dialog.Header>
		<form
			class="grid gap-4"
			onsubmit={(event) => {
				event.preventDefault();
				save.mutate();
			}}
		>
			{#if !role}
				<div class="grid gap-2">
					<span class="text-sm font-medium">Start from a template</span>
					<div class="flex flex-wrap gap-2">
						{#each roleTemplates as template (template.name)}
							<Button
								type="button"
								variant="outline"
								size="sm"
								title={template.description}
								onclick={() => {
									name = template.name;
									// Permissions you lack cannot be unticked, and saving them is refused.
									selected = template.permissions(catalog).filter((p) => session.can(p));
								}}
							>
								{template.name}
							</Button>
						{/each}
					</div>
				</div>
			{/if}
			<div class="grid gap-2">
				<Label for="role-name">Name</Label>
				<Input id="role-name" required maxlength={64} bind:value={name} />
			</div>
			<div class="grid gap-2">
				<Label for="role-description">Description</Label>
				<Textarea id="role-description" rows={2} bind:value={description} />
			</div>
			<fieldset class="grid gap-3">
				<legend class="mb-1 text-sm font-medium">Permissions</legend>
				<PermissionPicker {catalog} bind:selected />
			</fieldset>
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
