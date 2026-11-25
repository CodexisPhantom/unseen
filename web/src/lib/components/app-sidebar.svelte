<script lang="ts">
	import ActivityIcon from '@lucide/svelte/icons/activity';
	import ImportIcon from '@lucide/svelte/icons/folder-input';
	import LibraryIcon from '@lucide/svelte/icons/library';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import RulerIcon from '@lucide/svelte/icons/ruler';
	import SearchIcon from '@lucide/svelte/icons/search';
	import ShieldIcon from '@lucide/svelte/icons/shield';
	import UsersIcon from '@lucide/svelte/icons/users';
	import SlidersIcon from '@lucide/svelte/icons/sliders-horizontal';
	import ServerIcon from '@lucide/svelte/icons/server';
	import SettingsIcon from '@lucide/svelte/icons/settings';
	import { afterNavigate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import Logo from '#lib/components/logo.svelte';
	import * as Sidebar from '#lib/components/ui/sidebar/index.js';
	import UserMenu from '#lib/components/user-menu.svelte';
	import { navEntries } from '#lib/nav.js';
	import { session } from '#lib/session.svelte.js';

	const icons = {
		library: LibraryIcon,
		add: PlusIcon,
		import: ImportIcon,
		activity: ActivityIcon,
		settings: SettingsIcon,
		'settings/profiles': SlidersIcon,
		'settings/quality-sizes': RulerIcon,
		'settings/indexers': SearchIcon,
		users: UsersIcon,
		roles: ShieldIcon,
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
							<Logo class="size-8" />
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
	<Sidebar.Footer>
		<UserMenu />
	</Sidebar.Footer>
	<Sidebar.Rail />
</Sidebar.Root>
