<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { createMutation, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import * as AlertDialog from '#lib/components/ui/alert-dialog/index.js';
	import { Checkbox } from '#lib/components/ui/checkbox/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import type { Movie } from '#lib/library.js';

	let { open = $bindable(false), movie }: { open: boolean; movie: Movie } = $props();

	let deleteFiles = $state(false);
	let failure = $state('');

	$effect(() => {
		if (!open) return;
		deleteFiles = false;
		failure = '';
	});

	const queryClient = useQueryClient();
	const remove = createMutation(() => ({
		mutationFn: async () => {
			const { error } = await api.DELETE('/api/v1/movies/{id}', {
				params: { path: { id: movie.id }, query: { deleteFiles } }
			});
			if (error) throw new Error(error.detail ?? error.title);
		},
		onSuccess: async () => {
			open = false;
			queryClient.removeQueries({ queryKey: ['movie', movie.id] });
			await queryClient.invalidateQueries({ queryKey: ['movies'] });
			await goto(resolve('library'));
		},
		onError: (error) => (failure = error.message)
	}));
</script>

<AlertDialog.Root bind:open>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Delete {movie.title}?</AlertDialog.Title>
			<AlertDialog.Description>
				Unseenarr forgets the movie. The folder <code>{movie.folderName}</code> stays on disk unless you
				tick the box below. This cannot be undone.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<Label class="gap-2 font-normal">
			<Checkbox bind:checked={deleteFiles} />
			Also delete the movie folder and its files from disk
		</Label>
		{#if failure}
			<p role="alert" class="text-sm text-destructive">{failure}</p>
		{/if}
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
			<AlertDialog.Action
				disabled={remove.isPending}
				onclick={(event) => {
					// Stay open until the request answers, so a failure is visible.
					event.preventDefault();
					remove.mutate();
				}}
			>
				Delete
			</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
