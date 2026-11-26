import createClient from 'openapi-fetch';
import type { paths } from './schema.js';
import { basePath } from '../base-path.js';

// The one API client. Components never call `fetch` directly (docs/16-frontend.md).
// The base path is injected at serve time for sub-path hosting.
export const api = createClient<paths>({ baseUrl: basePath() });
