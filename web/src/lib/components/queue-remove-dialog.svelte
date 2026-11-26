<script lang="ts">
	import { createMutation, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import * as AlertDialog from '#lib/components/ui/alert-dialog/index.js';
	import { Checkbox } from '#lib/components/ui/checkbox/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import type { QueueItem } from '#lib/queue.js';

	let {
		open = $bindable(false),
		item
	}: {
		open: boolean;
		item: QueueItem | null;
	} = $props();

	let removeFromClient = $derived.by(() => {
		void open;
		void item;
		return true;
	});
	let blocklist = $derived.by(() => {
		void open;
		void item;
		return false;
	});
	let failure = $derived.by(() => {
		void open;
		void item;
		return '';
	});

	const queryClient = useQueryClient();
	const remove = createMutation(() => ({
		mutationFn: async (target: QueueItem) => {
			const { error } = await api.DELETE('/api/v1/queue/{id}', {
				params: {
					path: { id: target.id },
					query: { removeFromClient, blocklist }
				}
			});
			if (error) throw new Error(error.detail ?? error.title);
		},
		onSuccess: async () => {
			open = false;
			await queryClient.invalidateQueries({ queryKey: ['queue'] });
		},
		onError: (error) => (failure = error.message)
	}));
</script>

<AlertDialog.Root bind:open>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Remove download?</AlertDialog.Title>
			<AlertDialog.Description>
				{#if item}
					Remove <code class="break-all">{item.releaseTitle}</code> from the activity queue.
				{/if}
			</AlertDialog.Description>
		</AlertDialog.Header>
		<div class="flex flex-col gap-3">
			<Label class="gap-2 font-normal">
				<Checkbox bind:checked={removeFromClient} />
				Remove from download client
			</Label>
			<Label class="gap-2 font-normal">
				<Checkbox bind:checked={blocklist} />
				Add release to blocklist
			</Label>
		</div>
		{#if failure}
			<p role="alert" class="text-sm text-destructive">{failure}</p>
		{/if}
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
			<AlertDialog.Action
				disabled={remove.isPending || !item}
				onclick={(event) => {
					event.preventDefault();
					if (item) remove.mutate(item);
				}}
			>
				Remove
			</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
