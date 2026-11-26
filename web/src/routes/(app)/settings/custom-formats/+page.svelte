<script lang="ts">
	import DownloadIcon from '@lucide/svelte/icons/download';
	import FlaskConicalIcon from '@lucide/svelte/icons/flask-conical';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import UploadIcon from '@lucide/svelte/icons/upload';
	import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import CustomFormatExportDialog from '#lib/components/custom-format-export-dialog.svelte';
	import CustomFormatFormDialog from '#lib/components/custom-format-form-dialog.svelte';
	import CustomFormatImportDialog from '#lib/components/custom-format-import-dialog.svelte';
	import NoAccess from '#lib/components/no-access.svelte';
	import * as AlertDialog from '#lib/components/ui/alert-dialog/index.js';
	import { Badge } from '#lib/components/ui/badge/index.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import * as Card from '#lib/components/ui/card/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import * as Table from '#lib/components/ui/table/index.js';
	import {
		modifierName,
		problemMessage,
		sourceName,
		specSummary,
		type CustomFormat,
		type CustomFormatTestResult
	} from '#lib/custom-formats.js';
	import { session } from '#lib/session.svelte.js';

	const read = $derived(session.can('profiles:read'));
	const manage = $derived(session.can('profiles:manage'));

	const formats = createQuery(() => ({
		queryKey: ['custom-formats'],
		enabled: read,
		queryFn: async () => {
			const { data, error } = await api.GET('/api/v1/custom-formats');
			if (!data) throw new Error(error?.detail ?? 'Custom formats could not be loaded.');
			return data;
		}
	}));

	let formOpen = $state(false);
	let importOpen = $state(false);
	let exportOpen = $state(false);
	let exportTitle = $state('');
	let exportFilename = $state('custom-formats.json');
	let exportJson = $state('');

	let editing = $state<CustomFormat | null>(null);
	let deleting = $state<CustomFormat | null>(null);
	let failure = $state('');

	// Tester state
	let releaseName = $state('');
	let sizeGib = $state('');
	let movieYear = $state('');
	let testResult = $state.raw<CustomFormatTestResult | null>(null);
	let testError = $state('');

	const queryClient = useQueryClient();

	const remove = createMutation(() => ({
		mutationFn: async (format: CustomFormat) => {
			const { error } = await api.DELETE('/api/v1/custom-formats/{id}', {
				params: { path: { id: format.id } }
			});
			if (error) throw new Error(problemMessage(error));
		},
		onSuccess: () => {
			failure = '';
		},
		onError: (error) => {
			failure = error.message;
		},
		onSettled: () => queryClient.invalidateQueries({ queryKey: ['custom-formats'] })
	}));

	const exportAll = createMutation(() => ({
		mutationFn: async () => {
			const { data, error } = await api.GET('/api/v1/custom-formats/export');
			if (!data) throw new Error(error ? problemMessage(error) : 'Export failed.');
			return data;
		},
		onSuccess: (data) => {
			failure = '';
			exportTitle = 'Export all custom formats';
			exportFilename = 'custom-formats.json';
			exportJson = JSON.stringify(data, null, 2);
			exportOpen = true;
		},
		onError: (error) => {
			failure = error.message;
		}
	}));

	const exportOne = createMutation(() => ({
		mutationFn: async (format: CustomFormat) => {
			const { data, error } = await api.GET('/api/v1/custom-formats/{id}/export', {
				params: { path: { id: format.id } }
			});
			if (!data) throw new Error(error ? problemMessage(error) : 'Export failed.');
			return { format, data };
		},
		onSuccess: ({ format, data }) => {
			failure = '';
			exportTitle = `Export ${format.name}`;
			const slug = format.name
				.toLowerCase()
				.replace(/[^a-z0-9]+/g, '-')
				.replace(/^-|-$/g, '');
			exportFilename = `${slug || 'custom-format'}.json`;
			exportJson = JSON.stringify(data, null, 2);
			exportOpen = true;
		},
		onError: (error) => {
			failure = error.message;
		}
	}));

	const runTest = createMutation(() => ({
		mutationFn: async () => {
			const parsedSize = sizeGib.trim() ? Math.round(Number(sizeGib) * 1024 ** 3) : undefined;
			const parsedYear = movieYear.trim() ? Number(movieYear) : undefined;
			const { data, error } = await api.POST('/api/v1/custom-formats/test', {
				body: {
					releaseName: releaseName.trim(),
					sizeBytes: parsedSize,
					movieYear: parsedYear
				}
			});
			if (!data) throw new Error(error ? problemMessage(error) : 'Test failed.');
			return data;
		},
		onSuccess: (data) => {
			testError = '';
			testResult = data;
		},
		onError: (error) => {
			testError = error.message;
		}
	}));
</script>

<svelte:head>
	<title>Custom formats · Unseen</title>
