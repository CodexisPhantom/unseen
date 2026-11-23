import { redirect } from '@sveltejs/kit';
import { resolve } from '$app/paths';
import { api } from '#lib/api/client.js';
import type { PageLoad } from './$types';

// A signed-in user has nothing to do here; a fresh install needs setup first.
export const load: PageLoad = async () => {
	const { data: setup } = await api.GET('/api/v1/setup');
	if (setup?.required) redirect(307, resolve('setup'));
	const { response } = await api.GET('/api/v1/me');
	if (response.ok) redirect(307, resolve(''));
};
