<script lang="ts">
	import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import {
		adoptable,
		duplicateTargets,
		remainingRows,
		reviewRows,
		selectedItems,
		statusNames,
		summarize,
		withMatch,
		type AdoptFailure,
		type MovieLookup,
		type ReviewRow,
		type Summary
	} from '#lib/adopt.js';
	import AdoptMatchDialog from '#lib/components/adopt-match-dialog.svelte';
	import NoAccess from '#lib/components/no-access.svelte';
	import { Badge } from '#lib/components/ui/badge/index.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { Checkbox } from '#lib/components/ui/checkbox/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import * as Select from '#lib/components/ui/select/index.js';
	import { Switch } from '#lib/components/ui/switch/index.js';
	import * as Table from '#lib/components/ui/table/index.js';
	import { formatBytes } from '#lib/root-folders.js';
	import { session } from '#lib/session.svelte.js';

	const allowed = $derived(session.can('import:run'));

	const rootFolders = createQuery(() => ({
		queryKey: ['root-folders'],
		enabled: allowed,
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/root-folders');
			if (!data) throw new Error(error?.detail ?? 'The root folders could not be loaded.');
			return data;
		}
	}));
	const profiles = createQuery(() => ({
		queryKey: ['quality-profiles'],
		enabled: allowed,
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/quality-profiles');
			if (!data) throw new Error(error?.detail ?? 'The profiles could not be loaded.');
			return data;
		}
	}));

	let rootFolderId = $state<number | null>(null);
	let profileId = $state<number | null>(null);
	let monitored = $state(true);
	let rows = $state<ReviewRow[]>([]);
	let scannedRoot = $state<number | null>(null);
	let failure = $state('');
	let summary = $state<Summary | null>(null);
	let failures = $state<AdoptFailure[]>([]);
	let fixing = $state<number | null>(null);
	let fixOpen = $state(false);

	// Default to the first root folder and the first profile once they load.
	$effect(() => {
		if (rootFolderId === null && rootFolders.data?.length) rootFolderId = rootFolders.data[0].id;
	});
	$effect(() => {
		if (profileId === null && profiles.data?.length) profileId = profiles.data[0].id;
	});

	const selected = $derived(selectedItems(rows));
	const repeated = $derived(duplicateTargets(rows));
	const selectable = $derived(rows.filter(adoptable));
	const allSelected = $derived(selectable.length > 0 && selectable.every((row) => row.include));
	const someSelected = $derived(selectable.some((row) => row.include));

	const queryClient = useQueryClient();
	const scan = createMutation(() => ({
		mutationFn: async (id: number) => {
			const { data, error } = await api.POST('/api/v1/library/scan', {
				body: { rootFolderId: id }
			});
			if (!data) throw new Error(error?.detail ?? error?.title ?? 'The scan failed.');
			return { id, data };
		},
		onSuccess: ({ id, data }) => {
			failure = '';
			summary = null;
			failures = [];
			scannedRoot = id;
			rows = reviewRows(data.folders);
		},
		onError: (error) => (failure = error.message)
	}));

	const adopt = createMutation(() => ({
		mutationFn: async () => {
			if (scannedRoot === null || profileId === null) throw new Error('Choose a quality profile.');
			const { data, error } = await api.POST('/api/v1/library/adopt', {
				body: {
					rootFolderId: scannedRoot,
					qualityProfileIds: [profileId],
					monitored,
					movies: selected
				}
			});
			if (!data) throw new Error(error?.detail ?? error?.title ?? 'The adoption failed.');
			return data;
		},
		onSuccess: (result) => {
			failure = '';
			summary = summarize(result, rows.length - selected.length);
			failures = result.failed;
			rows = remainingRows(rows, result);
			void queryClient.invalidateQueries({ queryKey: ['movies'] });
			void queryClient.invalidateQueries({ queryKey: ['root-folders'] });
		},
		onError: (error) => (failure = error.message)
	}));

	function setAll(on: boolean) {
		rows = rows.map((row) => (adoptable(row) ? { ...row, include: on } : row));
	}

	function setInclude(index: number, on: boolean) {
		rows[index] = { ...rows[index], include: on };
	}

	function fix(movie: MovieLookup) {
		if (fixing === null) return;
		rows[fixing] = withMatch(rows[fixing], movie);
	}

	function badgeVariant(row: ReviewRow) {
		if (row.corrected) return 'default' as const;
		return row.folder.status === 'matched' ? ('secondary' as const) : ('outline' as const);
	}

	const profileName = $derived(profiles.data?.find((profile) => profile.id === profileId)?.name);
	const rootPath = $derived(rootFolders.data?.find((folder) => folder.id === rootFolderId)?.path);
