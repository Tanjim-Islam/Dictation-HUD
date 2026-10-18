import path from 'node:path';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react-swc';

export default defineConfig({
  plugins: [react()],
  resolve: { alias: { '@': path.resolve(__dirname, 'src') } },
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    // Rust build output and local tooling must not be watched (EBUSY crashes on Windows).
    watch: { ignored: ['**/src-tauri/**', '**/.scratch/**'] },
  },
  build: {
    sourcemap: true,
  },
});
