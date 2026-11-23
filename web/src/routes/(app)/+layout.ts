import { redirect } from '@sveltejs/kit';
import { resolve } from '$app/paths';
import { api } from '#lib/api/client.js';
import type { LayoutLoad } from './$types';

// Until the Owner exists, the API answers 503 `setup_required`
// (docs/15-permissions.md, "First run").
export const load: LayoutLoad = async () => {
	const { data } = await api.GET('/api/v1/setup');
	if (data?.required) redirect(307, resolve('setup'));
};
