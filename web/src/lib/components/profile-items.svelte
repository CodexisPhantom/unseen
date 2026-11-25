<script lang="ts">
	import { tick } from 'svelte';
	import GripVerticalIcon from '@lucide/svelte/icons/grip-vertical';
	import { Badge } from '#lib/components/ui/badge/index.js';
	import { Checkbox } from '#lib/components/ui/checkbox/index.js';
	import {
		dropIndex,
		itemDetail,
		itemId,
		itemName,
		moveItem,
		setAllowed,
		type ProfileItem
	} from '#lib/profiles.js';

	let {
		items,
		cutoff,
		onchange
	}: { items: ProfileItem[]; cutoff: number | null; onchange: (items: ProfileItem[]) => void } =
		$props();

	let list = $state<HTMLOListElement | null>(null);
	let dragging = $state<number | null>(null);
	let announcement = $state('');

	function announce(index: number) {
		announcement = `${itemName(items[index])} moved to position ${index + 1} of ${items.length}.`;
	}

	/** Moving a row in the DOM can drop focus; put it back on the handle. */
	async function focusHandle(id: number) {
		await tick();
		list?.querySelector<HTMLElement>(`[data-handle="${id}"]`)?.focus();
	}

	function move(from: number, to: number) {
		const target = Math.min(Math.max(to, 0), items.length - 1);
		if (target === from) return;
		const id = itemId(items[from]);
		onchange(moveItem(items, from, target));
		announce(target);
		return focusHandle(id);
	}

	function onkeydown(event: KeyboardEvent, index: number) {
		const to = { ArrowUp: index - 1, ArrowDown: index + 1, Home: 0, End: items.length - 1 }[
			event.key
		];
		if (to === undefined) return;
		event.preventDefault();
		move(index, to);
	}

	function onpointerdown(event: PointerEvent, id: number) {
		if (event.button !== 0) return;
		event.preventDefault();
		dragging = id;
		// Window listeners, not pointer capture: the handle moves in the DOM while it drags.
		const onmove = (e: PointerEvent) => {
			const rows = [...(list?.querySelectorAll<HTMLElement>('[data-row]') ?? [])].filter(
				(row) => row.dataset.row !== String(id)
			);
			const centres = rows.map((row) => {
				const box = row.getBoundingClientRect();
				return box.top + box.height / 2;
			});
			const from = items.findIndex((item) => itemId(item) === id);
			const to = dropIndex(centres, e.clientY);
			if (from !== -1 && to !== from) {
				onchange(moveItem(items, from, to));
				announce(to);
			}
		};
		const stop = () => {
			dragging = null;
			window.removeEventListener('pointermove', onmove);
			window.removeEventListener('pointerup', stop);
			window.removeEventListener('pointercancel', stop);
			focusHandle(id);
		};
		window.addEventListener('pointermove', onmove);
		window.addEventListener('pointerup', stop);
		window.addEventListener('pointercancel', stop);
	}
</script>

<ol bind:this={list} class="grid gap-1" aria-label="Qualities, lowest first">
	{#each items as item, index (itemId(item))}
		{@const id = itemId(item)}
		<li
			data-row={id}
			class="flex items-center gap-3 rounded-md border bg-background px-2 py-1.5 {dragging === id
				? 'bg-accent shadow-md'
				: ''}"
		>
			<button
				type="button"
				data-handle={id}
				class="flex size-7 shrink-0 cursor-grab touch-none items-center justify-center rounded-md text-muted-foreground outline-none hover:bg-accent focus-visible:ring-3 focus-visible:ring-ring/50 {dragging ===
				id
					? 'cursor-grabbing'
					: ''}"
				aria-label="Reorder {itemName(item)}, position {index + 1} of {items.length}. Use the up and down arrow keys."
				onkeydown={(event) => onkeydown(event, index)}
				onpointerdown={(event) => onpointerdown(event, id)}
			>
				<GripVerticalIcon class="size-4" />
			</button>
			<Checkbox
				aria-label="Allow {itemName(item)}"
				checked={item.allowed}
				onCheckedChange={(checked) => onchange(setAllowed(items, index, checked === true))}
			/>
			<div class="min-w-0 flex-1">
				<div class="truncate text-sm font-medium">{itemName(item)}</div>
				{#if item.type === 'group'}
					<div class="truncate text-xs text-muted-foreground">{itemDetail(item)}</div>
				{/if}
			</div>
			{#if id === cutoff}<Badge variant="secondary">Cutoff</Badge>{/if}
		</li>
	{/each}
</ol>
<p class="sr-only" role="status" aria-live="polite">{announcement}</p>
