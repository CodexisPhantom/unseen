<script lang="ts">
	import { createMutation, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { Checkbox } from '#lib/components/ui/checkbox/index.js';
	import * as Dialog from '#lib/components/ui/dialog/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import { Textarea } from '#lib/components/ui/textarea/index.js';
	import { groupPermissions, roleTemplates } from '#lib/roles.js';
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

	const groups = $derived(groupPermissions(catalog));
	const queryClient = useQueryClient();

	// Start from the role being edited, or from an empty form.
	$effect(() => {
		if (!open) return;
		name = role?.name ?? '';
		description = role?.description ?? '';
		selected = role?.permissions ?? [];
		failure = '';
	});

	function toggle(permission: Permission, on: boolean) {
		selected = on
			? [...selected, permission]
			: selected.filter((entry) => entry !== permission);
	}

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
									selected = template.permissions(catalog);
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
				{#each groups as group (group.resource)}
					<div class="grid gap-2">
						<span class="text-xs font-medium text-muted-foreground uppercase">
							{group.resource}
						</span>
						<div class="grid gap-2 sm:grid-cols-2">
							{#each group.permissions as permission (permission)}
								{@const held = session.can(permission)}
								<Label class="gap-2 font-normal {held ? '' : 'opacity-50'}">
									<Checkbox
										disabled={!held}
										checked={selected.includes(permission)}
										onCheckedChange={(on) => toggle(permission, on === true)}
									/>
									<span class="font-mono text-xs">{permission}</span>
								</Label>
							{/each}
						</div>
					</div>
				{/each}
				<p class="text-xs text-muted-foreground">
					Greyed permissions are ones you do not hold, so you cannot give them away.
				</p>
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
