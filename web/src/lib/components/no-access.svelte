<script lang="ts">
	import type { Permission } from '#lib/session.svelte.js';

	let { anyOf }: { anyOf: readonly Permission[] } = $props();
</script>

<h1 class="text-2xl font-semibold">No access</h1>
<p class="mt-2 text-muted-foreground">
	Your account cannot open this page. It needs one of these permissions:
</p>
<ul class="mt-2 list-inside list-disc text-sm">
	{#each anyOf as permission (permission)}
		<li><code>{permission}</code></li>
	{/each}
</ul>
<p class="mt-2 text-muted-foreground">Ask the Owner or an administrator to grant one.</p>
