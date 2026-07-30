import { defineConfig } from 'vitest/config';

// Standalone vitest config so tests don't load vite.config.js (whose
// sveltekit plugin expects a full app build context). Only pure .ts
// modules under src/lib are unit-tested here.
export default defineConfig({
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
});
