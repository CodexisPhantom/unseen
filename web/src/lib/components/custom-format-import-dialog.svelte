<script lang="ts">
	import UploadIcon from '@lucide/svelte/icons/upload';
	import { createMutation, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import * as Dialog from '#lib/components/ui/dialog/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import { Textarea } from '#lib/components/ui/textarea/index.js';
	import { problemMessage, type RadarrCustomFormat } from '#lib/custom-formats.js';

	let { open = $bindable(false) }: { open: boolean } = $props();

	let jsonText = $derived(open ? '' : '');
	let failure = $derived(open ? '' : '');

	const queryClient = useQueryClient();

	async function onFileSelected(event: Event) {
		const input = event.currentTarget as HTMLInputElement;
		const file = input.files?.[0];
		if (!file) return;
		jsonText = await file.text();
		failure = '';
		input.value = '';
	}

	const importMutation = createMutation(() => ({
		mutationFn: async () => {
			let parsed: unknown;
			try {
				parsed = JSON.parse(jsonText);
			} catch (err) {
				throw new Error(
					err instanceof Error ? `Invalid JSON: ${err.message}` : 'Invalid JSON syntax.',
					{ cause: err }
				);
			}
			const { error } = await api.POST('/api/v1/custom-formats/import', {
				body: parsed as RadarrCustomFormat | RadarrCustomFormat[]
			});
			if (error) throw new Error(problemMessage(error));
		},
		onSuccess: async () => {
			await queryClient.invalidateQueries({ queryKey: ['custom-formats'] });
			open = false;
		},
		onError: (error) => {
			failure = error.message;
		}
	}));
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-2xl">
		<Dialog.Header>
			<Dialog.Title>Import Radarr custom formats</Dialog.Title>
			<Dialog.Description>
				Paste one custom format JSON object or an array of formats exported from Radarr or TRaSH
				Guides, or load a JSON file.
			</Dialog.Description>
		</Dialog.Header>

		<form
			class="grid gap-4"
			onsubmit={(event) => {
				event.preventDefault();
				if (jsonText.trim()) importMutation.mutate();
			}}
		>
			<div class="grid gap-2">
				<div class="flex items-center justify-between">
					<Label for="cf-import-json">Radarr JSON</Label>
					<label
						class="inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-md border border-input bg-background px-3 text-xs font-medium shadow-xs hover:bg-accent hover:text-accent-foreground"
					>
						<UploadIcon class="size-4" />
						Load file
						<input
							type="file"
							accept=".json,application/json"
							class="sr-only"
							onchange={onFileSelected}
						/>
					</label>
				</div>
				<Textarea
					id="cf-import-json"
					rows={12}
					class="font-mono text-xs"
					placeholder={'{"name": "x265", "includeCustomFormatWhenRenaming": false, "specifications": [...]}'}
					bind:value={jsonText}
				/>
			</div>

			{#if failure}
				<p role="alert" class="text-sm text-destructive">{failure}</p>
			{/if}

			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (open = false)}>Cancel</Button>
				<Button type="submit" disabled={importMutation.isPending || !jsonText.trim()}>
					{importMutation.isPending ? 'Importing…' : 'Import'}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
