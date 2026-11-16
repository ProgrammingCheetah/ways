import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { svelteTesting } from '@testing-library/svelte/vite';

// The build is committed (dist/) and embedded in the zaf binary, so every file has a fixed
// name: the Rust side lists them (src/app/hub/dashboard/assets.rs) and a rebuild never renames one.
export default defineConfig({
  plugins: [svelte(), ...(process.env.VITEST ? [svelteTesting()] : [])],
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    cssCodeSplit: false,
    assetsInlineLimit: 0,
    rollupOptions: {
      output: {
        entryFileNames: 'assets/app.js',
        chunkFileNames: 'assets/[name].js',
        assetFileNames: 'assets/app[extname]',
      },
    },
  },
  // `npm run dev` serves the page and asks a running hub (zaf hub --port 7777) for the state.
  server: { proxy: { '/api': 'http://localhost:7777' } },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.js'],
  },
});
