import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  server: {
    // Dev server proxies API calls to a locally running `cargo run -p dxca-server`.
    proxy: {
      '/api': 'http://127.0.0.1:7580',
    },
  },
  build: {
    // Embedded into the server binary by include_dir (crates/dxca-server/src/assets.rs).
    outDir: 'dist',
    emptyOutDir: true,
  },
});
