<script lang="ts">
	import ActivityIcon from '@lucide/svelte/icons/activity';
	import LibraryIcon from '@lucide/svelte/icons/library';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import ServerIcon from '@lucide/svelte/icons/server';
	import SettingsIcon from '@lucide/svelte/icons/settings';
	import { afterNavigate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import * as Sidebar from '#lib/components/ui/sidebar/index.js';
	import { navEntries } from '#lib/nav.js';
	import { session } from '#lib/session.svelte.js';

	const icons = {
		library: LibraryIcon,
		add: PlusIcon,
		activity: ActivityIcon,
		settings: SettingsIcon,
		system: ServerIcon
	} as const;

	// Presentation only: hidden entries still answer 403 from the API.
	const entries = $derived(
		navEntries
			.filter((entry) => session.canAny(entry.anyOf))
			.map((entry) => ({
				title: entry.title,
				href: resolve(entry.path as keyof typeof icons),
				icon: icons[entry.path as keyof typeof icons]
			}))
	);

	const sidebar = Sidebar.useSidebar();
	// On a phone the sidebar is a sheet; close it after a navigation.
	afterNavigate(() => sidebar.setOpenMobile(false));
</script>

<Sidebar.Root collapsible="icon">
	<Sidebar.Header>
		<Sidebar.Menu>
			<Sidebar.MenuItem>
				<Sidebar.MenuButton size="lg">
					{#snippet child({ props })}
						<a href={resolve('')} {...props}>
							<span
								class="flex aspect-square size-8 items-center justify-center rounded-lg bg-sidebar-primary font-semibold text-sidebar-primary-foreground"
							>
								U
							</span>
							<span class="font-semibold">Unseen</span>
						</a>
					{/snippet}
				</Sidebar.MenuButton>
			</Sidebar.MenuItem>
		</Sidebar.Menu>
	</Sidebar.Header>
	<Sidebar.Content>
		<Sidebar.Group>
			<Sidebar.GroupContent>
				<Sidebar.Menu>
					{#each entries as entry (entry.href)}
						<Sidebar.MenuItem>
							<Sidebar.MenuButton
								isActive={page.url.pathname.startsWith(entry.href)}
								tooltipContent={entry.title}
							>
								{#snippet child({ props })}
									<a href={entry.href} {...props}>
										<entry.icon />
										<span>{entry.title}</span>
									</a>
								{/snippet}
							</Sidebar.MenuButton>
						</Sidebar.MenuItem>
					{/each}
				</Sidebar.Menu>
			</Sidebar.GroupContent>
		</Sidebar.Group>
	</Sidebar.Content>
	<Sidebar.Rail />
</Sidebar.Root>
