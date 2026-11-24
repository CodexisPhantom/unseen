<script lang="ts">
	import { Checkbox } from '#lib/components/ui/checkbox/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import { groupPermissions } from '#lib/roles.js';
	import { session, type Permission } from '#lib/session.svelte.js';

	let {
		catalog,
		selected = $bindable()
	}: { catalog: Permission[]; selected: Permission[] } = $props();

	const groups = $derived(groupPermissions(catalog));

	function toggle(permission: Permission, on: boolean) {
		selected = on ? [...selected, permission] : selected.filter((entry) => entry !== permission);
	}
</script>

<div class="grid gap-3">
	{#each groups as group (group.resource)}
		<div class="grid gap-2">
			<span class="text-xs font-medium text-muted-foreground uppercase">{group.resource}</span>
			<div class="grid gap-2 sm:grid-cols-2">
				{#each group.permissions as permission (permission)}
					{@const held = session.can(permission)}
					<Label class="gap-2 font-normal {held ? '' : 'opacity-50'}">
						<Checkbox
							disabled={!held}
							checked={selected.includes(permission)}
							onCheckedChange={(on) => toggle(permission, on === true)}
						/>
						<span class="font-mono text-xs">{permission}</span>
					</Label>
				{/each}
			</div>
		</div>
	{/each}
	<p class="text-xs text-muted-foreground">
		Greyed permissions are ones you do not hold, so you cannot give them away.
	</p>
</div>
