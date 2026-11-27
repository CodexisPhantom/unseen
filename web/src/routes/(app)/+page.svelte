<script lang="ts">
	import { session } from '#lib/session.svelte.js';
</script>

<h1 class="text-2xl font-semibold">Unseenarr</h1>
{#if session.me && session.me.permissions.length === 0}
	<p class="mt-4 max-w-prose text-muted-foreground">
		Signed in as {session.me.user.name}. Your account has no role yet, so there is nothing to show.
		Ask the Owner or an administrator to assign you a role.
	</p>
{/if}
