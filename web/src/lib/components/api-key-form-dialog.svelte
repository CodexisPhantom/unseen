<script lang="ts">
	import { createMutation, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import PermissionPicker from '#lib/components/permission-picker.svelte';
	import { Button } from '#lib/components/ui/button/index.js';
	import * as Dialog from '#lib/components/ui/dialog/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import type { Permission } from '#lib/session.svelte.js';

	let {
		open = $bindable(false),
		catalog
	}: { open: boolean; catalog: Permission[] } = $props();

	let name = $state('');
	let selected = $state<Permission[]>([]);
	let failure = $state('');
	/** The new key. It is in the create response only. */
	let created = $state('');
	let copied = $state(false);

	const queryClient = useQueryClient();

	$effect(() => {
		if (!open) return;
		name = '';
		selected = [];
		failure = '';
		created = '';
		copied = false;
	});

	const save = createMutation(() => ({
		mutationFn: async () => {
			const { data, error } = await api.POST('/api/v1/me/api-keys', {
				body: { name, permissions: selected }
			});
			if (!data) throw new Error(error?.detail ?? error?.title ?? 'The key was not created.');
			return data;
		},
		onSuccess: async (data) => {
			created = data.key;
			await queryClient.invalidateQueries({ queryKey: ['api-keys'] });
		},
		onError: (error) => (failure = error.message)
	}));

	async function copy() {
		try {
			await navigator.clipboard.writeText(created);
			copied = true;
		} catch {
			failure = 'The key could not be copied. Select it and copy it by hand.';
		}
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-xl">
		{#if created}
			<Dialog.Header>
				<Dialog.Title>Your new key</Dialog.Title>
				<Dialog.Description>
					Copy it now. It is not shown again; if you lose it, revoke it and create another.
				</Dialog.Description>
			</Dialog.Header>
			<div class="grid gap-3">
				<code class="rounded-md border bg-muted p-3 font-mono text-sm break-all select-all">
					{created}
				</code>
				{#if failure}
					<p role="alert" class="text-sm text-destructive">{failure}</p>
				{/if}
			</div>
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={copy}>
					{copied ? 'Copied' : 'Copy'}
				</Button>
				<Button type="button" onclick={() => (open = false)}>Done</Button>
			</Dialog.Footer>
		{:else}
			<Dialog.Header>
				<Dialog.Title>New API key</Dialog.Title>
				<Dialog.Description>
					A key can do only what you tick here, and never more than you can do yourself. If you
					lose a permission, the key loses it too.
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
					<Label for="key-name">Name</Label>
					<Input
						id="key-name"
						required
						maxlength={64}
						placeholder="Seerr"
						bind:value={name}
					/>
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
					<Button type="submit" disabled={save.isPending}>Create</Button>
				</Dialog.Footer>
			</form>
		{/if}
	</Dialog.Content>
</Dialog.Root>
