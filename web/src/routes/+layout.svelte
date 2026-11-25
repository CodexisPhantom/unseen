<script lang="ts">
	import './layout.css';
	import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query';
	import { ModeWatcher } from 'mode-watcher';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();

	const queryClient = new QueryClient();
</script>

<ModeWatcher />
<QueryClientProvider client={queryClient}>
	{@render children()}
</QueryClientProvider>
