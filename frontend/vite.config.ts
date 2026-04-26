import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  // Split heavy time-zero deps off the main entry chunk so the browser
  // can cache them independently of app code (their hashes only change
  // on dep bumps, not on every commit). Also silences Rollup's "chunk >
  // 500 kB" warning. See ISSUES.md #55. The dynamic-import code-splits
  // for @tauri-apps/api/{core,event,window} from #51 still fire — those
  // submodules are reachable only through `import()` calls so Rollup
  // emits them as separate chunks regardless of what we set here.
  build: {
    rollupOptions: {
      output: {
        manualChunks: {
          'vendor-react': ['react', 'react-dom', 'react/jsx-runtime'],
          'vendor-motion': ['framer-motion'],
        },
      },
    },
  },
  test: {
    globals: true,
    environment: 'jsdom',
    setupFiles: './src/setupTests.ts',
  },
});
