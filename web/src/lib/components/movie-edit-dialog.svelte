<script lang="ts">
	import { createMutation, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { Checkbox } from '#lib/components/ui/checkbox/index.js';
	import * as Dialog from '#lib/components/ui/dialog/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import * as Select from '#lib/components/ui/select/index.js';
	import { Switch } from '#lib/components/ui/switch/index.js';
	import { availabilityNames, type Movie } from '#lib/library.js';

	type Tag = { id: number; label: string };
	type Choice = { id: number; name: string };

	let {
		open = $bindable(false),
		movie,
		tags,
		profileNames
	}: {
		open: boolean;
		movie: Movie;
		tags: readonly Tag[];
		/** The quality profiles of this movie, by movie profile id. */
		profileNames: readonly Choice[];
	} = $props();

	let monitored = $state(true);
	let availability = $state('released');
	let tagIds = $state<number[]>([]);
	let primaryId = $state(0);
	let failure = $state('');

	// Start from the movie each time the dialog opens.
	$effect(() => {
		if (!open) return;
		monitored = movie.monitored;
		availability = movie.minimumAvailability;
		tagIds = [...movie.tags];
		primaryId = movie.profiles.find((profile) => profile.isPrimary)?.id ?? 0;
		failure = '';
	});

	const queryClient = useQueryClient();
	const save = createMutation(() => ({
		mutationFn: async () => {
			const currentPrimary = movie.profiles.find((profile) => profile.isPrimary)?.id;
			const { error } = await api.PATCH('/api/v1/movies/{id}', {
				params: { path: { id: movie.id } },
				body: {
					monitored,
					minimumAvailability: availability,
					tags: tagIds,
					primaryProfileId: primaryId !== currentPrimary ? primaryId : null
				}
			});
			if (error) throw new Error(error.detail ?? error.title);
		},
		onSuccess: async () => {
			await Promise.all([
				queryClient.invalidateQueries({ queryKey: ['movies'] }),
				queryClient.invalidateQueries({ queryKey: ['movie', movie.id] })
			]);
			open = false;
		},
		onError: (error) => (failure = error.message)
	}));

	function toggleTag(id: number, on: boolean) {
		tagIds = on ? [...tagIds, id] : tagIds.filter((tag) => tag !== id);
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>Edit {movie.title}</Dialog.Title>
			<Dialog.Description>
				Folder and file names do not change. Renaming is a separate action.
			</Dialog.Description>
		</Dialog.Header>
		<form
			class="grid gap-4"
			onsubmit={(event) => {
				event.preventDefault();
				save.mutate();
			}}
		>
			<div class="flex items-center justify-between gap-4">
				<Label for="movie-monitored">Monitored</Label>
				<Switch id="movie-monitored" bind:checked={monitored} />
			</div>
			<div class="grid gap-2">
				<Label for="movie-availability">Minimum availability</Label>
				<Select.Root type="single" bind:value={availability}>
					<Select.Trigger id="movie-availability" class="w-full">
						{availabilityNames[availability] ?? availability}
					</Select.Trigger>
					<Select.Content>
						{#each Object.entries(availabilityNames) as [value, label] (value)}
							<Select.Item {value} {label} />
						{/each}
					</Select.Content>
				</Select.Root>
			</div>
			{#if profileNames.length > 1}
				<div class="grid gap-2">
					<Label for="movie-primary">Primary profile</Label>
					<Select.Root
						type="single"
						value={String(primaryId)}
						onValueChange={(value) => (primaryId = Number(value))}
					>
						<Select.Trigger id="movie-primary" class="w-full">
							{profileNames.find((choice) => choice.id === primaryId)?.name ?? ''}
						</Select.Trigger>
						<Select.Content>
							{#each profileNames as choice (choice.id)}
								<Select.Item value={String(choice.id)} label={choice.name} />
							{/each}
						</Select.Content>
					</Select.Root>
				</div>
			{/if}
			{#if tags.length > 0}
				<fieldset class="grid gap-2">
					<legend class="mb-1 text-sm font-medium">Tags</legend>
					{#each tags as tag (tag.id)}
						<Label class="gap-2 font-normal">
							<Checkbox
								checked={tagIds.includes(tag.id)}
								onCheckedChange={(on) => toggleTag(tag.id, on === true)}
							/>
							{tag.label}
						</Label>
					{/each}
				</fieldset>
			{/if}
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
