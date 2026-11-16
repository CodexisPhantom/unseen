// The app is a static SPA (adapter-static). Server-only SvelteKit files
// cannot work, so this check fails when one exists. See docs/16-frontend.md.
import { globSync } from 'node:fs';

const forbidden = globSync('src/**/{+page.server,+layout.server,+server,hooks.server}.{ts,js}');
if (forbidden.length) {
	console.error('Server-only files are not allowed in a static SPA:');
	for (const file of forbidden) console.error(`  ${file}`);
	process.exit(1);
}
