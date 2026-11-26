<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { defaults, superForm } from 'sveltekit-superforms';
	import { zod4 } from 'sveltekit-superforms/adapters';
	import { z } from 'zod';
	import { authClient } from '#lib/auth-client.js';
	import { needsTwoFactorChallenge } from '#lib/auth-security.js';
	import { safeRedirect, signInMessage } from '#lib/auth.js';
	import Logo from '#lib/components/logo.svelte';
	import * as Card from '#lib/components/ui/card/index.js';
	import * as Form from '#lib/components/ui/form/index.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import { Label } from '#lib/components/ui/label/index.js';

	const schema = z.object({
		email: z.email('Enter a valid email address'),
		password: z.string().min(1, 'Enter your password')
	});

	let failure = $state('');
	let twoFactorRequired = $state(false);
	let verificationCode = $state('');
	let passkeyPending = $state(false);

	async function continueSignedIn() {
		// `redirectTo` is checked: only a path of this app leads on.
		// eslint-disable-next-line svelte/no-navigation-without-resolve
		await goto(safeRedirect(page.url.searchParams.get('redirectTo')), { invalidateAll: true });
	}

	async function verifyTwoFactor() {
		failure = '';
		const { error } = await authClient.twoFactor.verifyTotp({ code: verificationCode.trim() });
		if (error) {
			failure = error.message || 'That code was not accepted. Check it and try again.';
			return;
		}
		await continueSignedIn();
	}

	async function signInWithPasskey() {
		failure = '';
		passkeyPending = true;
		try {
			const { data, error } = await authClient.signIn.passkey();
			if (error) {
				failure = error.message || 'Passkey sign-in failed. Try again or use your password.';
				return;
			}
			if (!data) {
				failure = 'Passkey sign-in did not complete. Try again or use your password.';
				return;
			}
			await continueSignedIn();
		} catch {
			failure = 'Passkey sign-in failed. Try again or use your password.';
		} finally {
			passkeyPending = false;
		}
	}

	const form = superForm(defaults(zod4(schema)), {
		SPA: true,
		validators: zod4(schema),
		async onUpdate({ form }) {
			if (!form.valid) return;
			failure = '';
			const { data, error } = await authClient.signIn.email(form.data);
			if (needsTwoFactorChallenge(data)) {
				twoFactorRequired = true;
				return;
			}
			if (error) {
				failure = signInMessage({ code: error.code, status: error.status });
				return;
			}
			await continueSignedIn();
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
			<Logo class="mb-2 size-10" />
			<Card.Title>Sign in to Unseen</Card.Title>
		</Card.Header>
		<Card.Content>
			{#if twoFactorRequired}
				<form
					class="grid gap-4"
					onsubmit={(event) => {
						event.preventDefault();
						verifyTwoFactor();
					}}
				>
					<p class="text-sm text-muted-foreground">
						Enter the current code from your authenticator app to finish signing in.
					</p>
					<div class="grid gap-2">
						<Label for="two-factor-code">Authenticator code</Label>
						<Input
							id="two-factor-code"
							autocomplete="one-time-code"
							inputmode="numeric"
							pattern="[0-9]*"
							maxlength={8}
							required
							bind:value={verificationCode}
						/>
					</div>
					{#if failure}
						<p role="alert" class="text-sm text-destructive">{failure}</p>
					{/if}
					<Button type="submit" disabled={!verificationCode.trim()}>Verify code</Button>
				</form>
			{:else}
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
				<div class="flex items-center gap-3 text-xs text-muted-foreground">
					<span class="h-px flex-1 bg-border"></span>
					<span>or</span>
					<span class="h-px flex-1 bg-border"></span>
				</div>
				<Button type="button" variant="outline" disabled={passkeyPending} onclick={signInWithPasskey}>
					{passkeyPending ? 'Waiting for passkey…' : 'Sign in with a passkey'}
				</Button>
			{/if}
		</Card.Content>
	</Card.Root>
</main>
