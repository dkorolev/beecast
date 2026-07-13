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
  test('adjacent terminal background runs paint without horizontal seams', async ({ page }) => {
    await page.goto(fileUrl());
    const geometry = await page.evaluate(() => {
      const screen = document.querySelector('.sp-screen');
      screen.style.transform = 'scale(0.773)'; // exercise fractional pixel boundaries
      screen.innerHTML = '<span class="sp-bg" style="--sp-run-bg:#777;background:#777">AAAAAAAAAA</span>\n' +
        '<span class="sp-bg" style="--sp-run-bg:#777;background:#777">BBBBBBBBBB</span>\n' +
        '<span class="sp-bg" style="--sp-run-bg:#777;background:#777">CCCCCCCCCC</span>';
      const rows = Array.from(screen.querySelectorAll('span')).map((span) => {
        const rect = span.getBoundingClientRect();
        return { top: rect.top, bottom: rect.bottom };
      });
      return rows;
    });
    expect(geometry).toHaveLength(3);
    // Firefox reports adjacent edges with ~0.017 px of floating-point noise; anything
    // below 0.02 px still paints the same device pixel with no visible background gap.
    expect(Math.abs(geometry[0].bottom - geometry[1].top)).toBeLessThan(0.02);
    expect(Math.abs(geometry[1].bottom - geometry[2].top)).toBeLessThan(0.02);
  });

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

  test('a ?t=…&note=… deep link parks the player and shows the note banner', async ({ page }) => {
    // Deep links are query parameters precisely so they work from a file:// path — the
    // riskier transport is the one under test. (Was a manual pre-release check.)
    await page.goto(fileUrl() + '?t=1&note=hi');
    // Parked at the linked timestamp: paused (poster frame, play overlay up), not playing.
    await expect(page.locator('.sp-play')).toHaveAttribute('aria-label', 'Play');
    await expect(page.locator('.sp-overlay')).toBeVisible();
    expect(Number(await page.locator('.sp-seek').getAttribute('aria-valuenow'))).toBe(1);
    // The note banner names the moment and carries the comment.
    const banner = page.locator('#note-banner');
    await expect(banner).toBeVisible();
    await expect(banner.locator('.at')).toHaveText('@0:01');
    await expect(banner).toContainText('hi');
    // The comment is pre-filled so re-sharing keeps it.
    await expect(page.locator('#note')).toHaveValue('hi');
  });

  test('chapter navigation creates and restores a focused history entry', async ({ page }) => {
    // Chapters are always an opt-in overlay.
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
    await expect(toast.locator('.sp-toast-state')).toBeHidden();
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

  test('tall mounts keep chapters in an overlay', async ({ page }) => {
    await page.setViewportSize({ width: 1100, height: 720 });
    await page.goto(fileUrl());
    const player = page.locator('.beecast-player');
    await expect(player).not.toHaveClass(/sp-chapters-dock/);
    const panel = page.locator('.sp-chapters');
    await player.focus();
    await page.keyboard.press('c');
    await expect(panel).toBeVisible();
    await panel.locator('.sp-chap').nth(1).click();
    // Picks close the overlay and never resize the terminal.
    await expect(panel).toBeHidden();
    await player.focus();
    await page.keyboard.press('c');
    await expect(panel).toBeVisible();
  });

  test('leaving fullscreen recenters the same player in the page', async ({ page }) => {
    await page.goto(fileUrl());
    await page.evaluate(() => {
      document.body.style.paddingTop = '900px';
      document.body.style.paddingBottom = '900px';
    });
    await page.locator('.sp-fs').evaluate((button) => button.click());
    await page.waitForFunction(() => document.fullscreenElement !== null);
    await page.evaluate(() => document.exitFullscreen());
    await page.waitForFunction(() => document.fullscreenElement === null);
    await page.waitForTimeout(500);
    const offset = await page.locator('.beecast-player').evaluate((el) => {
      const rect = el.getBoundingClientRect();
      return Math.abs((rect.top + rect.height / 2) - window.innerHeight / 2);
    });
    expect(offset).toBeLessThan(24);
  });

  test('chapter overlay never changes fullscreen terminal scale', async ({ page }) => {
    await page.goto(fileUrl());
    await page.locator('.sp-fs').evaluate((button) => button.click());
    await page.waitForFunction(() => document.fullscreenElement !== null);
    const player = page.locator('.beecast-player');
    const before = await page.locator('.sp-screen').evaluate((screen) => ({
      transform: screen.style.transform,
      rect: screen.getBoundingClientRect().toJSON(),
    }));
    await player.focus();
    await page.keyboard.press('c');
    await expect(page.locator('.sp-chapters')).toBeVisible();
    const after = await page.locator('.sp-screen').evaluate((screen) => ({
      transform: screen.style.transform,
      rect: screen.getBoundingClientRect().toJSON(),
    }));
    expect(after.transform).toBe(before.transform);
    expect(Math.abs(after.rect.width - before.rect.width)).toBeLessThan(0.01);
    expect(Math.abs(after.rect.height - before.rect.height)).toBeLessThan(0.01);
    await page.evaluate(() => document.exitFullscreen());
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
