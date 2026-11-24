<script lang="ts">
	import { createQuery, keepPreviousData } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import { actionLabel, dataText, pageCount, startOfDay, targetLabel } from '#lib/audit.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import * as Table from '#lib/components/ui/table/index.js';
	import { session } from '#lib/session.svelte.js';

	const PAGE_SIZE = 50;

	let page = $state(1);
	let action = $state('');
	let from = $state('');
	let to = $state('');

	const allowed = $derived(session.can('system:audit'));

	const log = createQuery(() => ({
		queryKey: ['audit-log', page, action, from, to],
		enabled: allowed,
		placeholderData: keepPreviousData,
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/audit-log', {
				params: {
					query: {
						page,
						pageSize: PAGE_SIZE,
						action: action.trim() || undefined,
						from: startOfDay(from),
						// The chosen day is included: up to the start of the next.
						to: startOfDay(to, 1)
					}
				}
			});
			if (!data) throw new Error(error?.detail ?? 'The audit log could not be loaded.');
			return data;
		}
	}));
	const pages = $derived(pageCount(log.data?.total ?? 0, PAGE_SIZE));

	/** A new filter starts on the first page. */
	function filtered(change: () => void) {
		change();
		page = 1;
	}
</script>

<svelte:head>
	<title>Audit log · Unseen</title>
</svelte:head>

<h1 class="text-2xl font-semibold">Audit log</h1>
<p class="mt-1 text-sm text-muted-foreground">Who changed what, newest first.</p>

{#if !allowed}
	<p class="mt-4 text-muted-foreground">You do not have access to the audit log.</p>
{:else}
	<div class="mt-4 flex flex-wrap items-end gap-3">
		<label class="text-sm">
			<span class="block text-muted-foreground">Action</span>
			<Input
				class="w-48"
				placeholder="role.updated"
				value={action}
				oninput={(event) => filtered(() => (action = event.currentTarget.value))}
			/>
		</label>
		<label class="text-sm">
			<span class="block text-muted-foreground">From</span>
			<Input
				type="date"
				value={from}
				oninput={(event) => filtered(() => (from = event.currentTarget.value))}
			/>
		</label>
		<label class="text-sm">
			<span class="block text-muted-foreground">To</span>
			<Input
				type="date"
				value={to}
				oninput={(event) => filtered(() => (to = event.currentTarget.value))}
			/>
		</label>
	</div>

	{#if log.isPending}
		<p class="mt-4 text-muted-foreground">Loading…</p>
	{:else if log.isError}
		<p class="mt-4 text-destructive">{log.error.message}</p>
	{:else if log.data.items.length === 0}
		<p class="mt-4 text-muted-foreground">No entries.</p>
	{:else}
		<Table.Root class="mt-4">
			<Table.Header>
				<Table.Row>
					<Table.Head>Time</Table.Head>
					<Table.Head>Actor</Table.Head>
					<Table.Head>Action</Table.Head>
					<Table.Head>Target</Table.Head>
					<Table.Head>Details</Table.Head>
					<Table.Head>IP</Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each log.data.items as row (row.id)}
					<Table.Row>
						<Table.Cell class="whitespace-nowrap">
							{new Date(row.createdAt).toLocaleString()}
						</Table.Cell>
						<Table.Cell>
							{row.actorName}{#if row.apiKeyId !== null}
								<span class="text-muted-foreground"> (API key {row.apiKeyId})</span>
							{/if}
						</Table.Cell>
						<Table.Cell>{actionLabel(row.action)}</Table.Cell>
						<Table.Cell>{targetLabel(row.targetKind, row.targetId)}</Table.Cell>
						<Table.Cell>
							{#if row.data}
								<details>
									<summary class="cursor-pointer text-sm">Show</summary>
									<pre class="mt-1 text-xs">{dataText(row.data)}</pre>
								</details>
							{/if}
						</Table.Cell>
						<Table.Cell class="font-mono text-xs">{row.ipAddress ?? ''}</Table.Cell>
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
		<div class="mt-4 flex items-center gap-3 text-sm">
			<Button variant="outline" size="sm" disabled={page <= 1} onclick={() => (page -= 1)}>
				Previous
			</Button>
			<span>Page {page} of {pages}</span>
			<Button variant="outline" size="sm" disabled={page >= pages} onclick={() => (page += 1)}>
				Next
			</Button>
		</div>
	{/if}
{/if}
