<script lang="ts">
	import { createMutation, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import * as Table from '#lib/components/ui/table/index.js';
	import {
		changes,
		draftOf,
		problem as draftProblem,
		type Draft,
		type QualityDefinition
	} from '#lib/quality-definitions.js';

	let { definition, manage }: { definition: QualityDefinition; manage: boolean } = $props();

	// Starts from the stored values, and again after each save.
	let draft = $derived<Draft>(draftOf(definition));
	let failure = $state('');

	const body = $derived(changes(draft, definition));
	const dirty = $derived(Object.keys(body).length > 0);
	const problem = $derived(dirty ? draftProblem(draft) : '');

	const queryClient = useQueryClient();
	const save = createMutation(() => ({
		mutationFn: async () => {
			const { error } = await api.PATCH('/api/v1/quality-definitions/{qualityId}', {
				params: { path: { qualityId: definition.qualityId } },
				body
			});
			if (error) throw new Error(error.detail ?? error.title);
		},
		onSuccess: () => (failure = ''),
		onError: (error) => (failure = error.message),
		onSettled: () => queryClient.invalidateQueries({ queryKey: ['quality-definitions'] })
	}));

	const label = (what: string) => `${what} of ${definition.name}`;
</script>

<Table.Row>
	<Table.Cell class="font-medium">
		{definition.name}
		{#if failure || problem}
			<p role="alert" class="text-xs font-normal text-destructive">{failure || problem}</p>
		{/if}
	</Table.Cell>
	<Table.Cell>
		<Input
			type="number"
			min="0"
			step="any"
			class="w-24"
			aria-label={label('Minimum')}
			disabled={!manage}
			bind:value={draft.minSize}
		/>
	</Table.Cell>
	<Table.Cell>
		<Input
			type="number"
			min="0"
			step="any"
			class="w-24"
			aria-label={label('Preferred size')}
			placeholder="None"
			disabled={!manage}
			bind:value={draft.preferredSize}
		/>
	</Table.Cell>
	<Table.Cell>
		<Input
			type="number"
			min="0"
			step="any"
			class="w-24"
			aria-label={label('Maximum')}
			placeholder="No limit"
			disabled={!manage}
			bind:value={draft.maxSize}
		/>
	</Table.Cell>
	{#if manage}
		<Table.Cell class="text-end">
			<Button
				size="sm"
				disabled={!dirty || problem !== '' || save.isPending}
				onclick={() => save.mutate()}
			>
				Save
			</Button>
		</Table.Cell>
	{/if}
</Table.Row>
