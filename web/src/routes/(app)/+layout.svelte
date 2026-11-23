<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import AppSidebar from '#lib/components/app-sidebar.svelte';
	import NoAccess from '#lib/components/no-access.svelte';
	import ThemeToggle from '#lib/components/theme-toggle.svelte';
	import * as Sidebar from '#lib/components/ui/sidebar/index.js';
	import { entryFor } from '#lib/nav.js';
	import { session } from '#lib/session.svelte.js';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();

	// The page's section, when it needs a permission. The API still answers 403.
	const entry = $derived(entryFor(page.url.pathname.slice(resolve('').length)));
	const allowed = $derived(!entry || session.canAny(entry.anyOf));
</script>

<Sidebar.Provider>
	<AppSidebar />
	<Sidebar.Inset>
		<header class="flex h-12 shrink-0 items-center justify-between border-b px-2">
			<Sidebar.Trigger />
			<ThemeToggle />
		</header>
		<main class="flex-1 p-4">
			{#if allowed}
				{@render children()}
			{:else if entry}
				<NoAccess anyOf={entry.anyOf} />
			{/if}
		</main>
	</Sidebar.Inset>
</Sidebar.Provider>
