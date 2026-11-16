import { defineConfig } from 'vitest/config';
import tailwindcss from '@tailwindcss/vite';
import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';

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
	],
	test: {
		environment: 'node',
		include: ['src/**/*.{test,spec}.{js,ts}'],
		// No store or helper exists yet (M1 adds the first).
		passWithNoTests: true,
		expect: { requireAssertions: true },
		coverage: {
			// Minimum from docs/12-testing.md. Not counted: vendored shadcn-svelte
			// code, generated types, and modules that only create an instance.
			include: ['src/lib/**/*.ts'],
			exclude: [
				'src/lib/components/ui/**',
				'src/lib/hooks/is-mobile.svelte.ts',
				'src/lib/utils.ts',
				'src/lib/index.ts',
				'src/lib/api/schema.d.ts',
				'src/lib/api/client.ts'
			],
			thresholds: { lines: 80 }
		}
	}
});
