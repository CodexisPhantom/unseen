<script lang="ts">
	import { asset } from '$app/paths';
	import { cn } from '#lib/utils.js';

	let { class: className }: { class?: string } = $props();
</script>

<!-- The ink of the mark follows the theme: dark ink on light, light ink on dark. -->
<img src={asset('unseenarr-mark.svg')} alt="" class={cn('dark:hidden', className)} />
<img src={asset('unseenarr-mark-light.svg')} alt="" class={cn('hidden dark:block', className)} />
