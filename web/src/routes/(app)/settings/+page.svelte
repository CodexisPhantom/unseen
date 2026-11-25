<script lang="ts">
	import RootFoldersSection from '#lib/components/root-folders-section.svelte';
	import { session } from '#lib/session.svelte.js';
</script>

<svelte:head>
	<title>Settings · Unseen</title>
</svelte:head>

<h1 class="text-2xl font-semibold">Settings</h1>

<div class="mt-6 grid gap-10">
	{#if session.can('settings:read')}
		<RootFoldersSection />
	{/if}
</div>
