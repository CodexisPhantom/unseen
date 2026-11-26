<script lang="ts">
	import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { authClient } from '#lib/auth-client.js';
	import { formatBackupCodes, totpSecretFromUri } from '#lib/auth-security.js';
	import { Button } from '#lib/components/ui/button/index.js';
	import { Input } from '#lib/components/ui/input/index.js';
	import { Label } from '#lib/components/ui/label/index.js';
	import { session } from '#lib/session.svelte.js';

	let password = $state('');
	let passkeyName = $state('My passkey');
	let failure = $state('');
	let setup = $state<{ secret: string; backupCodes: string[]; verified: boolean } | null>(null);
	let verificationCode = $state('');
	let verificationPending = $state(false);
	let copied = $state(false);
	const queryClient = useQueryClient();

	const passkeys = createQuery(() => ({
		queryKey: ['passkeys'],
		queryFn: async () => {
			const { data, error } = await authClient.passkey.listUserPasskeys();
			if (!data) throw new Error(error?.message ?? 'Passkeys could not be loaded.');
			return data;
		}
	}));

	const enableTwoFactor = createMutation(() => ({
		mutationFn: async () => {
			const { data, error } = await authClient.twoFactor.enable({
				password,
				method: 'totp',
				issuer: 'Unseen'
			});
			if (!data) throw new Error(error?.message ?? 'Two-factor authentication could not be enabled.');
			return data;
		},
		onSuccess: (data) => {
			if (data.method !== 'totp') {
				failure = 'The authenticator setup returned an unexpected response.';
				return;
			}
			setup = {
				secret: totpSecretFromUri(data.totpURI),
				backupCodes: data.backupCodes,
				verified: false
			};
			password = '';
		},
		onError: (error) => (failure = error.message)
	}));

	async function verifySetup() {
		if (!setup) return;
		failure = '';
		verificationPending = true;
		try {
			const { error } = await authClient.twoFactor.verifyTotp({ code: verificationCode.trim() });
			if (error) {
				failure = error.message || 'That code was not accepted. Check it and try again.';
				return;
			}
			setup = { ...setup, verified: true };
			verificationCode = '';
			if (session.me) {
				session.me = {
					...session.me,
					user: { ...session.me.user, twoFactorEnabled: true }
				};
			}
		} catch {
			failure = 'That code was not accepted. Check it and try again.';
		} finally {
			verificationPending = false;
		}
	}

	const disableTwoFactor = createMutation(() => ({
		mutationFn: async () => {
			const { error } = await authClient.twoFactor.disable({ password });
			if (error) throw new Error(error.message ?? 'Two-factor authentication could not be disabled.');
		},
		onSuccess: () => {
			password = '';
			setup = null;
			if (session.me) {
				session.me = {
					...session.me,
					user: { ...session.me.user, twoFactorEnabled: false }
				};
			}
		},
		onError: (error) => (failure = error.message)
	}));

	const addPasskey = createMutation(() => ({
		mutationFn: async () => {
			const { data, error } = await authClient.passkey.addPasskey({ name: passkeyName.trim() });
			if (!data) throw new Error(error?.message ?? 'The passkey could not be registered.');
		},
		onSuccess: async () => {
			failure = '';
			await queryClient.invalidateQueries({ queryKey: ['passkeys'] });
		},
		onError: (error) => (failure = error.message)
	}));

	const removePasskey = createMutation(() => ({
		mutationFn: async (id: string) => {
			const { error } = await authClient.passkey.deletePasskey({ id });
			if (error) throw new Error(error.message ?? 'The passkey could not be removed.');
		},
		onSuccess: async () => {
			failure = '';
			await queryClient.invalidateQueries({ queryKey: ['passkeys'] });
		},
		onError: (error) => (failure = error.message)
	}));

	async function copyBackupCodes() {
		if (!setup) return;
		try {
			await navigator.clipboard.writeText(formatBackupCodes(setup.backupCodes));
			copied = true;
		} catch {
			failure = 'The recovery codes could not be copied. Select them and copy them by hand.';
		}
	}
</script>

