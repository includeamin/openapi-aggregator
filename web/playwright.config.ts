import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: 'e2e',
  use: { baseURL: 'http://localhost:4173/openapi-aggregator/' },
  webServer: {
    command: 'npm run build && npm run preview -- --port 4173 --strictPort',
    url: 'http://localhost:4173/openapi-aggregator/',
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});