</script>

<svelte:head>
	<title>Import · Unseenarr</title>
</svelte:head>

{#if !allowed}
	<NoAccess anyOf={['import:run']} />
{:else}
	<h1 class="text-2xl font-semibold">Adopt a library</h1>
	<p class="mt-1 max-w-2xl text-sm text-muted-foreground">
		Scan a root folder for movie folders that Unseenarr does not know yet, check the matches, then
		adopt them. Adopting never moves, renames or deletes a file. The quality of each file is
		unknown until the parser fills it in.
	</p>

	<div class="mt-4 flex flex-wrap items-end gap-2">
		<div class="grid gap-2">
			<Label for="adopt-root">Root folder</Label>
			<Select.Root
				type="single"
				value={rootFolderId === null ? '' : String(rootFolderId)}
				onValueChange={(value) => (rootFolderId = Number(value))}
			>
				<Select.Trigger id="adopt-root" class="w-72 max-w-full font-mono text-xs">
					{rootPath ?? (rootFolders.isPending ? 'Loading…' : 'Choose a root folder')}
				</Select.Trigger>
				<Select.Content>
					{#each rootFolders.data ?? [] as folder (folder.id)}
						<Select.Item value={String(folder.id)} label={folder.path} />
					{/each}
				</Select.Content>
			</Select.Root>
		</div>
		<Button
			disabled={rootFolderId === null || scan.isPending}
			onclick={() => rootFolderId !== null && scan.mutate(rootFolderId)}
		>
			{scan.isPending ? 'Scanning…' : 'Scan'}
		</Button>
	</div>
	{#if rootFolders.isError}
		<p role="alert" class="mt-2 text-sm text-destructive">{rootFolders.error.message}</p>
	{:else if rootFolders.data?.length === 0}
		<p class="mt-2 text-sm text-muted-foreground">Add a root folder in the settings first.</p>
	{/if}

	{#if failure}
		<p role="alert" class="mt-4 text-sm text-destructive">{failure}</p>
	{/if}

	{#if summary}
		<section class="mt-4 rounded-lg border p-4" aria-labelledby="adopt-summary">
			<h2 id="adopt-summary" class="font-semibold">Adoption finished</h2>
			<p class="mt-1 text-sm">
				{summary.adopted}
				{summary.adopted === 1 ? 'movie' : 'movies'} adopted with {summary.files}
				{summary.files === 1 ? 'file' : 'files'}, {summary.failed} failed, {summary.skipped} left out.
			</p>
			{#if failures.length > 0}
				<ul class="mt-2 grid gap-1 text-sm">
					{#each failures as item (item.folderName)}
						<li>
							<code class="break-all">{item.folderName}</code>:
							<span class="text-destructive">{item.message}</span>
						</li>
					{/each}
				</ul>
			{/if}
		</section>
	{/if}

	{#if scannedRoot !== null && !scan.isPending}
		{#if rows.length === 0}
			<p class="mt-4 text-muted-foreground">
				{summary ? 'No folder is left to review.' : 'No new folder found in this root folder.'}
			</p>
		{:else}
			<div class="mt-6 flex flex-wrap items-end gap-4">
				<div class="grid gap-2">
					<Label for="adopt-profile">Quality profile</Label>
					<Select.Root
						type="single"
						value={profileId === null ? '' : String(profileId)}
						onValueChange={(value) => (profileId = Number(value))}
					>
						<Select.Trigger id="adopt-profile" class="w-56">
							{profileName ?? (profiles.isPending ? 'Loading…' : 'Choose a profile')}
						</Select.Trigger>
						<Select.Content>
							{#each profiles.data ?? [] as profile (profile.id)}
								<Select.Item value={String(profile.id)} label={profile.name} />
							{/each}
						</Select.Content>
					</Select.Root>
				</div>
				<Label class="gap-2 pb-2 font-normal">
					<Switch bind:checked={monitored} />
					Monitored
				</Label>
				<div class="ms-auto flex items-center gap-3 pb-1">
					<span class="text-sm text-muted-foreground">
						{selected.length} of {rows.length} selected
					</span>
					<Button
						disabled={selected.length === 0 || profileId === null || adopt.isPending}
						onclick={() => adopt.mutate()}
					>
						{adopt.isPending ? 'Adopting…' : `Adopt ${selected.length}`}
					</Button>
				</div>
			</div>
			{#if repeated.size > 0}
				<p class="mt-2 text-sm text-destructive">
					More than one selected folder points at the same movie. Only the first can be adopted.
				</p>
			{/if}

			<Table.Root class="mt-4">
				<Table.Header>
					<Table.Row>
						<Table.Head class="w-10">
							<Checkbox
								aria-label="Select all"
								checked={allSelected}
								indeterminate={someSelected && !allSelected}
								disabled={selectable.length === 0}
								onCheckedChange={(on) => setAll(on === true)}
							/>
						</Table.Head>
						<Table.Head>Folder</Table.Head>
						<Table.Head>Match</Table.Head>
						<Table.Head>Video files</Table.Head>
						<Table.Head class="text-end">Actions</Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each rows as row, index (row.folder.folderName)}
						<Table.Row>
							<Table.Cell>
								<Checkbox
									aria-label={`Adopt ${row.folder.folderName}`}
									checked={row.include}
									disabled={!adoptable(row)}
									onCheckedChange={(on) => setInclude(index, on === true)}
								/>
							</Table.Cell>
							<Table.Cell class="font-mono text-xs break-all">{row.folder.folderName}</Table.Cell>
							<Table.Cell>
								{#if row.tmdbId !== null}
									<span class="font-medium">{row.matchTitle}</span>
									<span class="text-muted-foreground">({row.matchYear ?? '—'})</span>
								{:else}
									<span class="text-muted-foreground">—</span>
								{/if}
								<Badge variant={badgeVariant(row)} class="ms-2">
									{row.corrected ? 'Corrected' : statusNames[row.folder.status]}
								</Badge>
							</Table.Cell>
							<Table.Cell class="text-xs">
								{#if row.folder.files.length === 0}
									<span class="text-muted-foreground">None</span>
								{:else}
									<span class="font-mono break-all">{row.folder.files[0].relativePath}</span>
									<span class="text-muted-foreground">
										{formatBytes(row.folder.files[0].size)}
										{#if row.folder.files.length > 1}
											, +{row.folder.files.length - 1} more
										{/if}
									</span>
								{/if}
							</Table.Cell>
							<Table.Cell class="text-end">
								{#if session.can('library:add') && row.folder.files.length > 0}
									<Button variant="outline" size="sm" onclick={() => {
										fixing = index;
										fixOpen = true;
									}}>
										{row.tmdbId === null ? 'Find match' : 'Change match'}
									</Button>
								{/if}
							</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		{/if}
	{/if}

	{#if fixing !== null && rows[fixing]}
		<AdoptMatchDialog
			bind:open={fixOpen}
			folderName={rows[fixing].folder.folderName}
			term={rows[fixing].folder.title}
			onselect={fix}
		/>
	{/if}
{/if}
