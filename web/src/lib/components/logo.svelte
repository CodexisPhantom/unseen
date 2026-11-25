<script lang="ts">
	import { asset } from '$app/paths';
	import { cn } from '#lib/utils.js';

	let { class: className }: { class?: string } = $props();
</script>

<!-- The ink of the mark follows the theme: dark ink on light, light ink on dark. -->
<img src={asset('unseen-mark.svg')} alt="" class={cn('dark:hidden', className)} />
<img src={asset('unseen-mark-light.svg')} alt="" class={cn('hidden dark:block', className)} />
