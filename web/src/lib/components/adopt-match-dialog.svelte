<script lang="ts">
	import { createMutation } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import type { MovieLookup } from '#lib/adopt.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import * as Dialog from '#lib/components/ui/dialog/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import { posterUrl } from '#lib/library.js';

	let {
		open = $bindable(false),
		folderName,
		term,
		onselect
	}: {
		open: boolean;
		folderName: string;
		/** The search text to start with. */
		term: string;
		onselect: (movie: MovieLookup) => void;
	} = $props();

	let text = $state('');
	let results = $state<MovieLookup[] | null>(null);
	let failure = $state('');

	const search = createMutation(() => ({
		mutationFn: async (value: string) => {
			const { data, error } = await api.GET('/api/v1/movies/lookup', {
				params: { query: { term: value } }
			});
			if (!data) throw new Error(error?.detail ?? 'The search failed.');
			return data;
		},
		onSuccess: (data) => {
			failure = '';
			results = data;
		},
		onError: (error) => (failure = error.message)
	}));

	// Start from the folder's title each time the dialog opens.
	$effect(() => {
		if (!open) return;
		text = term;
		results = null;
		failure = '';
		if (term.trim()) search.mutate(term.trim());
	});

	function choose(movie: MovieLookup) {
		onselect(movie);
		open = false;
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-lg">
		<Dialog.Header>
			<Dialog.Title>Choose the movie</Dialog.Title>
			<Dialog.Description>
				For the folder <code class="break-all">{folderName}</code>. Search by title, or use
				<code>tmdb:603</code> or <code>imdb:tt0133093</code>.
			</Dialog.Description>
		</Dialog.Header>
		<form
			class="flex gap-2"
			onsubmit={(event) => {
				event.preventDefault();
				if (text.trim()) search.mutate(text.trim());
			}}
		>
			<Input aria-label="Search TMDB" placeholder="Title, tmdb: or imdb:" bind:value={text} />
			<Button type="submit" disabled={search.isPending || !text.trim()}>Search</Button>
		</form>
		{#if failure}
			<p role="alert" class="text-sm text-destructive">{failure}</p>
		{/if}
		{#if search.isPending}
			<p class="text-sm text-muted-foreground">Searching…</p>
		{:else if results && results.length === 0}
			<p class="text-sm text-muted-foreground">Nothing found.</p>
		{:else if results}
			<ul class="grid gap-2">
				{#each results as movie (movie.tmdbId)}
					{@const poster = posterUrl(movie.posterPath, 92)}
					<li>
						<button
							type="button"
							class="flex w-full gap-3 rounded-lg border p-2 text-start hover:bg-muted"
							onclick={() => choose(movie)}
						>
							{#if poster}
								<img
									src={poster}
									alt=""
									width="46"
									height="69"
									loading="lazy"
									class="h-[69px] w-[46px] shrink-0 rounded object-cover"
								/>
							{:else}
								<div class="h-[69px] w-[46px] shrink-0 rounded bg-muted"></div>
							{/if}
							<span class="min-w-0">
								<span class="block font-medium">
									{movie.title}
									<span class="font-normal text-muted-foreground">({movie.year ?? '—'})</span>
								</span>
								<span class="line-clamp-2 text-xs text-muted-foreground">{movie.overview}</span>
							</span>
						</button>
					</li>
				{/each}
			</ul>
		{/if}
	</Dialog.Content>
</Dialog.Root>
