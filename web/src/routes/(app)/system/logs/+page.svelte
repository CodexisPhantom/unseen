<script lang="ts">
	import { createInfiniteQuery } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import type { components } from '#lib/api/schema.js';
	import { Badge } from '#lib/components/ui/badge/index.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import * as Select from '#lib/components/ui/select/index.js';
	import * as Table from '#lib/components/ui/table/index.js';
	import {
		LOG_LEVELS,
		type LogLevelFilter,
		fieldsText,
		fileOptionLabel,
		formatTimestamp,
		hasFields,
		levelBadgeVariant,
		levelLabel,
		levelQueryParam,
		parseLevelFilter
	} from '#lib/logs.js';
	import { session } from '#lib/session.svelte.js';

	const PAGE_SIZE = 100;

	type LogsPage = components['schemas']['LogsResponse'];

	let level = $state<LogLevelFilter>('all');
	let search = $state('');
	let chosenFile = $state('');
	let downloading = $state(false);
	let downloadError = $state('');

	const allowed = $derived(session.can('system:logs'));

	const logs = createInfiniteQuery(() => ({
		queryKey: ['logs', level, search.trim()],
		enabled: allowed,
		initialPageParam: undefined as string | undefined,
		getNextPageParam: (lastPage: LogsPage) => lastPage.nextBefore ?? undefined,
		queryFn: async ({ pageParam }) => {
			const { data, error } = await api.GET('/api/v1/logs', {
				params: {
					query: {
						level: levelQueryParam(level),
						search: search.trim() || undefined,
						before: pageParam,
						limit: PAGE_SIZE
					}
				}
			});
			if (!data) throw new Error(error?.detail ?? 'The logs could not be loaded.');
			return data;
		}
	}));

	const entries = $derived(logs.data?.pages.flatMap((page) => page.items) ?? []);
	const files = $derived(logs.data?.pages[0]?.files ?? []);
	const retentionDays = $derived(logs.data?.pages[0]?.retentionDays);
	const selectedFile = $derived(
		files.some((file) => file.name === chosenFile) ? chosenFile : (files[0]?.name ?? '')
	);
	const selectedFileInfo = $derived(files.find((file) => file.name === selectedFile));

	function triggerTextDownload(filename: string, content: string) {
		const blob = new Blob([content], { type: 'text/plain;charset=utf-8' });
		const url = URL.createObjectURL(blob);
		const anchor = document.createElement('a');
		anchor.href = url;
		anchor.download = filename;
		document.body.append(anchor);
		anchor.click();
		anchor.remove();
		URL.revokeObjectURL(url);
	}

	async function downloadSelectedFile() {
		if (!selectedFile || downloading) return;
		downloading = true;
		downloadError = '';
		try {
			const { data, error } = await api.GET('/api/v1/logs/files/{name}', {
				params: { path: { name: selectedFile } },
				parseAs: 'text'
			});
			if (error || typeof data !== 'string') {
				throw new Error(error?.detail ?? error?.title ?? 'The log file could not be downloaded.');
			}
			triggerTextDownload(selectedFile, data);
		} catch (err) {
			downloadError = err instanceof Error ? err.message : 'The log file could not be downloaded.';
		} finally {
			downloading = false;
		}
	}
</script>

<svelte:head>
	<title>Logs · Unseen</title>
</svelte:head>

