import tailwindcss from '@tailwindcss/vite';
import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

export default defineConfig({
	// In development the API runs on its own port (`just dev`).
	server: { proxy: { '/api': 'http://localhost:7979' } },
	plugins: [
		tailwindcss(),
		sveltekit({
			compilerOptions: {
				// Force runes mode for the project, except for libraries. Can be removed in svelte 6.
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},

			// Static SPA: the Rust binary serves the build folder and falls back to index.html.
			adapter: adapter({ fallback: 'index.html' })
		})
	]
});
