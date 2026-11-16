<script lang="ts">
	import { createQuery } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';

	const health = createQuery(() => ({
		queryKey: ['health'],
		queryFn: async () => {
			// A 503 carries the same body as a 200, with `database: "error"`.
			const { data, error } = await api.GET('/api/v1/health');
			return data ?? error;
		}
	}));
</script>

<h1 class="text-2xl font-semibold">System</h1>

{#if health.isPending}
	<p class="mt-4 text-muted-foreground">Loading…</p>
{:else if health.isError}
	<p class="mt-4 text-destructive">The API does not answer.</p>
{:else if health.data}
	<dl class="mt-4 grid max-w-sm grid-cols-2 gap-2 text-sm">
		<dt class="text-muted-foreground">Version</dt>
		<dd data-testid="version">{health.data.version}</dd>
		<dt class="text-muted-foreground">Database</dt>
		<dd data-testid="database" class={health.data.database === 'ok' ? '' : 'text-destructive'}>
			{health.data.database}
		</dd>
	</dl>
{/if}
