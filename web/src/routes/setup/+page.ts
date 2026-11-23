import { redirect } from '@sveltejs/kit';
import { resolve } from '$app/paths';
import { api } from '#lib/api/client.js';
import type { PageLoad } from './$types';

// Setup is done once; afterwards the page leads into the app.
export const load: PageLoad = async () => {
	const { data } = await api.GET('/api/v1/setup');
	if (data && !data.required) redirect(307, resolve(''));
};
