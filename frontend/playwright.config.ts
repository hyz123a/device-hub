import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './e2e',
  timeout: 30_000,
  expect: {
    timeout: 5_000,
  },
  use: {
    baseURL: 'http://127.0.0.1:8090',
    trace: 'on-first-retry',
    video: process.env.PLAYWRIGHT_VIDEO === 'on' ? 'on' : 'retain-on-failure',
  },
  webServer: [
    {
      command: 'sh e2e/start-device-hub-server.sh',
      url: 'http://127.0.0.1:3100/api/devices',
      reuseExistingServer: false,
      timeout: 120_000,
    },
    {
      command: 'env -u FORCE_COLOR NO_COLOR=true trunk serve --config Trunk.e2e.toml',
      url: 'http://127.0.0.1:8090',
      reuseExistingServer: !process.env.CI,
      timeout: 120_000,
    },
  ],
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
});
