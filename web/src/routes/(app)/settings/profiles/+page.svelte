<script lang="ts">
	import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import NoAccess from '#lib/components/no-access.svelte';
	import ProfileFormDialog from '#lib/components/profile-form-dialog.svelte';
	import * as AlertDialog from '#lib/components/ui/alert-dialog/index.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import * as Table from '#lib/components/ui/table/index.js';
	import {
		allowedSummary,
		cutoffName,
		formatScoreSummary,
		type QualityProfile
	} from '#lib/profiles.js';
	import { session } from '#lib/session.svelte.js';

	const read = $derived(session.can('profiles:read'));
	const manage = $derived(session.can('profiles:manage'));

	const profiles = createQuery(() => ({
		queryKey: ['quality-profiles'],
		enabled: read,
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/quality-profiles');
			if (!data) throw new Error(error?.detail ?? 'The profiles could not be loaded.');
			return data;
		}
	}));

	let formOpen = $state(false);
	let editing = $state<QualityProfile | null>(null);
	let deleting = $state<QualityProfile | null>(null);
	let failure = $state('');

	const queryClient = useQueryClient();
	const remove = createMutation(() => ({
		mutationFn: async (profile: QualityProfile) => {
			const { error } = await api.DELETE('/api/v1/quality-profiles/{id}', {
				params: { path: { id: profile.id } }
			});
			if (error) throw new Error(error.detail ?? error.title);
		},
		onSuccess: () => (failure = ''),
		onError: (error) => (failure = error.message),
		onSettled: () => queryClient.invalidateQueries({ queryKey: ['quality-profiles'] })
	}));
</script>

<svelte:head>
	<title>Quality profiles · Unseen</title>
</svelte:head>

{#if !read}
	<NoAccess anyOf={['profiles:read']} />
{:else}
	<div class="flex items-center justify-between">
		<h1 class="text-2xl font-semibold">Quality profiles</h1>
		{#if manage}
			<Button
				onclick={() => {
					editing = null;
					formOpen = true;
				}}
			>
				New profile
			</Button>
		{/if}
	</div>
	<p class="mt-1 text-sm text-muted-foreground">
		A profile lists the qualities a movie accepts, in order, and where upgrades stop.
	</p>

	{#if failure}
		<p role="alert" class="mt-4 text-sm text-destructive">{failure}</p>
	{/if}

	{#if profiles.isPending}
		<p class="mt-4 text-muted-foreground">Loading…</p>
	{:else if profiles.isError}
		<p class="mt-4 text-destructive">{profiles.error.message}</p>
	{:else if profiles.data.length === 0}
		<p class="mt-4 text-muted-foreground">There are no quality profiles.</p>
	{:else}
		<Table.Root class="mt-4">
			<Table.Header>
				<Table.Row>
					<Table.Head>Name</Table.Head>
					<Table.Head>Allowed</Table.Head>
					<Table.Head>Cutoff</Table.Head>
					<Table.Head>Upgrades</Table.Head>
					<Table.Head>Format scores</Table.Head>
					{#if manage}<Table.Head class="text-end">Actions</Table.Head>{/if}
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each profiles.data as profile (profile.id)}
					<Table.Row>
						<Table.Cell class="font-medium">{profile.name}</Table.Cell>
						<Table.Cell class="max-w-md truncate" title={allowedSummary(profile.items)}>
							{allowedSummary(profile.items)}
						</Table.Cell>
						<Table.Cell>{cutoffName(profile)}</Table.Cell>
						<Table.Cell>{profile.upgradeAllowed ? 'Yes' : 'No'}</Table.Cell>
						<Table.Cell class="text-muted-foreground">{formatScoreSummary(profile)}</Table.Cell>
						{#if manage}
							<Table.Cell class="space-x-2 text-end">
								<Button
									variant="outline"
									size="sm"
									onclick={() => {
										editing = profile;
										formOpen = true;
									}}
								>
									Edit
								</Button>
								<Button variant="outline" size="sm" onclick={() => (deleting = profile)}>
									Delete
								</Button>
							</Table.Cell>
						{/if}
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	{/if}

	{#if manage}
		<ProfileFormDialog
			bind:open={formOpen}
			profile={editing}
			availableFormats={profiles.data?.[0]?.formatScores ?? []}
		/>

		<AlertDialog.Root open={deleting !== null} onOpenChange={(open) => !open && (deleting = null)}>
			<AlertDialog.Content>
				<AlertDialog.Header>
					<AlertDialog.Title>Delete {deleting?.name}?</AlertDialog.Title>
					<AlertDialog.Description>
						A profile that a movie uses cannot be deleted. This cannot be undone.
					</AlertDialog.Description>
				</AlertDialog.Header>
				<AlertDialog.Footer>
					<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
					<AlertDialog.Action
						onclick={() => {
							if (deleting) remove.mutate(deleting);
							deleting = null;
						}}
					>
						Delete
					</AlertDialog.Action>
				</AlertDialog.Footer>
			</AlertDialog.Content>
		</AlertDialog.Root>
	{/if}
{/if}
