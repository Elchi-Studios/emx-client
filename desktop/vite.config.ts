import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

// Tauri sets TAURI_DEV_HOST when a phone or another machine runs the app.
const host = (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [sveltekit()],
  // Tauri runs the dev server itself and expects a fixed port.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: { ignored: ['**/src-tauri/**'] }
  }
});
