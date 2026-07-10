'use strict';

const { test, expect } = require('@playwright/test');
const { fileUrl, startServer, collectConsoleViolations, exercisePlayer } = require('./helpers');

// The two hard gates, each asserted over both required transports: the generated page
// must work from a plain double-clicked file AND from a web server, making zero network
// requests after the initial load and emitting zero console errors or warnings.

for (const transport of ['file', 'http']) {
  test.describe(`over ${transport}://`, () => {
    let server = null;
    let url;

    test.beforeAll(async () => {
      if (transport === 'http') {
        server = await startServer();
        url = server.url;
      } else {
        url = fileUrl();
      }
    });

    test.afterAll(async () => {
      if (server) await server.close();
    });

    test('zero console errors or warnings through a full session', async ({ page }) => {
      const violations = collectConsoleViolations(page);
      await page.goto(url);
      await exercisePlayer(page);
      expect(violations).toEqual([]);
    });

    test('zero network requests after the initial load', async ({ page, context }) => {
      await page.goto(url);
      await page.locator('.sp-play').waitFor({ state: 'visible' });

      // From here on, record and abort EVERYTHING. The page already has all it needs.
      const attempted = [];
      await context.route('**/*', (route) => {
        attempted.push(`${route.request().method()} ${route.request().url()}`);
        return route.abort();
      });

      await exercisePlayer(page);
      expect(attempted).toEqual([]);
    });
  });
}
