<script lang="ts">
	import DownloadClientsSection from '#lib/components/download-clients-section.svelte';
	import LibrariesSection from '#lib/components/libraries-section.svelte';
	import ProxySection from '#lib/components/proxy-section.svelte';
	import RootFoldersSection from '#lib/components/root-folders-section.svelte';
	import SecuritySection from '#lib/components/security-section.svelte';
	import { session } from '#lib/session.svelte.js';
</script>

<svelte:head>
	<title>Settings · Unseen</title>
</svelte:head>

<h1 class="text-2xl font-semibold">Settings</h1>

<div class="mt-6 grid gap-10">
	{#if session.can('settings:read')}
		<LibrariesSection />
		<RootFoldersSection />
	{/if}
	{#if session.can('settings:read')}
		<ProxySection />
	{/if}
	{#if session.can('clients:read')}
		<DownloadClientsSection />
	{/if}
	<SecuritySection />
</div>
