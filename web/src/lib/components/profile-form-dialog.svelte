<script lang="ts">
	import { createMutation, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import ProfileItems from '#lib/components/profile-items.svelte';
	import { Button } from '#lib/components/ui/button/index.js';
	import * as Dialog from '#lib/components/ui/dialog/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import * as Select from '#lib/components/ui/select/index.js';
	import { Switch } from '#lib/components/ui/switch/index.js';
	import {
		cutoffChoices,
		draftOf,
		draftProblem,
		itemId,
		itemName,
		resolveCutoff,
		type Draft,
		type ProfileItem,
		type QualityProfile
	} from '#lib/profiles.js';

	let {
		open = $bindable(false),
		profile
	}: { open: boolean; profile: QualityProfile | null } = $props();

	let draft = $state<Draft>(draftOf(null));
	let failure = $state('');

	const queryClient = useQueryClient();

	// Start from the profile being edited, or from a new one.
	$effect(() => {
		if (!open) return;
		draft = draftOf(profile);
		failure = '';
	});

	const choices = $derived(cutoffChoices(draft.items));
	const problem = $derived(draftProblem(draft));
	const cutoffLabel = $derived.by(() => {
		const chosen = choices.find((item) => itemId(item) === draft.cutoff);
		return chosen ? itemName(chosen) : 'Choose a cutoff';
	});

	function setItems(items: ProfileItem[]) {
		draft.items = items;
		draft.cutoff = resolveCutoff(items, draft.cutoff);
	}

	const save = createMutation(() => ({
		mutationFn: async () => {
			const { cutoff, name, upgradeAllowed, items } = draft;
			if (cutoff === null) throw new Error(problem);
			const body = { name: name.trim(), upgradeAllowed, cutoff, items };
			const { error } = profile
				? await api.PATCH('/api/v1/quality-profiles/{id}', {
						params: { path: { id: profile.id } },
						body
					})
				: await api.POST('/api/v1/quality-profiles', { body });
			if (error) throw new Error(error.detail ?? error.title);
		},
		onSuccess: async () => {
			await queryClient.invalidateQueries({ queryKey: ['quality-profiles'] });
			open = false;
		},
		onError: (error) => (failure = error.message)
	}));
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-xl">
		<Dialog.Header>
			<Dialog.Title>{profile ? `Edit ${profile.name}` : 'New quality profile'}</Dialog.Title>
			<Dialog.Description>
				Order the qualities from lowest to highest and tick the ones this profile accepts.
				Upgrades stop at the cutoff.
			</Dialog.Description>
		</Dialog.Header>
		<form
			class="grid gap-4"
			onsubmit={(event) => {
				event.preventDefault();
				if (!problem) save.mutate();
			}}
		>
			<div class="grid gap-2">
				<Label for="profile-name">Name</Label>
				<Input id="profile-name" required maxlength={64} bind:value={draft.name} />
			</div>
			<div class="flex items-center justify-between gap-4">
				<Label for="profile-upgrade">Upgrade files until the cutoff</Label>
				<Switch id="profile-upgrade" bind:checked={draft.upgradeAllowed} />
			</div>
			<div class="grid gap-2">
				<Label for="profile-cutoff">Cutoff</Label>
				<Select.Root
					type="single"
					value={draft.cutoff === null ? '' : String(draft.cutoff)}
					onValueChange={(value) => (draft.cutoff = Number(value))}
					disabled={choices.length === 0}
				>
					<Select.Trigger id="profile-cutoff" class="w-full">{cutoffLabel}</Select.Trigger>
					<Select.Content>
						{#each choices as item (itemId(item))}
							<Select.Item value={String(itemId(item))} label={itemName(item)} />
						{/each}
					</Select.Content>
				</Select.Root>
			</div>
			<fieldset class="grid gap-2">
				<legend class="mb-1 text-sm font-medium">Qualities, lowest first</legend>
				<p class="text-xs text-muted-foreground">
					Drag a handle, or focus it and press the up and down arrow keys, to reorder.
				</p>
				<ProfileItems items={draft.items} cutoff={draft.cutoff} onchange={setItems} />
			</fieldset>
			{#if failure || problem}
				<p role="alert" class="text-sm text-destructive">{failure || problem}</p>
			{/if}
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (open = false)}>Cancel</Button>
				<Button type="submit" disabled={save.isPending || problem !== ''}>Save</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
