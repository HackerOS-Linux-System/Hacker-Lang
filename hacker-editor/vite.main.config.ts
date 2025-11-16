import { defineConfig } from 'vite';
import { join } from 'path';

export default defineConfig({
  resolve: {
    alias: {
      '@': join(__dirname, 'src'),
    },
  },
  build: {
    target: 'node20',
  },
});