<div class="flex flex-wrap items-start justify-between gap-3">
	<div>
		<h1 class="text-2xl font-semibold">Logs</h1>
		<p class="mt-1 text-sm text-muted-foreground">
			Application logs, newest first.{#if retentionDays !== undefined}
				Daily files are kept for {retentionDays}
				{retentionDays === 1 ? 'day' : 'days'}.
			{/if}
		</p>
	</div>
	{#if allowed}
		<Button
			variant="outline"
			size="sm"
			disabled={logs.isFetching}
			onclick={() => logs.refetch()}
		>
			{logs.isFetching ? 'Refreshing…' : 'Refresh'}
		</Button>
	{/if}
</div>

{#if !allowed}
	<p class="mt-4 text-muted-foreground">You do not have access to the logs.</p>
{:else}
	<div class="mt-4 flex flex-wrap items-end justify-between gap-3">
		<div class="flex flex-wrap items-end gap-3">
			<div class="text-sm">
				<span class="block text-muted-foreground">Level</span>
				<Select.Root
					type="single"
					value={level}
					onValueChange={(value) => (level = parseLevelFilter(value))}
				>
					<Select.Trigger class="w-36" aria-label="Minimum log level">
						{levelLabel(level)}
					</Select.Trigger>
					<Select.Content>
						{#each LOG_LEVELS as option (option)}
							<Select.Item value={option} label={levelLabel(option)} />
						{/each}
					</Select.Content>
				</Select.Root>
			</div>
			<label class="text-sm">
				<span class="block text-muted-foreground">Search</span>
				<Input
					class="w-64"
					placeholder="Filter message, target, or fields"
					value={search}
					oninput={(event) => (search = event.currentTarget.value)}
				/>
			</label>
		</div>

		{#if files.length > 0}
			<div class="flex flex-wrap items-end gap-2">
				<div class="text-sm">
					<span class="block text-muted-foreground">Daily file</span>
					<Select.Root
						type="single"
						value={selectedFile}
						onValueChange={(value) => {
							chosenFile = value;
							downloadError = '';
						}}
					>
						<Select.Trigger class="w-64" aria-label="Log file to download">
							{selectedFileInfo ? fileOptionLabel(selectedFileInfo) : selectedFile}
						</Select.Trigger>
						<Select.Content>
							{#each files as file (file.name)}
								<Select.Item value={file.name} label={fileOptionLabel(file)} />
							{/each}
						</Select.Content>
					</Select.Root>
				</div>
				<Button
					variant="outline"
					disabled={!selectedFile || downloading}
					onclick={downloadSelectedFile}
				>
					{downloading ? 'Downloading…' : 'Download'}
				</Button>
			</div>
		{/if}
	</div>

	{#if downloadError}
		<p role="alert" class="mt-3 text-sm text-destructive">{downloadError}</p>
	{/if}

	{#if logs.isPending}
		<p class="mt-4 text-muted-foreground">Loading…</p>
	{:else if logs.isError}
		<p class="mt-4 text-destructive">{logs.error.message}</p>
	{:else if entries.length === 0}
		<p class="mt-4 text-muted-foreground">No log entries.</p>
	{:else}
		<Table.Root class="mt-4">
			<Table.Header>
				<Table.Row>
					<Table.Head>Time</Table.Head>
					<Table.Head>Level</Table.Head>
					<Table.Head>Target</Table.Head>
					<Table.Head>Message</Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each entries as entry, index (`${entry.timestamp}-${index}`)}
					<Table.Row>
						<Table.Cell class="whitespace-nowrap font-mono text-xs">
							{formatTimestamp(entry.timestamp)}
						</Table.Cell>
						<Table.Cell>
							<Badge variant={levelBadgeVariant(entry.level)}>{levelLabel(entry.level)}</Badge>
						</Table.Cell>
						<Table.Cell class="font-mono text-xs text-muted-foreground">
							{entry.target}
						</Table.Cell>
						<Table.Cell class="max-w-2xl">
							<div class="font-mono text-xs break-words whitespace-pre-wrap">{entry.message}</div>
							{#if hasFields(entry.fields)}
								<details class="mt-1">
									<summary class="cursor-pointer text-xs text-muted-foreground">Fields</summary>
									<pre class="mt-1 overflow-x-auto text-xs">{fieldsText(entry.fields)}</pre>
								</details>
							{/if}
						</Table.Cell>
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>

		{#if logs.hasNextPage}
			<div class="mt-4">
				<Button
					variant="outline"
					size="sm"
					disabled={logs.isFetchingNextPage}
					onclick={() => logs.fetchNextPage()}
				>
					{logs.isFetchingNextPage ? 'Loading older…' : 'Load older'}
				</Button>
			</div>
		{/if}
	{/if}
{/if}
