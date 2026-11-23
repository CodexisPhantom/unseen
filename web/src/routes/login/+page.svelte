<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { defaults, superForm } from 'sveltekit-superforms';
	import { zod4 } from 'sveltekit-superforms/adapters';
	import { z } from 'zod';
	import { authClient } from '#lib/auth-client.js';
	import { safeRedirect, signInMessage } from '#lib/auth.js';
	import * as Card from '#lib/components/ui/card/index.js';
	import * as Form from '#lib/components/ui/form/index.js';
	import { Input } from '#lib/components/ui/input/index.js';

	const schema = z.object({
		email: z.email('Enter a valid email address'),
		password: z.string().min(1, 'Enter your password')
	});

	let failure = $state('');

	const form = superForm(defaults(zod4(schema)), {
		SPA: true,
		validators: zod4(schema),
		async onUpdate({ form }) {
			if (!form.valid) return;
			failure = '';
			const { error } = await authClient.signIn.email(form.data);
			if (error) {
				failure = signInMessage({ code: error.code, status: error.status });
				return;
			}
			// `redirectTo` is checked: only a path of this app leads on.
			// eslint-disable-next-line svelte/no-navigation-without-resolve
			await goto(safeRedirect(page.url.searchParams.get('redirectTo')), { invalidateAll: true });
		}
	});
	const { form: formData, enhance, submitting } = form;
</script>

<svelte:head>
	<title>Sign in · Unseen</title>
</svelte:head>

<main class="flex min-h-svh items-center justify-center p-4">
	<Card.Root class="w-full max-w-sm">
		<Card.Header>
			<Card.Title>Sign in to Unseen</Card.Title>
		</Card.Header>
		<Card.Content>
			<form method="POST" use:enhance class="grid gap-4">
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
								autocomplete="current-password"
								bind:value={$formData.password}
							/>
						{/snippet}
					</Form.Control>
					<Form.FieldErrors />
				</Form.Field>
				{#if failure}
					<p role="alert" class="text-sm text-destructive">{failure}</p>
				{/if}
				<Form.Button disabled={$submitting}>Sign in</Form.Button>
			</form>
		</Card.Content>
	</Card.Root>
</main>
