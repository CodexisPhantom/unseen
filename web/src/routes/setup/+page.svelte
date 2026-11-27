<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { defaults, superForm } from 'sveltekit-superforms';
	import { zod4 } from 'sveltekit-superforms/adapters';
	import { z } from 'zod';
	import { api } from '#lib/api/client.js';
	import Logo from '#lib/components/logo.svelte';
	import * as Card from '#lib/components/ui/card/index.js';
	import * as Form from '#lib/components/ui/form/index.js';
	import { Input } from '#lib/components/ui/input/index.js';

	// The server checks the same rules; the minimum length is docs/11-security.md.
	const schema = z.object({
		name: z.string().trim().min(1, 'Enter a name'),
		email: z.email('Enter a valid email address'),
		password: z.string().min(10, 'Use at least 10 characters')
	});

	let failure = $state('');

	const form = superForm(defaults(zod4(schema)), {
		SPA: true,
		validators: zod4(schema),
		async onUpdate({ form }) {
			if (!form.valid) return;
			failure = '';
			const { error, response } = await api.POST('/api/v1/setup', { body: form.data });
			// 201 signs the Owner in; 409 means someone finished setup first.
			if (response.status === 201 || response.status === 409) {
				await goto(resolve(''));
				return;
			}
			failure = error?.detail ?? error?.title ?? 'Setup failed. Try again.';
		}
	});
	const { form: formData, enhance, submitting } = form;
</script>

<svelte:head>
	<title>Setup · Unseenarr</title>
</svelte:head>

<main class="flex min-h-svh items-center justify-center p-4">
	<Card.Root class="w-full max-w-sm">
		<Card.Header>
			<Logo class="mb-2 size-10" />
			<Card.Title>Create the Owner account</Card.Title>
			<Card.Description>
				This account can do everything, including creating other users.
			</Card.Description>
		</Card.Header>
		<Card.Content>
			<form method="POST" use:enhance class="grid gap-4">
				<Form.Field {form} name="name">
					<Form.Control>
						{#snippet children({ props })}
							<Form.Label>Name</Form.Label>
							<Input {...props} autocomplete="name" bind:value={$formData.name} />
						{/snippet}
					</Form.Control>
					<Form.FieldErrors />
				</Form.Field>
				<Form.Field {form} name="email">
					<Form.Control>
						{#snippet children({ props })}
							<Form.Label>Email</Form.Label>
							<Input
								{...props}
								type="email"
								autocomplete="email"
								bind:value={$formData.email}
							/>
						{/snippet}
					</Form.Control>
					<Form.FieldErrors />
				</Form.Field>
				<Form.Field {form} name="password">
					<Form.Control>
						{#snippet children({ props })}
							<Form.Label>Password</Form.Label>
							<Input
								{...props}
								type="password"
								autocomplete="new-password"
								bind:value={$formData.password}
							/>
						{/snippet}
					</Form.Control>
					<Form.Description>At least 10 characters.</Form.Description>
					<Form.FieldErrors />
				</Form.Field>
				{#if failure}
					<p role="alert" class="text-sm text-destructive">{failure}</p>
				{/if}
				<Form.Button disabled={$submitting}>Create account</Form.Button>
			</form>
		</Card.Content>
	</Card.Root>
</main>
