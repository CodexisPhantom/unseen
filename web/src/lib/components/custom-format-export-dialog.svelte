<script lang="ts">
	import CheckIcon from '@lucide/svelte/icons/check';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import DownloadIcon from '@lucide/svelte/icons/download';
	import { Button } from '#lib/components/ui/button/index.js';
	import * as Dialog from '#lib/components/ui/dialog/index.js';
	import { Textarea } from '#lib/components/ui/textarea/index.js';

	let {
		open = $bindable(false),
		title,
		filename,
		jsonText
	}: { open: boolean; title: string; filename: string; jsonText: string } = $props();

	let copied = $derived(open ? false : false);

	async function copyJson() {
		await navigator.clipboard.writeText(jsonText);
		copied = true;
		setTimeout(() => {
			copied = false;
		}, 2000);
	}

	function downloadJson() {
		const blob = new Blob([jsonText], { type: 'application/json' });
		const url = URL.createObjectURL(blob);
		const link = document.createElement('a');
		link.href = url;
		link.download = filename;
		link.click();
		URL.revokeObjectURL(url);
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-2xl">
		<Dialog.Header>
			<Dialog.Title>{title}</Dialog.Title>
			<Dialog.Description>
				Radarr-compatible JSON without internal ids. Copy or download to reuse in Radarr or another
				Unseen instance.
			</Dialog.Description>
		</Dialog.Header>

		<Textarea
			readonly
			rows={14}
			class="font-mono text-xs"
			value={jsonText}
			aria-label="Exported custom format JSON"
		/>

		<Dialog.Footer>
			<Button type="button" variant="outline" onclick={copyJson}>
				{#if copied}
					<CheckIcon class="size-4" />
					Copied
				{:else}
					<CopyIcon class="size-4" />
					Copy JSON
				{/if}
			</Button>
			<Button type="button" onclick={downloadJson}>
				<DownloadIcon class="size-4" />
				Download
			</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
