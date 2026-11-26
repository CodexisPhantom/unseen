<script lang="ts">
	import { createMutation, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import * as Dialog from '#lib/components/ui/dialog/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import * as Select from '#lib/components/ui/select/index.js';
	import { Switch } from '#lib/components/ui/switch/index.js';
	import {
		createRequestBody,
		draftOf,
		draftProblem,
		INDEXER_KINDS,
		indexerKindLabel,
		indexerUrlPlaceholder,
		isIndexerKind,
		requestBody,
		type Draft,
		type Indexer
	} from '#lib/indexers.js';

	let {
		open = $bindable(false),
		indexer
	}: { open: boolean; indexer: Indexer | null } = $props();

	let draft = $state<Draft>(draftOf(null));
	let failure = $state('');

	const queryClient = useQueryClient();

	// Start from the indexer being edited, or from a new one.
	$effect(() => {
		if (!open) return;
		draft = draftOf(indexer);
		failure = '';
	});

	const problem = $derived(draftProblem(draft, indexer !== null));

	const save = createMutation(() => ({
		mutationFn: async () => {
			const { error } = indexer
				? await api.PATCH('/api/v1/indexers/{id}', {
						params: { path: { id: indexer.id } },
						body: requestBody(draft)
					})
				: await api.POST('/api/v1/indexers', {
						// `draftProblem` guarantees a key for a new indexer.
						body: createRequestBody(draft)
					});
			if (error) throw new Error(error.detail ?? error.title);
		},
		onSuccess: async () => {
			await queryClient.invalidateQueries({ queryKey: ['indexers'] });
			open = false;
		},
		onError: (error) => (failure = error.message)
	}));
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-xl">
		<Dialog.Header>
			<Dialog.Title>{indexer ? `Edit ${indexer.name}` : 'New indexer'}</Dialog.Title>
			<Dialog.Description>
				A Torznab (torrent) or Newznab (Usenet) indexer. Save it, then use Test to check the
				address and the key.
			</Dialog.Description>
		</Dialog.Header>
		<form
			class="grid gap-4"
			onsubmit={(event) => {
				event.preventDefault();
				if (!problem) save.mutate();
			}}
		>
			<div class="grid gap-4 sm:grid-cols-2">
				<div class="grid gap-2">
					<Label for="indexer-name">Name</Label>
					<Input id="indexer-name" required maxlength={64} bind:value={draft.name} />
				</div>
				<div class="grid gap-2">
					<Label for="indexer-kind">Kind</Label>
					{#if indexer}
						<Input id="indexer-kind" value={indexerKindLabel(indexer.kind)} disabled />
					{:else}
						<Select.Root
							type="single"
							value={draft.kind}
							onValueChange={(value) => {
								if (isIndexerKind(value)) draft.kind = value;
							}}
						>
							<Select.Trigger id="indexer-kind" class="w-full">
								{indexerKindLabel(draft.kind)}
							</Select.Trigger>
							<Select.Content>
								{#each INDEXER_KINDS as option (option.value)}
									<Select.Item value={option.value} label={option.label} />
								{/each}
							</Select.Content>
						</Select.Root>
					{/if}
				</div>
			</div>
			<div class="grid gap-2">
				<Label for="indexer-url">{indexerKindLabel(draft.kind)} URL</Label>
				<Input
					id="indexer-url"
					required
					type="url"
					placeholder={indexerUrlPlaceholder(draft.kind)}
					bind:value={draft.url}
				/>
			</div>
			<div class="grid gap-2">
				<Label for="indexer-api-key">API key</Label>
				<Input
					id="indexer-api-key"
					type="password"
					autocomplete="off"
					required={!indexer}
					placeholder={indexer?.hasApiKey ? 'Unchanged' : ''}
					class="font-mono"
					bind:value={draft.apiKey}
				/>
				{#if indexer?.hasApiKey}
					<p class="text-xs text-muted-foreground">
						The stored key is unchanged. Type a new key to replace it.
					</p>
				{/if}
			</div>
			{#if indexer?.apiKeyMustBeReentered}
				<p class="text-sm text-destructive">The stored API key cannot be read. Enter it again.</p>
			{/if}
			<div class="grid gap-4 sm:grid-cols-2">
				<div class="grid gap-2">
					<Label for="indexer-categories">Categories</Label>
					<Input
						id="indexer-categories"
						placeholder="2000, 2040"
						class="font-mono"
						bind:value={draft.categories}
					/>
					<p class="text-xs text-muted-foreground">
						{indexerKindLabel(draft.kind)} ids. 2000 is movies.
					</p>
				</div>
				<div class="grid gap-2">
					<Label for="indexer-priority">Priority</Label>
					<Input
						id="indexer-priority"
						type="number"
						min={1}
						max={50}
						required
						bind:value={draft.priority}
					/>
					<p class="text-xs text-muted-foreground">1 to 50. The lower number wins a tie.</p>
				</div>
			</div>
			<div class="flex items-center justify-between gap-4">
				<Label for="indexer-enabled">Enabled</Label>
				<Switch id="indexer-enabled" bind:checked={draft.enabled} />
			</div>
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
