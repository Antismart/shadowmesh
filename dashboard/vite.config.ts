/// <reference types="vitest" />
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  test: {
    globals: true,
    environment: 'jsdom',
    setupFiles: './src/test-setup.ts',
  },
  server: {
    port: 3000,
    proxy: {
      '/api': 'http://localhost:8081',
      '/health': 'http://localhost:8081',
      '/metrics': 'http://localhost:8081',
      '/ipfs': 'http://localhost:8081',
      '/signaling': {
        target: 'http://localhost:8081',
        ws: true,
      },
    },
  },
  build: {
    outDir: 'dist',
    sourcemap: false,
  },
});
