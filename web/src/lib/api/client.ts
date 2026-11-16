import createClient from 'openapi-fetch';
import type { paths } from './schema.js';

// The one API client. Components never call `fetch` directly (docs/16-frontend.md).
export const api = createClient<paths>();
