import { resolve } from 'node:path';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

// Tauri expects a fixed dev port, and the palette is a second window with its own page.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    target: 'es2022',
    // Every Lucide icon is bundled so modules can name any of them; the app loads from disk.
    chunkSizeWarningLimit: 1200,
    rollupOptions: {
      input: {
        main: resolve(import.meta.dirname, 'index.html'),
        palette: resolve(import.meta.dirname, 'palette.html'),
      },
    },
  },
  test: { include: ['test/**/*.test.ts'] },
});
