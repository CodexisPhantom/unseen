<script lang="ts">
	import { createQuery } from '@tanstack/svelte-query';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import { api } from '#lib/api/client.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import * as Dialog from '#lib/components/ui/dialog/index.js';

	let {
		open = $bindable(false),
		start = '',
		onselect
	}: { open: boolean; start?: string; onselect: (path: string) => void } = $props();

	let current = $state('');

	$effect(() => {
		if (open) current = start.trim();
	});

	const listing = createQuery(() => ({
		queryKey: ['filesystem', current],
		enabled: open,
		retry: false,
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/filesystem', {
				params: { query: { path: current } }
			});
			if (!data) throw new Error(error?.detail ?? error?.title ?? 'The folder could not be listed.');
			return data;
		}
	}));
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-xl">
		<Dialog.Header>
			<Dialog.Title>Choose a folder</Dialog.Title>
			<Dialog.Description>The folders on the server that Unseenarr can see.</Dialog.Description>
		</Dialog.Header>
		<div class="grid gap-2">
			<div class="flex items-center gap-2">
				<Button
					type="button"
					variant="outline"
					size="icon"
					aria-label="Up one folder"
					disabled={!listing.data || listing.data.parent === null}
					onclick={() => (current = listing.data?.parent ?? '')}
				>
					<ArrowUpIcon />
				</Button>
				<code class="min-w-0 flex-1 truncate rounded-md border bg-muted px-2 py-1.5 text-sm">
					{listing.data ? listing.data.path || 'Top level' : current || 'Top level'}
				</code>
			</div>
			{#if listing.isPending}
				<p class="text-muted-foreground">Loading…</p>
			{:else if listing.isError}
				<p role="alert" class="text-sm text-destructive">{listing.error.message}</p>
				<Button type="button" variant="outline" onclick={() => (current = '')}>
					Go to the top level
				</Button>
			{:else if listing.data.folders.length === 0}
				<p class="text-muted-foreground">No folders here.</p>
			{:else}
				<ul class="max-h-80 overflow-y-auto rounded-md border">
					{#each listing.data.folders as folder (folder.path)}
						<li class="border-b last:border-b-0">
							<button
								type="button"
								class="flex w-full items-center gap-2 px-3 py-2 text-start text-sm hover:bg-accent"
								onclick={() => (current = folder.path)}
							>
								<FolderIcon class="size-4 shrink-0 text-muted-foreground" />
								<span class="truncate">{folder.name}</span>
							</button>
						</li>
					{/each}
				</ul>
			{/if}
		</div>
		<Dialog.Footer>
			<Button type="button" variant="outline" onclick={() => (open = false)}>Cancel</Button>
			<Button
				type="button"
				disabled={!listing.data?.path}
				onclick={() => {
					if (listing.data) onselect(listing.data.path);
					open = false;
				}}
			>
				Select this folder
			</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
