import { error, redirect } from '@sveltejs/kit';
import { resolve } from '$app/paths';
import { api } from '#lib/api/client.js';
import { session } from '#lib/session.svelte.js';
import type { LayoutLoad } from './$types';

// Until the Owner exists, the API answers 503 `setup_required`
// (docs/15-permissions.md, "First run"). Then every page needs a session.
export const load: LayoutLoad = async ({ url }) => {
	const { data: setup } = await api.GET('/api/v1/setup');
	if (setup?.required) redirect(307, resolve('setup'));

	const { data: me, response } = await api.GET('/api/v1/me');
	if (response.status === 401) {
		const back = encodeURIComponent(url.pathname + url.search);
		redirect(307, `${resolve('login')}?redirectTo=${back}`);
	}
	if (!me) error(response.status, 'The current user could not be loaded.');
	session.me = me;
};
