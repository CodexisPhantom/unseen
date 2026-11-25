<script lang="ts">
	import { createMutation, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import * as Dialog from '#lib/components/ui/dialog/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import { Switch } from '#lib/components/ui/switch/index.js';
	import {
		draftOf,
		draftProblem,
		requestBody,
		type Draft,
		type DownloadClient
	} from '#lib/download-clients.js';

	let {
		open = $bindable(false),
		client
	}: { open: boolean; client: DownloadClient | null } = $props();

	let draft = $state<Draft>(draftOf(null));
	let failure = $state('');

	const queryClient = useQueryClient();

	// Start from the client being edited, or from a new one.
	$effect(() => {
		if (!open) return;
		draft = draftOf(client);
		failure = '';
	});

	const problem = $derived(draftProblem(draft));

	const save = createMutation(() => ({
		mutationFn: async () => {
			const { error } = client
				? await api.PATCH('/api/v1/download-clients/{id}', {
						params: { path: { id: client.id } },
						body: requestBody(draft, true)
					})
				: await api.POST('/api/v1/download-clients', { body: requestBody(draft, false) });
			if (error) throw new Error(error.detail ?? error.title);
		},
		onSuccess: async () => {
			await queryClient.invalidateQueries({ queryKey: ['download-clients'] });
			open = false;
		},
		onError: (error) => (failure = error.message)
	}));
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-xl">
		<Dialog.Header>
			<Dialog.Title>{client ? `Edit ${client.name}` : 'New download client'}</Dialog.Title>
			<Dialog.Description>
				Unseen sends torrents to qBittorrent and follows them by category. Save the client, then
				use Test to check the login.
			</Dialog.Description>
		</Dialog.Header>
		<form
			class="grid gap-4"
			onsubmit={(event) => {
				event.preventDefault();
				if (!problem) save.mutate();
			}}
		>
			<div class="grid gap-2">
				<Label for="client-name">Name</Label>
				<Input id="client-name" required maxlength={64} bind:value={draft.name} />
			</div>
			<div class="grid gap-2">
				<Label for="client-url">Address</Label>
				<Input
					id="client-url"
					required
					type="url"
					placeholder="http://qbittorrent:8080"
					bind:value={draft.url}
				/>
			</div>
			<div class="grid gap-4 sm:grid-cols-2">
				<div class="grid gap-2">
					<Label for="client-username">Username</Label>
					<Input id="client-username" autocomplete="off" bind:value={draft.username} />
				</div>
				<div class="grid gap-2">
					<Label for="client-password">Password</Label>
					<Input
						id="client-password"
						type="password"
						autocomplete="new-password"
						placeholder={client?.hasPassword ? 'Leave empty to keep it' : ''}
						bind:value={draft.password}
					/>
				</div>
			</div>
			{#if client?.passwordMustBeReentered}
				<p class="text-sm text-destructive">
					The stored password cannot be read. Enter it again.
				</p>
			{/if}
			<div class="grid gap-2">
				<Label for="client-api-key">API key</Label>
				<Input
					id="client-api-key"
					type="password"
					autocomplete="off"
					placeholder={client?.hasApiKey ? 'Leave empty to keep it' : 'qbt_…'}
					class="font-mono"
					bind:value={draft.apiKey}
				/>
				<p class="text-xs text-muted-foreground">
					qBittorrent 5.2 or later: Options, Web UI, API key. When a key is set, Unseen uses it
					and ignores the username and password.
				</p>
			</div>
			{#if client?.apiKeyMustBeReentered}
				<p class="text-sm text-destructive">The stored API key cannot be read. Enter it again.</p>
			{/if}
			<div class="grid gap-2">
				<Label for="client-category">Category</Label>
				<Input id="client-category" required bind:value={draft.category} />
				<p class="text-xs text-muted-foreground">
					Unseen finds its own torrents by this category. Radarr uses <code>radarr</code>.
				</p>
			</div>
			<fieldset class="grid gap-2">
				<legend class="mb-1 text-sm font-medium">Remote path mapping</legend>
				<p class="text-xs text-muted-foreground">
					Fill both when qBittorrent sees the download folder under another path than Unseen
					does.
				</p>
				<div class="grid gap-4 sm:grid-cols-2">
					<Input
						aria-label="Path as qBittorrent sees it"
						placeholder="/downloads"
						class="font-mono"
						bind:value={draft.remotePath}
					/>
					<Input
						aria-label="Path as Unseen sees it"
						placeholder="/data/downloads"
						class="font-mono"
						bind:value={draft.localPath}
					/>
				</div>
			</fieldset>
			<div class="flex items-center justify-between gap-4">
				<Label for="client-enabled">Enabled</Label>
				<Switch id="client-enabled" bind:checked={draft.enabled} />
			</div>
			{#if failure || problem}
				<p role="alert" class="text-sm text-destructive">{failure || problem}</p>
			{/if}
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (open = false)}>Cancel</Button>
				<Button type="submit" disabled={save.isPending || problem !== ''}>Save</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
