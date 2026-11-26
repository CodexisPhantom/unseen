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
		formatScorePayload,
		itemId,
		itemName,
		resolveCutoff,
		setFormatScore,
		type Draft,
		type ProfileFormatScore,
		type ProfileItem,
		type QualityProfile
	} from '#lib/profiles.js';

	let {
		open = $bindable(false),
		profile,
		availableFormats = []
	}: {
		open: boolean;
		profile: QualityProfile | null;
		availableFormats?: readonly ProfileFormatScore[];
	} = $props();

	let draft = $state<Draft>(draftOf(null));
	let formatFilter = $state('');
	let failure = $state('');

	const queryClient = useQueryClient();

	// Start from the profile being edited, or from a new one.
	$effect(() => {
		if (!open) return;
		draft = draftOf(profile, availableFormats);
		formatFilter = '';
		failure = '';
	});

	const choices = $derived(cutoffChoices(draft.items));
	const problem = $derived(draftProblem(draft));
	const cutoffLabel = $derived.by(() => {
		const chosen = choices.find((item) => itemId(item) === draft.cutoff);
		return chosen ? itemName(chosen) : 'Choose a cutoff';
	});
	const scoredCount = $derived(draft.formatScores.filter((entry) => entry.score !== 0).length);
	const filteredFormats = $derived.by(() => {
		const query = formatFilter.trim().toLowerCase();
		if (!query) return draft.formatScores;
		return draft.formatScores.filter((entry) => entry.name.toLowerCase().includes(query));
	});

	function setItems(items: ProfileItem[]) {
		draft.items = items;
		draft.cutoff = resolveCutoff(items, draft.cutoff);
	}

	function parseIntegerInput(value: string): number {
		if (value.trim() === '') return 0;
		return Number(value);
	}

	const save = createMutation(() => ({
		mutationFn: async () => {
			const {
				cutoff,
				name,
				upgradeAllowed,
				items,
				minFormatScore,
				cutoffFormatScore,
				minUpgradeFormatScore,
				formatScores
			} = draft;
			if (cutoff === null) throw new Error(problem);
			const body = {
				name: name.trim(),
				upgradeAllowed,
				cutoff,
				items,
				minFormatScore,
				cutoffFormatScore,
				minUpgradeFormatScore,
				formatScores: formatScorePayload(formatScores)
			};
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
			<fieldset class="grid gap-3 rounded-md border p-3">
				<div class="flex items-center justify-between gap-2">
					<legend class="text-sm font-medium">Custom format scores</legend>
					{#if scoredCount > 0}
						<Button
							type="button"
							variant="ghost"
							size="sm"
							onclick={() => {
								draft.formatScores = draft.formatScores.map((entry) => ({ ...entry, score: 0 }));
							}}
						>
							Reset scores ({scoredCount})
						</Button>
					{/if}
				</div>
				<div class="grid gap-3 sm:grid-cols-3">
					<div class="grid gap-1.5">
						<Label for="profile-min-format-score">Minimum score</Label>
						<Input
							id="profile-min-format-score"
							type="number"
							step="1"
							value={draft.minFormatScore}
							oninput={(event) => {
								draft.minFormatScore = parseIntegerInput(event.currentTarget.value);
							}}
						/>
						<p class="text-xs text-muted-foreground">Reject releases below this.</p>
					</div>
					<div class="grid gap-1.5">
						<Label for="profile-cutoff-format-score">Upgrade until score</Label>
						<Input
							id="profile-cutoff-format-score"
							type="number"
							step="1"
							value={draft.cutoffFormatScore}
							oninput={(event) => {
								draft.cutoffFormatScore = parseIntegerInput(event.currentTarget.value);
							}}
						/>
						<p class="text-xs text-muted-foreground">Stop score upgrades here.</p>
					</div>
					<div class="grid gap-1.5">
						<Label for="profile-min-upgrade-format-score">Minimum gain</Label>
						<Input
							id="profile-min-upgrade-format-score"
							type="number"
							step="1"
							min="1"
							value={draft.minUpgradeFormatScore}
							oninput={(event) => {
								draft.minUpgradeFormatScore = parseIntegerInput(event.currentTarget.value);
							}}
						/>
						<p class="text-xs text-muted-foreground">Smallest gain to upgrade.</p>
					</div>
				</div>
				{#if draft.formatScores.length === 0}
					<p class="text-xs text-muted-foreground">
						No custom formats yet. Add custom formats to assign scores to this profile.
					</p>
				{:else}
					{#if draft.formatScores.length > 5}
						<Input
							type="search"
							placeholder="Filter custom formats…"
							aria-label="Filter custom formats"
							bind:value={formatFilter}
						/>
					{/if}
					{#if filteredFormats.length === 0}
						<p class="text-xs text-muted-foreground">No custom formats match "{formatFilter}".</p>
					{:else}
						<div class="max-h-56 divide-y overflow-y-auto rounded-md border">
							{#each filteredFormats as entry (entry.customFormatId)}
								<div class="flex items-center justify-between gap-3 px-3 py-2">
									<Label
										for={`cf-score-${entry.customFormatId}`}
										class="min-w-0 flex-1 truncate font-normal"
									>
										{entry.name}
									</Label>
									<Input
										id={`cf-score-${entry.customFormatId}`}
										type="number"
										step="1"
										class="w-24 text-end tabular-nums"
										value={entry.score}
										oninput={(event) => {
											draft.formatScores = setFormatScore(
												draft.formatScores,
												entry.customFormatId,
												parseIntegerInput(event.currentTarget.value)
											);
										}}
									/>
								</div>
							{/each}
						</div>
					{/if}
				{/if}
			</fieldset>
			{#if failure || problem}
				<p role="alert" class="text-sm text-destructive">{failure || problem}</p>
			{/if}
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (open = false)}>Cancel</Button>
				<Button type="submit" disabled={save.isPending || problem !== ''}>
					{save.isPending ? 'Saving…' : 'Save'}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
