<script lang="ts">
	import AppSidebar from '#lib/components/app-sidebar.svelte';
	import ThemeToggle from '#lib/components/theme-toggle.svelte';
	import * as Sidebar from '#lib/components/ui/sidebar/index.js';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();
</script>

<Sidebar.Provider>
	<AppSidebar />
	<Sidebar.Inset>
		<header class="flex h-12 shrink-0 items-center justify-between border-b px-2">
			<Sidebar.Trigger />
			<ThemeToggle />
		</header>
		<main class="flex-1 p-4">
			{@render children()}
		</main>
	</Sidebar.Inset>
</Sidebar.Provider>
