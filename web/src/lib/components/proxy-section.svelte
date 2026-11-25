<script lang="ts">
	import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import { Badge } from '#lib/components/ui/badge/index.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import { Switch } from '#lib/components/ui/switch/index.js';
	import { changes, draftOf, summary, type Draft } from '#lib/proxy.js';
	import { session } from '#lib/session.svelte.js';

	const manage = $derived(session.can('settings:manage'));

	const settings = createQuery(() => ({
		queryKey: ['proxy-settings'],
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/settings/proxy');
			if (!data) throw new Error(error?.detail ?? 'The proxy settings could not be loaded.');
			return data;
		}
	}));

	let draft = $state<Draft>({ url: '', bypassLocal: true });
	let failure = $state('');
	let result = $state('');

	// Fill the form once the settings arrive, and again after each save.
	$effect(() => {
		if (settings.data) draft = draftOf(settings.data);
	});

	const body = $derived(settings.data ? changes(draft, settings.data) : {});
	const dirty = $derived(Object.keys(body).length > 0);

	const queryClient = useQueryClient();
	const save = createMutation(() => ({
		mutationFn: async (patch: { url?: string; bypassLocal?: boolean }) => {
			const { error } = await api.PUT('/api/v1/settings/proxy', { body: patch });
			if (error) throw new Error(error.detail ?? error.title);
		},
		onSuccess: () => {
			failure = '';
			result = '';
		},
		onError: (error) => (failure = error.message),
		onSettled: () => queryClient.invalidateQueries({ queryKey: ['proxy-settings'] })
	}));
	const test = createMutation(() => ({
		mutationFn: async () => {
			const { data, error } = await api.POST('/api/v1/settings/proxy/test', { body: {} });
			if (!data) throw new Error(error?.detail ?? error?.title ?? 'The test failed.');
			return data;
		},
		onSuccess: (data) => {
			failure = '';
			result = `The proxy carried a request. The target answered HTTP ${data.status}.`;
		},
		onError: (error) => {
			result = '';
			failure = error.message;
		}
	}));
</script>

<section aria-labelledby="proxy-title">
	<h2 id="proxy-title" class="text-lg font-semibold">Proxy for indexers</h2>
	<p class="mt-1 text-sm text-muted-foreground">
		Indexer requests can go through an HTTP, HTTPS or SOCKS5 proxy, so that an indexer does not see
		this server's address. Each indexer has a switch to use it. TMDB, download clients and imports
		never use it.
	</p>

	{#if settings.isPending}
		<p class="mt-4 text-muted-foreground">Loading…</p>
	{:else if settings.isError}
		<p class="mt-4 text-destructive">{settings.error.message}</p>
	{:else}
		{@const current = settings.data}
		{@const note = summary(current)}
		<form
			class="mt-4 grid max-w-xl gap-4"
			onsubmit={(event) => {
				event.preventDefault();
				save.mutate(body);
			}}
		>
			<div class="grid gap-2">
				<Label for="proxy-url">Proxy URL</Label>
				<Input
					id="proxy-url"
					type="text"
					autocomplete="off"
					class="font-mono"
					placeholder="socks5://user:password@proxy.example:1080"
					disabled={!manage || current.overridden.includes('url')}
					bind:value={draft.url}
				/>
				{#if note}
					<p class="text-xs text-muted-foreground">
						Set: <code>{note}</code>. Leave the field empty to keep it.
					</p>
				{/if}
				{#if current.overridden.includes('url')}
					<Badge variant="secondary" class="w-fit">Set by an environment variable</Badge>
				{/if}
			</div>
			<div class="flex items-center gap-2">
				<Switch
					id="proxy-bypass-local"
					disabled={!manage || current.overridden.includes('bypassLocal')}
					bind:checked={draft.bypassLocal}
				/>
				<Label for="proxy-bypass-local">Skip the proxy for local addresses</Label>
				{#if current.overridden.includes('bypassLocal')}
					<Badge variant="secondary">Set by an environment variable</Badge>
				{/if}
			</div>
			{#if failure}
				<p role="alert" class="text-sm text-destructive">{failure}</p>
			{/if}
			{#if result}
				<p role="status" class="text-sm text-muted-foreground">{result}</p>
			{/if}
			{#if manage}
				<div class="flex flex-wrap gap-2">
					<Button type="submit" disabled={!dirty || save.isPending}>Save</Button>
					<Button
						type="button"
						variant="outline"
						disabled={!current.hasUrl || test.isPending}
						onclick={() => test.mutate()}
					>
						Test
					</Button>
					{#if current.hasUrl && !current.overridden.includes('url')}
						<Button
							type="button"
							variant="outline"
							disabled={save.isPending}
							onclick={() => save.mutate({ url: '' })}
						>
							Remove the proxy
						</Button>
					{/if}
				</div>
			{/if}
		</form>
	{/if}
</section>
