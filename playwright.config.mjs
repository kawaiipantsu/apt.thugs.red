import { defineConfig } from '@playwright/test';
export default defineConfig({
  testDir: './tests/ui',
  workers: 1,
  reporter: 'list',
  use: {
    baseURL: process.env.XXC_TEST_ORIGIN,
    headless: true,
    ignoreHTTPSErrors: process.env.XXC_TEST_SELF_SIGNED_PROXY === '1',
    launchOptions: { executablePath: process.env.CHROMIUM_PATH || undefined },
  },
});
