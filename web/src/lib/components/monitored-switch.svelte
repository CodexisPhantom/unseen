<script lang="ts">
	import { createMutation, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import { Switch } from '#lib/components/ui/switch/index.js';
	import type { Movie } from '#lib/library.js';
	import { session } from '#lib/session.svelte.js';

	let { movie, onerror }: { movie: Movie; onerror?: (message: string) => void } = $props();

	const queryClient = useQueryClient();

	/** Writes the new state into the cached list and detail, so the switch moves at once. */
	function patchCache(monitored: boolean) {
		queryClient.setQueryData<Movie[]>(['movies'], (list) =>
			list?.map((m) => (m.id === movie.id ? { ...m, monitored } : m))
		);
		queryClient.setQueryData<Movie>(['movie', movie.id], (one) =>
			one ? { ...one, monitored } : one
		);
	}

	const toggle = createMutation(() => ({
		mutationFn: async (monitored: boolean) => {
			const { error } = await api.PATCH('/api/v1/movies/{id}', {
				params: { path: { id: movie.id } },
				body: { monitored }
			});
			if (error) throw new Error(error.detail ?? error.title);
		},
		onMutate: async (monitored: boolean) => {
			await queryClient.cancelQueries({ queryKey: ['movies'] });
			patchCache(monitored);
			return { was: !monitored };
		},
		onError: (error, _monitored, context) => {
			if (context) patchCache(context.was);
			onerror?.(error.message);
		},
		onSettled: () => {
			queryClient.invalidateQueries({ queryKey: ['movies'] });
			queryClient.invalidateQueries({ queryKey: ['movie', movie.id] });
		}
	}));
</script>

<Switch
	size="sm"
	aria-label={`Monitor ${movie.title}`}
	checked={movie.monitored}
	disabled={!session.can('library:edit')}
	onCheckedChange={(on) => toggle.mutate(on)}
/>
