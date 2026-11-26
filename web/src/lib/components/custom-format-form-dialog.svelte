<script lang="ts">
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import { createMutation, useQueryClient } from '@tanstack/svelte-query';
	import { api } from '#lib/api/client.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { Checkbox } from '#lib/components/ui/checkbox/index.js';
	import * as Dialog from '#lib/components/ui/dialog/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import * as Select from '#lib/components/ui/select/index.js';
	import { Switch } from '#lib/components/ui/switch/index.js';
	import {
		createDraftSpec,
		defaultFieldsFor,
		draftOf,
		draftProblem,
		draftToPayload,
		fieldValue,
		optionLabel,
		problemMessage,
		setFieldValue,
		SPECIFICATION_TYPES,
		specTypeInfo,
		type CustomFormat,
		type CustomFormatDraft
	} from '#lib/custom-formats.js';

	let {
		open = $bindable(false),
		format
	}: { open: boolean; format: CustomFormat | null } = $props();

	let draft = $derived<CustomFormatDraft>(open ? draftOf(format) : draftOf(null));
	let failure = $derived(open ? '' : '');

	const queryClient = useQueryClient();

	const problem = $derived(draftProblem(draft));

	function addCondition() {
		draft.specifications = [...draft.specifications, createDraftSpec()];
	}

	function removeCondition(key: number) {
		draft.specifications = draft.specifications.filter((spec) => spec.key !== key);
	}

	function changeImplementation(key: number, implementation: string) {
		draft.specifications = draft.specifications.map((spec) =>
			spec.key === key
				? { ...spec, implementation, fields: defaultFieldsFor(implementation) }
				: spec
		);
	}

	function updateField(key: number, fieldName: string, value: unknown) {
		draft.specifications = draft.specifications.map((spec) =>
			spec.key === key ? { ...spec, fields: setFieldValue(spec.fields, fieldName, value) } : spec
		);
	}

	const save = createMutation(() => ({
		mutationFn: async () => {
			const body = draftToPayload(draft);
			const { error } = format
				? await api.PATCH('/api/v1/custom-formats/{id}', {
						params: { path: { id: format.id } },
						body
					})
				: await api.POST('/api/v1/custom-formats', { body });
			if (error) throw new Error(problemMessage(error));
		},
		onSuccess: async () => {
			await queryClient.invalidateQueries({ queryKey: ['custom-formats'] });
			open = false;
		},
		onError: (error) => {
			failure = error.message;
		}
	}));
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-2xl">
		<Dialog.Header>
			<Dialog.Title>{format ? `Edit ${format.name}` : 'New custom format'}</Dialog.Title>
			<Dialog.Description>
				Define conditions that match releases. Every required condition must match, and at least one
				condition per type group must match.
			</Dialog.Description>
		</Dialog.Header>

		<form
			class="grid gap-4"
			onsubmit={(event) => {
				event.preventDefault();
				if (!problem) save.mutate();
			}}
		>
			<div class="grid gap-2">
				<Label for="cf-name">Name</Label>
				<Input id="cf-name" required maxlength={120} bind:value={draft.name} />
			</div>

			<div class="flex items-center justify-between gap-4">
				<Label for="cf-rename">Include custom format when renaming</Label>
				<Switch id="cf-rename" bind:checked={draft.includeCustomFormatWhenRenaming} />
			</div>

			<fieldset class="grid gap-3">
				<div class="flex items-center justify-between">
					<legend class="text-sm font-medium">Conditions</legend>
					<Button type="button" variant="outline" size="sm" onclick={addCondition}>
						<PlusIcon class="size-4" />
						Add condition
					</Button>
				</div>

				{#if draft.specifications.length === 0}
					<p class="rounded-md border border-dashed p-4 text-center text-sm text-muted-foreground">
						No conditions yet. A format with no conditions matches every release.
					</p>
				{:else}
					<div class="grid gap-3">
						{#each draft.specifications as spec, index (spec.key)}
							{@const info = specTypeInfo(spec.implementation)}
							<div class="grid gap-3 rounded-md border p-3">
								<div class="flex items-start justify-between gap-2">
									<div class="grid flex-1 gap-3 sm:grid-cols-2">
										<div class="grid gap-1.5">
											<Label for="spec-name-{spec.key}">Condition name</Label>
											<Input
												id="spec-name-{spec.key}"
												placeholder="e.g. x265"
												bind:value={spec.name}
											/>
										</div>
										<div class="grid gap-1.5">
											<Label for="spec-type-{spec.key}">Type</Label>
											<Select.Root
												type="single"
												value={spec.implementation}
												onValueChange={(value) => changeImplementation(spec.key, value)}
											>
												<Select.Trigger id="spec-type-{spec.key}" class="w-full">
													{info.label}
												</Select.Trigger>
												<Select.Content>
													{#each SPECIFICATION_TYPES as typeOption (typeOption.implementation)}
														<Select.Item
															value={typeOption.implementation}
															label={typeOption.label}
														/>
													{/each}
												</Select.Content>
											</Select.Root>
										</div>
									</div>
									<Button
										type="button"
										variant="ghost"
										size="icon"
										class="mt-6 shrink-0 text-muted-foreground hover:text-destructive"
										aria-label="Remove condition {index + 1}"
										onclick={() => removeCondition(spec.key)}
									>
										<Trash2Icon class="size-4" />
									</Button>
								</div>

								{#if info.kind === 'regex'}
									<div class="grid gap-1.5">
										<Label for="spec-val-{spec.key}">Regular expression</Label>
										<Input
											id="spec-val-{spec.key}"
											class="font-mono text-xs"
											placeholder="e.g. (x|h)[ ._-]?265|hevc"
											value={String(fieldValue(spec.fields, 'value') ?? '')}
											oninput={(event) =>
												updateField(spec.key, 'value', event.currentTarget.value)}
										/>
									</div>
								{:else if info.kind === 'select' || info.kind === 'language'}
									<div class="grid gap-3 sm:grid-cols-2">
										<div class="grid gap-1.5">
											<Label for="spec-val-{spec.key}">{info.label}</Label>
											<Select.Root
												type="single"
												value={String(fieldValue(spec.fields, 'value') ?? '')}
												onValueChange={(value) => updateField(spec.key, 'value', Number(value))}
											>
												<Select.Trigger id="spec-val-{spec.key}" class="w-full">
													{optionLabel(info.options, fieldValue(spec.fields, 'value'))}
												</Select.Trigger>
												<Select.Content>
													{#each info.options ?? [] as opt (opt.value)}
														<Select.Item value={String(opt.value)} label={opt.label} />
													{/each}
												</Select.Content>
											</Select.Root>
										</div>
										{#if info.kind === 'language'}
											<div class="flex items-center gap-2 pt-6">
												<Checkbox
													id="spec-except-{spec.key}"
													checked={Boolean(fieldValue(spec.fields, 'exceptLanguage'))}
													onCheckedChange={(checked) =>
														updateField(spec.key, 'exceptLanguage', Boolean(checked))}
												/>
												<Label for="spec-except-{spec.key}">Except language</Label>
											</div>
										{/if}
									</div>
								{:else if info.kind === 'range-float' || info.kind === 'range-int'}
									<div class="grid gap-3 sm:grid-cols-2">
										<div class="grid gap-1.5">
											<Label for="spec-min-{spec.key}">
												Minimum{info.unit ? ` (${info.unit})` : ''}
											</Label>
											<Input
												id="spec-min-{spec.key}"
												type="number"
												step={info.kind === 'range-float' ? '0.1' : '1'}
												value={String(fieldValue(spec.fields, 'min') ?? 0)}
												oninput={(event) =>
													updateField(spec.key, 'min', Number(event.currentTarget.value))}
											/>
										</div>
										<div class="grid gap-1.5">
											<Label for="spec-max-{spec.key}">
												Maximum{info.unit ? ` (${info.unit})` : ''}
											</Label>
											<Input
												id="spec-max-{spec.key}"
												type="number"
												step={info.kind === 'range-float' ? '0.1' : '1'}
												value={String(fieldValue(spec.fields, 'max') ?? 0)}
												oninput={(event) =>
													updateField(spec.key, 'max', Number(event.currentTarget.value))}
											/>
										</div>
									</div>
								{/if}

								<div class="flex flex-wrap items-center gap-6 pt-1">
									<div class="flex items-center gap-2">
										<Switch id="spec-negate-{spec.key}" bind:checked={spec.negate} />
										<Label for="spec-negate-{spec.key}">Negate</Label>
									</div>
									<div class="flex items-center gap-2">
										<Switch id="spec-required-{spec.key}" bind:checked={spec.required} />
										<Label for="spec-required-{spec.key}">Required</Label>
									</div>
								</div>
							</div>
						{/each}
					</div>
				{/if}
			</fieldset>

			{#if failure || problem}
				<p role="alert" class="text-sm text-destructive">{failure || problem}</p>
			{/if}

			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (open = false)}>Cancel</Button>
				<Button type="submit" disabled={save.isPending || problem !== ''}>
					{save.isPending ? 'Saving…' : 'Save'}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