</svelte:head>

{#if !read}
	<NoAccess anyOf={['profiles:read']} />
{:else}
	<div class="flex flex-wrap items-center justify-between gap-3">
		<div>
			<h1 class="text-2xl font-semibold">Custom formats</h1>
			<p class="mt-1 text-sm text-muted-foreground">
				Match release names by regex, resolution, source, language, size, or year to adjust
				profile scores.
			</p>
		</div>
		<div class="flex flex-wrap items-center gap-2">
			<Button
				variant="outline"
				disabled={exportAll.isPending || !formats.data?.length}
				onclick={() => exportAll.mutate()}
			>
				<DownloadIcon class="size-4" />
				Export all
			</Button>
			{#if manage}
				<Button variant="outline" onclick={() => (importOpen = true)}>
					<UploadIcon class="size-4" />
					Import JSON
				</Button>
				<Button
					onclick={() => {
						editing = null;
						formOpen = true;
					}}
				>
					<PlusIcon class="size-4" />
					New format
				</Button>
			{/if}
		</div>
	</div>

	{#if failure}
		<p role="alert" class="mt-4 text-sm text-destructive">{failure}</p>
	{/if}

	{#if formats.isPending}
		<p class="mt-6 text-sm text-muted-foreground">Loading…</p>
	{:else if formats.isError}
		<p role="alert" class="mt-6 text-sm text-destructive">{formats.error.message}</p>
	{:else if formats.data.length === 0}
		<div class="mt-6 rounded-md border border-dashed p-8 text-center">
			<p class="text-sm text-muted-foreground">No custom formats yet.</p>
			{#if manage}
				<div class="mt-4 flex justify-center gap-2">
					<Button variant="outline" onclick={() => (importOpen = true)}>Import Radarr JSON</Button>
					<Button
						onclick={() => {
							editing = null;
							formOpen = true;
						}}
					>
						Create format
					</Button>
				</div>
			{/if}
		</div>
	{:else}
		<Table.Root class="mt-4">
			<Table.Header>
				<Table.Row>
					<Table.Head>Name</Table.Head>
					<Table.Head>Conditions</Table.Head>
					<Table.Head>In rename</Table.Head>
					<Table.Head class="text-end">Actions</Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each formats.data as format (format.id)}
					<Table.Row>
						<Table.Cell class="font-medium">{format.name}</Table.Cell>
						<Table.Cell>
							<div class="flex flex-wrap gap-1.5">
								{#each format.specifications as spec, index (`${format.id}-${index}`)}
									<Badge variant={spec.required ? 'default' : 'secondary'}>
										{specSummary(spec)}
									</Badge>
								{/each}
								{#if format.specifications.length === 0}
									<span class="text-xs text-muted-foreground">Matches all releases</span>
								{/if}
							</div>
						</Table.Cell>
						<Table.Cell>{format.includeCustomFormatWhenRenaming ? 'Yes' : 'No'}</Table.Cell>
						<Table.Cell class="space-x-2 text-end">
							<Button
								variant="outline"
								size="sm"
								disabled={exportOne.isPending}
								onclick={() => exportOne.mutate(format)}
							>
								Export
							</Button>
							{#if manage}
								<Button
									variant="outline"
									size="sm"
									onclick={() => {
										editing = format;
										formOpen = true;
									}}
								>
									Edit
								</Button>
								<Button variant="outline" size="sm" onclick={() => (deleting = format)}>
									Delete
								</Button>
							{/if}
						</Table.Cell>
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	{/if}

	<Card.Root class="mt-8">
		<Card.Header>
			<Card.Title class="flex items-center gap-2">
				<FlaskConicalIcon class="size-4" />
				Release tester
			</Card.Title>
			<Card.Description>
				Enter a release name to see which custom formats match and the resulting score for each
				quality profile.
			</Card.Description>
		</Card.Header>
		<Card.Content>
			<form
				class="grid gap-4"
				onsubmit={(event) => {
					event.preventDefault();
					if (releaseName.trim()) runTest.mutate();
				}}
			>
				<div class="grid gap-3 sm:grid-cols-12">
					<div class="grid gap-1.5 sm:col-span-8">
						<Label for="test-release-name">Release name</Label>
						<Input
							id="test-release-name"
							class="font-mono text-xs"
							placeholder="The.Matrix.1999.2160p.UHD.BluRay.REMUX.HDR.HEVC.TrueHD.7.1.Atmos-FGT"
							bind:value={releaseName}
						/>
					</div>
					<div class="grid gap-1.5 sm:col-span-2">
						<Label for="test-size">Size (GiB, optional)</Label>
						<Input
							id="test-size"
							type="number"
							min="0"
							step="0.1"
							placeholder="e.g. 15"
							bind:value={sizeGib}
						/>
					</div>
					<div class="grid gap-1.5 sm:col-span-2">
						<Label for="test-year">Movie year (optional)</Label>
						<Input
							id="test-year"
							type="number"
							min="1880"
							max="2100"
							placeholder="e.g. 1999"
							bind:value={movieYear}
						/>
					</div>
				</div>

				<div class="flex items-center gap-3">
					<Button type="submit" disabled={runTest.isPending || !releaseName.trim()}>
						{runTest.isPending ? 'Testing…' : 'Test release'}
					</Button>
					{#if testError}
						<p role="alert" class="text-sm text-destructive">{testError}</p>
					{/if}
				</div>
			</form>

			{#if testResult}
				<div class="mt-6 grid gap-6 border-t pt-6">
					<div>
						<h3 class="text-sm font-medium">Parsed release</h3>
						<div class="mt-2 flex flex-wrap gap-2 text-xs">
							<Badge variant="outline">Title: {testResult.parsed.title || '—'}</Badge>
							{#if testResult.parsed.year}
								<Badge variant="outline">Year: {testResult.parsed.year}</Badge>
							{/if}
							{#if testResult.parsed.resolution}
								<Badge variant="outline">Resolution: {testResult.parsed.resolution}p</Badge>
							{/if}
							<Badge variant="outline">Source: {sourceName(testResult.parsed.source)}</Badge>
							{#if testResult.parsed.qualityModifier !== 0}
								<Badge variant="outline">
									Modifier: {modifierName(testResult.parsed.qualityModifier)}
								</Badge>
							{/if}
							{#if testResult.parsed.releaseGroup}
								<Badge variant="outline">Group: {testResult.parsed.releaseGroup}</Badge>
							{/if}
							{#if testResult.parsed.edition}
								<Badge variant="outline">Edition: {testResult.parsed.edition}</Badge>
							{/if}
							{#if testResult.parsed.languages.length > 0}
								<Badge variant="outline">
									Languages: {testResult.parsed.languages.join(', ')}
								</Badge>
							{/if}
						</div>
					</div>

					<div>
						<h3 class="text-sm font-medium">
							Matched formats ({testResult.matchedFormats.length})
						</h3>
						{#if testResult.matchedFormats.length === 0}
							<p class="mt-2 text-sm text-muted-foreground">No custom formats matched.</p>
						{:else}
							<div class="mt-2 flex flex-wrap gap-2">
								{#each testResult.matchedFormats as matched (matched.id)}
									<Badge>{matched.name}</Badge>
								{/each}
							</div>
						{/if}
					</div>

					<div>
						<h3 class="text-sm font-medium">Score per quality profile</h3>
						<Table.Root class="mt-2">
							<Table.Header>
								<Table.Row>
									<Table.Head>Profile</Table.Head>
									<Table.Head>Total score</Table.Head>
									<Table.Head>Min / Cutoff</Table.Head>
									<Table.Head>Matched breakdown</Table.Head>
								</Table.Row>
							</Table.Header>
							<Table.Body>
								{#each testResult.profileScores as profile (profile.profileId)}
									<Table.Row>
										<Table.Cell class="font-medium">{profile.profileName}</Table.Cell>
										<Table.Cell class="font-mono font-semibold">
											{profile.totalScore > 0 ? `+${profile.totalScore}` : profile.totalScore}
										</Table.Cell>
										<Table.Cell class="font-mono text-xs text-muted-foreground">
											{profile.minFormatScore} / {profile.cutoffFormatScore}
										</Table.Cell>
										<Table.Cell>
											{#if profile.formatScores.length === 0}
												<span class="text-xs text-muted-foreground">—</span>
											{:else}
												<div class="flex flex-wrap gap-1.5">
													{#each profile.formatScores as entry (entry.formatId)}
														<Badge variant="secondary" class="font-mono text-xs">
															{entry.formatName}: {entry.score > 0
																? `+${entry.score}`
																: entry.score}
														</Badge>
													{/each}
												</div>
											{/if}
										</Table.Cell>
									</Table.Row>
								{/each}
							</Table.Body>
						</Table.Root>
					</div>
				</div>
			{/if}
		</Card.Content>
	</Card.Root>

	<CustomFormatExportDialog
		bind:open={exportOpen}
		title={exportTitle}
		filename={exportFilename}
		jsonText={exportJson}
	/>

	{#if manage}
		<CustomFormatFormDialog bind:open={formOpen} format={editing} />
		<CustomFormatImportDialog bind:open={importOpen} />

		<AlertDialog.Root open={deleting !== null} onOpenChange={(open) => !open && (deleting = null)}>
			<AlertDialog.Content>
				<AlertDialog.Header>
					<AlertDialog.Title>Delete {deleting?.name}?</AlertDialog.Title>
					<AlertDialog.Description>
						Deleting this custom format also removes its scores from every quality profile. This
						cannot be undone.
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
