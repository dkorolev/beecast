'use strict';

const { defineConfig } = require('@playwright/test');

// The suite tests THE artifact: global-setup builds the fixture page with the real CLI,
// and every test loads that file — over file:// directly and over a throwaway HTTP
// server. No dev server, no bundler, no mock DOM.
module.exports = defineConfig({
  testDir: './tests',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: 'list',
  globalSetup: require.resolve('./global-setup'),
  use: {
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  projects: [
    { name: 'chromium', use: { browserName: 'chromium' } },
    { name: 'firefox', use: { browserName: 'firefox' } },
    { name: 'webkit', use: { browserName: 'webkit' } },
  ],
});
