<script lang="ts">
	import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import { createQuery } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import { Badge } from '#lib/components/ui/badge/index.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import * as Dialog from '#lib/components/ui/dialog/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import * as Select from '#lib/components/ui/select/index.js';
	import * as Table from '#lib/components/ui/table/index.js';
	import * as Tooltip from '#lib/components/ui/tooltip/index.js';
	import type { Movie } from '#lib/library.js';
	import {
		filterByQuality,
		formatAge,
		formatScore,
		qualityChoices,
		rejectionText,
		sortReleases,
		type SortDirection,
		type SortKey
	} from '#lib/releases.js';
	import { formatBytes } from '#lib/root-folders.js';

	type Choice = { id: number; name: string };

	let {
		open = $bindable(false),
		movie,
		profileNames
	}: {
		open: boolean;
		movie: Movie;
		/** The quality profiles of this movie, by movie profile id. */
		profileNames: readonly Choice[];
	} = $props();

	const columns: { key: SortKey; label: string; end?: boolean }[] = [
		{ key: 'title', label: 'Title' },
		{ key: 'indexer', label: 'Indexer' },
		{ key: 'quality', label: 'Quality' },
		{ key: 'size', label: 'Size', end: true },
		{ key: 'seeders', label: 'Seeders', end: true },
		{ key: 'age', label: 'Age', end: true },
		{ key: 'score', label: 'Score', end: true }
	];

	let profileId = $state(0);
	let qualityId = $state<number | null>(null);
	let sortKey = $state<SortKey | null>(null);
	let direction = $state<SortDirection>('asc');

	// Start from the primary profile and the server's ranking each time the dialog opens.
	$effect(() => {
		if (!open) return;
		profileId = movie.profiles.find((profile) => profile.isPrimary)?.id ?? movie.profiles[0]?.id ?? 0;
		qualityId = null;
		sortKey = null;
		direction = 'asc';
	});

	const search = createQuery(() => ({
		queryKey: ['movie', movie.id, 'releases', profileId],
		enabled: open && profileId !== 0,
		retry: false,
		gcTime: 0,
		refetchOnWindowFocus: false,
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/movies/{id}/releases', {
				params: { path: { id: movie.id }, query: { profileId } }
			});
			if (!data) throw new Error(error?.detail ?? error?.title ?? 'The search failed.');
			return data;
		}
	}));

	const qualities = $derived(qualityChoices(search.data?.releases ?? []));
	const rows = $derived(
		sortReleases(filterByQuality(search.data?.releases ?? [], qualityId), sortKey, direction)
	);

	function sortBy(key: SortKey) {
		if (sortKey !== key) {
			sortKey = key;
			direction = key === 'title' || key === 'indexer' ? 'asc' : 'desc';
		} else if (direction === 'desc' && key !== 'title' && key !== 'indexer') {
			direction = 'asc';
		} else if (direction === 'asc' && (key === 'title' || key === 'indexer')) {
			direction = 'desc';
		} else {
			sortKey = null;
		}
	}

	function ariaSort(key: SortKey) {
		if (sortKey !== key) return 'none';
		return direction === 'asc' ? 'ascending' : 'descending';
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-6xl">
		<Dialog.Header>
			<Dialog.Title>Search releases for {movie.title}</Dialog.Title>
			<Dialog.Description>
				Every indexer answers. A rejected release shows why. Nothing is grabbed from here yet.
			</Dialog.Description>
		</Dialog.Header>

		<div class="flex flex-wrap items-end gap-4">
			{#if profileNames.length > 1}
				<div class="grid gap-2">
					<Label for="search-profile">Profile</Label>
					<Select.Root
						type="single"
						value={String(profileId)}
						onValueChange={(value) => {
							profileId = Number(value);
							qualityId = null;
						}}
					>
						<Select.Trigger id="search-profile" class="w-56">
							{profileNames.find((choice) => choice.id === profileId)?.name ?? ''}
						</Select.Trigger>
						<Select.Content>
							{#each profileNames as choice (choice.id)}
								<Select.Item value={String(choice.id)} label={choice.name} />
							{/each}
						</Select.Content>
					</Select.Root>
				</div>
			{/if}
			<div class="grid gap-2">
				<Label for="search-quality">Quality</Label>
				<Select.Root
					type="single"
					value={qualityId === null ? 'all' : String(qualityId)}
					onValueChange={(value) => (qualityId = value === 'all' ? null : Number(value))}
				>
					<Select.Trigger id="search-quality" class="w-56" disabled={qualities.length === 0}>
						{qualities.find((quality) => quality.id === qualityId)?.name ?? 'All qualities'}
					</Select.Trigger>
					<Select.Content>
						<Select.Item value="all" label="All qualities" />
						{#each qualities as quality (quality.id)}
							<Select.Item value={String(quality.id)} label={quality.name} />
						{/each}
					</Select.Content>
				</Select.Root>
			</div>
			<Button variant="outline" onclick={() => search.refetch()} disabled={search.isFetching}>
				Search again
			</Button>
		</div>

		{#if search.isError}
			<p role="alert" class="text-sm text-destructive">{search.error.message}</p>
		{/if}
		{#if search.data && search.data.indexerErrors.length > 0}
			<ul role="alert" class="grid gap-1 rounded-md border border-destructive/40 p-3 text-sm text-destructive">
				{#each search.data.indexerErrors as problem (problem.indexerId)}
					<li><strong>{problem.indexerName}</strong>: {problem.message}</li>
				{/each}
			</ul>
		{/if}

		{#if search.isFetching}
			<p class="text-sm text-muted-foreground">Searching…</p>
		{:else if search.data}
			<Tooltip.Provider>
				<Table.Root>
					<Table.Header>
						<Table.Row>
							{#each columns as column (column.key)}
								<Table.Head class={column.end ? 'text-end' : ''} aria-sort={ariaSort(column.key)}>
									<Button
										variant="ghost"
										size="xs"
										class="-mx-2"
										onclick={() => sortBy(column.key)}
									>
										{column.label}
										{#if sortKey === column.key}
											{#if direction === 'asc'}<ArrowUpIcon />{:else}<ArrowDownIcon />{/if}
										{/if}
									</Button>
								</Table.Head>
							{/each}
							<Table.Head><span class="sr-only">Grab</span></Table.Head>
						</Table.Row>
					</Table.Header>
					<Table.Body>
						{#each rows as release (release.id)}
							<Table.Row class={release.accepted ? '' : 'opacity-60'}>
								<Table.Cell class="max-w-md whitespace-normal break-all">
									{release.title}
									<div class="mt-1 flex flex-wrap gap-1">
										{#if !release.accepted}
											<Tooltip.Root>
												<Tooltip.Trigger>
													{#snippet child({ props })}
														<Badge variant="destructive" {...props} tabindex={0}>Rejected</Badge>
													{/snippet}
												</Tooltip.Trigger>
												<Tooltip.Content class="whitespace-pre-line">
													{rejectionText(release)}
												</Tooltip.Content>
											</Tooltip.Root>
										{/if}
										{#each release.customFormats as format (format.name)}
											<Badge variant="secondary">{format.name} {formatScore(format.score)}</Badge>
										{/each}
									</div>
								</Table.Cell>
								<Table.Cell>{release.indexerName}</Table.Cell>
								<Table.Cell>{release.qualityName}</Table.Cell>
								<Table.Cell class="text-end">{formatBytes(release.size)}</Table.Cell>
								<Table.Cell class="text-end">{release.seeders ?? '—'}</Table.Cell>
								<Table.Cell class="text-end">{formatAge(release.ageHours)}</Table.Cell>
								<Table.Cell class="text-end">{formatScore(release.score.customFormatScore)}</Table.Cell>
								<Table.Cell class="text-end">
									<Button size="xs" disabled title="Grabbing arrives with downloads">Grab</Button>
								</Table.Cell>
							</Table.Row>
						{:else}
							<Table.Row>
								<Table.Cell colspan={columns.length + 1} class="text-center text-muted-foreground">
									{qualityId === null ? 'No release found.' : 'No release of this quality.'}
								</Table.Cell>
							</Table.Row>
						{/each}
					</Table.Body>
				</Table.Root>
			</Tooltip.Provider>
		{/if}
	</Dialog.Content>
</Dialog.Root>
