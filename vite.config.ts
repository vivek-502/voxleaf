import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import { fileURLToPath } from 'node:url';

/**
 * The desktop app is served by the Rust process itself: in a release build the
 * embedded server hands out the built bundle, and in dev Vite serves it and
 * proxies the same-origin API/media routes over to that server. Keeping one
 * origin is what lets the UI keep using plain `fetch('/api/...')`, multipart
 * XHR uploads with real progress, and `<audio src>` range streaming exactly as
 * the web app did.
 */
const API_PORT = Number(process.env.VOXLEAF_PORT ?? 7931);
const API_ORIGIN = `http://127.0.0.1:${API_PORT}`;

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  clearScreen: false,
  server: {
    // A fixed port on the IPv4 loopback, distinct from the default: dev
    // machines often run another Vite on 5173 (sometimes on `::1` only), and
    // the webview must reach this one deterministically.
    host: '127.0.0.1',
    port: 5177,
    strictPort: true,
    proxy: {
      '/api': { target: API_ORIGIN, changeOrigin: false },
    },
  },
  build: {
    target: 'es2022',
    sourcemap: false,
    chunkSizeWarningLimit: 1200,
  },
});