<section aria-labelledby="security-heading" class="grid gap-4">
	<div>
		<h2 id="security-heading" class="text-lg font-semibold">Security</h2>
		<p class="mt-1 text-sm text-muted-foreground">
			Protect your account with an authenticator app or a passkey.
		</p>
	</div>

	<div class="grid gap-3 rounded-lg border p-4">
		<div>
			<h3 class="font-medium">Authenticator app</h3>
			<p class="text-sm text-muted-foreground">
				{session.me?.user.twoFactorEnabled
					? 'Two-factor authentication is on.'
					: 'Add a time-based code when you sign in.'}
			</p>
		</div>

		{#if setup}
			<div class="grid gap-3">
				<p class="text-sm">
					Enter this setup key in your authenticator app.
				</p>
				<code class="rounded-md border bg-muted p-3 font-mono text-sm break-all select-all">{setup.secret}</code>
				{#if setup.verified}
					<p class="text-sm">
						Save the recovery codes somewhere safe. Each code works once.
					</p>
					<div class="grid gap-2">
						<div class="flex items-center justify-between gap-2">
							<Label for="backup-codes">Recovery codes</Label>
							<Button type="button" variant="outline" size="sm" onclick={copyBackupCodes}>
								{copied ? 'Copied' : 'Copy codes'}
							</Button>
						</div>
						<textarea
							id="backup-codes"
							readonly
							class="min-h-28 w-full rounded-md border bg-muted p-3 font-mono text-sm focus-visible:ring-2 focus-visible:ring-ring"
							>{formatBackupCodes(setup.backupCodes)}</textarea
						>
					</div>
					<Button type="button" onclick={() => (setup = null)}>Done</Button>
				{:else}
					<form
						class="flex flex-wrap items-end gap-3"
						onsubmit={(event) => {
							event.preventDefault();
							verifySetup();
						}}
					>
						<div class="grid gap-2">
							<Label for="setup-verification-code">Authenticator code</Label>
							<Input
								id="setup-verification-code"
								autocomplete="one-time-code"
								inputmode="numeric"
								pattern="[0-9]{6}"
								maxlength={6}
								required
								bind:value={verificationCode}
							/>
						</div>
						<Button type="submit" disabled={verificationCode.trim().length !== 6 || verificationPending}>
							{verificationPending ? 'Verifying…' : 'Verify code'}
						</Button>
					</form>
				{/if}
				{#if failure}
					<p role="alert" class="text-sm text-destructive">{failure}</p>
				{/if}
				</div>
		{:else if session.me?.user.twoFactorEnabled}
			<form
				class="flex flex-wrap items-end gap-3"
				onsubmit={(event) => {
					event.preventDefault();
					failure = '';
					disableTwoFactor.mutate();
				}}
			>
				<div class="grid gap-2">
					<Label for="disable-two-factor-password">Password to turn off</Label>
					<Input id="disable-two-factor-password" type="password" autocomplete="current-password" required bind:value={password} />
				</div>
				<Button type="submit" variant="outline" disabled={disableTwoFactor.isPending}>
					Turn off
				</Button>
			</form>
		{:else}
			<form
				class="flex flex-wrap items-end gap-3"
				onsubmit={(event) => {
					event.preventDefault();
					failure = '';
					enableTwoFactor.mutate();
				}}
			>
				<div class="grid gap-2">
					<Label for="enable-two-factor-password">Confirm with your password</Label>
					<Input id="enable-two-factor-password" type="password" autocomplete="current-password" required bind:value={password} />
				</div>
				<Button type="submit" disabled={enableTwoFactor.isPending}>
					{enableTwoFactor.isPending ? 'Setting up…' : 'Set up authenticator'}
				</Button>
			</form>
		{/if}
	</div>

	<div class="grid gap-3 rounded-lg border p-4">
		<div>
			<h3 class="font-medium">Passkeys</h3>
			<p class="text-sm text-muted-foreground">
				Sign in with a device, security key, or password manager.
			</p>
		</div>
		<form
			class="flex flex-wrap items-end gap-3"
			onsubmit={(event) => {
				event.preventDefault();
				failure = '';
				addPasskey.mutate();
			}}
		>
			<div class="grid gap-2">
				<Label for="passkey-name">Passkey name</Label>
			<Input id="passkey-name" maxlength={64} required bind:value={passkeyName} />
			</div>
			<Button type="submit" disabled={addPasskey.isPending}>
				{addPasskey.isPending ? 'Waiting for device…' : 'Add passkey'}
			</Button>
		</form>

		{#if passkeys.isPending}
			<p class="text-sm text-muted-foreground">Loading passkeys…</p>
		{:else if passkeys.isError}
			<p class="text-sm text-destructive">{passkeys.error.message}</p>
		{:else if passkeys.data.length === 0}
			<p class="text-sm text-muted-foreground">You have no passkeys yet.</p>
		{:else}
			<ul class="grid gap-2">
				{#each passkeys.data as passkey (passkey.id)}
					<li class="flex items-center justify-between gap-3 border-t pt-3">
						<span class="min-w-0 truncate text-sm font-medium">{passkey.name || 'Passkey'}</span>
						<Button
							type="button"
							variant="outline"
							size="sm"
							disabled={removePasskey.isPending}
							onclick={() => removePasskey.mutate(passkey.id)}
						>
							Remove
						</Button>
					</li>
				{/each}
			</ul>
		{/if}
	</div>

	{#if failure && !setup}
		<p role="alert" class="text-sm text-destructive">{failure}</p>
	{/if}
</section>
