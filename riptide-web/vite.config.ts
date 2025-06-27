import { defineConfig } from 'vite';
import { resolve } from 'path';

export default defineConfig({
  root: 'src',
  build: {
    outDir: '../static',
    emptyOutDir: false, // Don't delete existing static files
    rollupOptions: {
      input: {
        app: resolve(__dirname, 'src/main.ts'),
        torrents: resolve(__dirname, 'src/torrents.ts'),
        library: resolve(__dirname, 'src/library.ts'),
        search: resolve(__dirname, 'src/search.ts'),
        settings: resolve(__dirname, 'src/settings.ts'),
      },
      output: {
        entryFileNames: 'js/[name].js',
        chunkFileNames: 'js/[name]-[hash].js',
        assetFileNames: (assetInfo) => {
          if (assetInfo.name?.endsWith('.css')) {
            return 'css/[name].[ext]';
          }
          return 'assets/[name]-[hash].[ext]';
        },
      },
    },
  },
  server: {
    port: 3001,
  },
});