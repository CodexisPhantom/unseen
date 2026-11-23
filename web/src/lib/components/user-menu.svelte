<script lang="ts">
	import ChevronsUpDownIcon from '@lucide/svelte/icons/chevrons-up-down';
	import LogOutIcon from '@lucide/svelte/icons/log-out';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { authClient } from '#lib/auth-client.js';
	import * as DropdownMenu from '#lib/components/ui/dropdown-menu/index.js';
	import * as Sidebar from '#lib/components/ui/sidebar/index.js';
	import { session } from '#lib/session.svelte.js';

	const sidebar = Sidebar.useSidebar();

	async function signOut() {
		await authClient.signOut();
		session.me = null;
		await goto(resolve('login'), { invalidateAll: true });
	}
</script>

{#if session.me}
	<Sidebar.Menu>
		<Sidebar.MenuItem>
			<DropdownMenu.Root>
				<DropdownMenu.Trigger>
					{#snippet child({ props })}
						<Sidebar.MenuButton size="lg" {...props}>
							<span
								class="flex aspect-square size-8 items-center justify-center rounded-lg bg-sidebar-accent font-semibold uppercase"
							>
								{session.me?.user.name.charAt(0)}
							</span>
							<span class="grid flex-1 text-start text-sm leading-tight">
								<span class="truncate font-medium">{session.me?.user.name}</span>
								<span class="truncate text-xs text-muted-foreground">
									{session.me?.user.email}
								</span>
							</span>
							<ChevronsUpDownIcon class="ms-auto size-4" />
						</Sidebar.MenuButton>
					{/snippet}
				</DropdownMenu.Trigger>
				<DropdownMenu.Content side={sidebar.isMobile ? 'bottom' : 'right'} align="end">
					<DropdownMenu.Item onclick={signOut}>
						<LogOutIcon />
						Sign out
					</DropdownMenu.Item>
				</DropdownMenu.Content>
			</DropdownMenu.Root>
		</Sidebar.MenuItem>
	</Sidebar.Menu>
{/if}
