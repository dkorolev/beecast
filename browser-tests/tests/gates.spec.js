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

test.describe('human-facing behavior', () => {
  test('chapter controls stay absent and c is a no-op without chapters', async ({ page }) => {
    await page.goto(fileUrl());
    await page.evaluate(() => {
      const mount = document.createElement('div');
      mount.id = 'empty-chapter-player';
      document.body.appendChild(mount);
      BeeCastPlayer.create({ data: '{"version":3,"term":{"cols":10,"rows":3}}\n[0.1,"o","hello"]\n' }, mount, {
        controls: true,
        markers: [],
      });
    });
    const player = page.locator('#empty-chapter-player .beecast-player');
    const button = player.locator('.sp-chapbtn');
    const panel = player.locator('.sp-chapters');
    await expect(button).toBeHidden();
    await expect(panel).toBeHidden();
    await player.focus();
    await page.keyboard.press('c');
    await expect(panel).toBeHidden();
    await expect(button).toHaveAttribute('aria-expanded', 'false');
  });

  test('all player controls remain reachable at phone width', async ({ page }) => {
    await page.setViewportSize({ width: 320, height: 640 });
    await page.goto(fileUrl());
    const player = page.locator('.beecast-player');
    const bounds = await player.boundingBox();
    for (const control of ['.sp-play', '.sp-seek', '.sp-chapbtn', '.sp-speed', '.sp-fs']) {
      const box = await page.locator(control).boundingBox();
      expect(box, `${control} is visible`).not.toBeNull();
      expect(box.x).toBeGreaterThanOrEqual(bounds.x);
      expect(box.x + box.width).toBeLessThanOrEqual(bounds.x + bounds.width + 1);
    }
  });

  test('pointer seeking works for touch and pen as well as mouse', async ({ page }) => {
    await page.goto(fileUrl());
    const seek = page.locator('.sp-seek');
    const box = await seek.boundingBox();
    await seek.dispatchEvent('pointerdown', {
      pointerId: 7, pointerType: 'touch', button: 0,
      clientX: box.x + box.width * 0.75, clientY: box.y + box.height / 2,
    });
    const value = Number(await seek.getAttribute('aria-valuenow'));
    const max = Number(await seek.getAttribute('aria-valuemax'));
    expect(value).toBeGreaterThan(max * 0.5);
  });

  test('chapter navigation creates and restores a focused history entry', async ({ page }) => {
    // Narrow enough that chapters stay an opt-in overlay (not auto-docked).
    await page.setViewportSize({ width: 520, height: 700 });
    await page.goto(fileUrl());
    await page.locator('.sp-chapbtn').click();
    await page.locator('.sp-chap').nth(1).click();
    expect(new URL(page.url()).searchParams.has('t')).toBe(true);
    await page.goBack();
    await expect(page.locator('.beecast-player')).toBeFocused();
    expect(Number(await page.locator('.sp-seek').getAttribute('aria-valuenow'))).toBe(0);
  });

  test('keyboard chapter jumps show a disappearing toast naming the chapter', async ({ page }) => {
    await page.goto(fileUrl());
    await page.locator('.beecast-player').focus();
    await page.keyboard.press(']');
    const toast = page.locator('.sp-toast');
    await expect(toast).toHaveClass(/sp-toast-show/);
    await expect(toast.locator('.sp-toast-state-label')).toHaveText('CHAPTER');
    expect(await toast.locator('.sp-toast-id').textContent()).toMatch(/./);
    expect(await toast.locator('.sp-toast-meta').textContent()).toMatch(/^\d+\/\d+ · ./);
    // It fades out on its own.
    await expect(toast).not.toHaveClass(/sp-toast-show/, { timeout: 5000 });
  });

  test('digit and arrow keys jump chapters; c toggles the panel', async ({ page }) => {
    // Overlay mode: chapters start hidden and close after a row click.
    await page.setViewportSize({ width: 520, height: 700 });
    await page.goto(fileUrl());
    const player = page.locator('.beecast-player');
    await expect(player).not.toHaveClass(/sp-chapters-dock/);
    await player.focus();
    await page.keyboard.press('c');
    const panel = page.locator('.sp-chapters');
    await expect(panel).toBeVisible();
    // Open panel rows stay clickable (not rebuilt out from under the pointer).
    await panel.locator('.sp-chap').nth(0).click();
    await expect(panel).toBeHidden();
    await player.focus();
    await page.keyboard.press('c');
    await expect(panel).toBeVisible();
    await page.keyboard.press('c');
    await expect(panel).toBeHidden();
    await player.focus();
    await page.keyboard.press('0');
    await expect(page.locator('.sp-toast')).toHaveClass(/sp-toast-show/);
    expect(await page.locator('.sp-toast-meta').textContent()).toMatch(/^1\/\d+ · ./);
    await page.keyboard.press('ArrowDown');
    expect(await page.locator('.sp-toast-meta').textContent()).toMatch(/^2\/\d+ · ./);
  });

  test('tall mounts dock chapters beside the terminal', async ({ page }) => {
    await page.setViewportSize({ width: 1100, height: 720 });
    await page.goto(fileUrl());
    const player = page.locator('.beecast-player');
    await expect(player).toHaveClass(/sp-chapters-dock/);
    const panel = page.locator('.sp-chapters');
    await expect(panel).toBeVisible();
    await panel.locator('.sp-chap').nth(1).click();
    // Docked picks keep the list open.
    await expect(panel).toBeVisible();
    await player.focus();
    await page.keyboard.press('c');
    await expect(panel).toBeHidden();
  });

  test('definite-height mounts pin the bar at the bottom and center the terminal', async ({ page }) => {
    await page.setViewportSize({ width: 1100, height: 720 });
    await page.goto(fileUrl());
    const geom = await page.evaluate(() => {
      const player = document.querySelector('.beecast-player').getBoundingClientRect();
      const bar = document.querySelector('.sp-bar').getBoundingClientRect();
      const box = document.querySelector('.sp-screen-box').getBoundingClientRect();
      const stage = document.querySelector('.sp-stage').getBoundingClientRect();
      return { player, bar, box, stage };
    });
    // Control bar hugs the bottom of the player (no interstitial gap below the terminal).
    expect(Math.abs((geom.bar.y + geom.bar.height) - (geom.player.y + geom.player.height))).toBeLessThan(2);
    expect(geom.bar.y).toBeGreaterThan(geom.box.y + geom.box.height - 1);
    // Terminal is vertically centered in the stage (above the bar).
    const stageMid = geom.stage.y + geom.stage.height / 2;
    const boxMid = geom.box.y + geom.box.height / 2;
    expect(Math.abs(boxMid - stageMid)).toBeLessThan(24);
  });

  test('menus move focus with arrows and return it with Escape', async ({ page }) => {
    await page.goto(fileUrl());
    const speed = page.locator('.sp-speed');
    await speed.click();
    await expect(page.locator('.sp-speedopt.sp-on')).toBeFocused();
    const before = await page.locator(':focus').textContent();
    await page.keyboard.press('ArrowDown');
    expect(await page.locator(':focus').textContent()).not.toBe(before);
    await page.keyboard.press('Escape');
    await expect(speed).toBeFocused();
  });
});
