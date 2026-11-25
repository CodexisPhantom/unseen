<script lang="ts">
	import { createMutation, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import { loadChoices, lookupYear, pickId, saveChoices, type MovieLookup } from '#lib/add.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { Checkbox } from '#lib/components/ui/checkbox/index.js';
	import * as Dialog from '#lib/components/ui/dialog/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import * as Select from '#lib/components/ui/select/index.js';
	import { Switch } from '#lib/components/ui/switch/index.js';
	import { availabilityNames, type Movie } from '#lib/library.js';

	type Tag = { id: number; label: string };
	type RootFolder = { id: number; path: string; accessible: boolean };
	type Profile = { id: number; name: string };

	let {
		open = $bindable(false),
		result,
		rootFolders,
		profiles,
		tags,
		onadded
	}: {
		open: boolean;
		result: MovieLookup;
		rootFolders: readonly RootFolder[];
		profiles: readonly Profile[];
		tags: readonly Tag[];
		onadded: (movie: Movie) => void;
	} = $props();

	let rootFolderId = $state<number | null>(null);
	let profileId = $state<number | null>(null);
	let availability = $state('released');
	let monitored = $state(true);
	let tagIds = $state<number[]>([]);
	let failure = $state('');

	// Start from the remembered folder and profile each time the dialog opens.
	$effect(() => {
		if (!open) return;
		const remembered = loadChoices();
		rootFolderId = pickId(
			remembered.rootFolderId,
			rootFolders.map((folder) => folder.id)
		);
		profileId = pickId(
			remembered.qualityProfileId,
			profiles.map((profile) => profile.id)
		);
		availability = 'released';
		monitored = true;
		tagIds = [];
		failure = '';
	});

	const queryClient = useQueryClient();
	const add = createMutation(() => ({
		mutationFn: async () => {
			if (rootFolderId === null || profileId === null) {
				throw new Error('Choose a root folder and a quality profile.');
			}
			const { data, error } = await api.POST('/api/v1/movies', {
				body: {
					tmdbId: result.tmdbId,
					rootFolderId,
					qualityProfileIds: [profileId],
					minimumAvailability: availability,
					monitored,
					tags: tagIds
				}
			});
			if (!data) throw new Error(error?.detail ?? error?.title ?? 'The movie could not be added.');
			return data;
		},
		onSuccess: async (movie) => {
			saveChoices({ rootFolderId, qualityProfileId: profileId });
			await queryClient.invalidateQueries({ queryKey: ['movies'] });
			open = false;
			onadded(movie);
		},
		onError: (error) => (failure = error.message)
	}));

	function toggleTag(id: number, on: boolean) {
		tagIds = on ? [...tagIds, id] : tagIds.filter((tag) => tag !== id);
	}

	const year = $derived(lookupYear(result));
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>Add {result.title}{year ? ` (${year})` : ''}</Dialog.Title>
			<Dialog.Description>Choose where the movie lives and how Unseen treats it.</Dialog.Description>
		</Dialog.Header>
		<form
			class="grid gap-4"
			onsubmit={(event) => {
				event.preventDefault();
				add.mutate();
			}}
		>
			<div class="grid gap-2">
				<Label for="add-root-folder">Root folder</Label>
				<Select.Root
					type="single"
					value={rootFolderId === null ? '' : String(rootFolderId)}
					onValueChange={(value) => (rootFolderId = Number(value))}
				>
					<Select.Trigger id="add-root-folder" class="w-full">
						{rootFolders.find((folder) => folder.id === rootFolderId)?.path ?? 'Choose a folder'}
					</Select.Trigger>
					<Select.Content>
						{#each rootFolders as folder (folder.id)}
							<Select.Item
								value={String(folder.id)}
								label={folder.accessible ? folder.path : `${folder.path} (not accessible)`}
							/>
						{/each}
					</Select.Content>
				</Select.Root>
			</div>
			<div class="grid gap-2">
				<Label for="add-profile">Quality profile</Label>
				<Select.Root
					type="single"
					value={profileId === null ? '' : String(profileId)}
					onValueChange={(value) => (profileId = Number(value))}
				>
					<Select.Trigger id="add-profile" class="w-full">
						{profiles.find((profile) => profile.id === profileId)?.name ?? 'Choose a profile'}
					</Select.Trigger>
					<Select.Content>
						{#each profiles as profile (profile.id)}
							<Select.Item value={String(profile.id)} label={profile.name} />
						{/each}
					</Select.Content>
				</Select.Root>
			</div>
			<div class="grid gap-2">
				<Label for="add-availability">Minimum availability</Label>
				<Select.Root type="single" bind:value={availability}>
					<Select.Trigger id="add-availability" class="w-full">
						{availabilityNames[availability] ?? availability}
					</Select.Trigger>
					<Select.Content>
						{#each Object.entries(availabilityNames) as [value, label] (value)}
							<Select.Item {value} {label} />
						{/each}
					</Select.Content>
				</Select.Root>
			</div>
			<div class="flex items-center justify-between gap-4">
				<Label for="add-monitored">Monitored</Label>
				<Switch id="add-monitored" bind:checked={monitored} />
			</div>
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
				<Button
					type="submit"
					disabled={add.isPending || rootFolderId === null || profileId === null}
				>
					Add movie
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
