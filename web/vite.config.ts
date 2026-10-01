import { defineConfig } from 'vitest/config';

export default defineConfig({
  base: '/openapi-aggregator/',
  build: { target: 'es2022' },
  test: { include: ['tests/**/*.test.ts'], environment: 'node' },
});
