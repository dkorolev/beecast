'use strict';

const http = require('http');
const fs = require('fs');
const path = require('path');
const { pathToFileURL } = require('url');

const PAGE = path.join(__dirname, '..', '.artifacts', 'sample.html');

/** The built page as a file:// URL — Playwright navigates it directly. */
function fileUrl() {
  return pathToFileURL(PAGE).href;
}

/** A throwaway HTTP server for the built page; returns { url, close }. */
function startServer() {
  const html = fs.readFileSync(PAGE);
  const server = http.createServer((req, res) => {
    res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
    res.end(html);
  });
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      resolve({
        url: `http://127.0.0.1:${port}/sample.html`,
        close: () => new Promise((done) => server.close(done)),
      });
    });
  });
}

/**
 * Collect everything the page is forbidden to emit: console errors AND warnings, plus
 * uncaught exceptions. Attach before navigation; assert `violations` is empty at the end.
 */
function collectConsoleViolations(page) {
  const violations = [];
  page.on('console', (msg) => {
    if (msg.type() === 'error' || msg.type() === 'warning') {
      violations.push(`console.${msg.type()}: ${msg.text()}`);
    }
  });
  page.on('pageerror', (err) => {
    violations.push(`pageerror: ${err.message}`);
  });
  return violations;
}

/**
 * Drive the player through a representative session: play, watch time advance, pause,
 * seek from the keyboard, open and close the speed menu. Enough churn that a gate
 * holding through it means something.
 */
async function exercisePlayer(page) {
  const play = page.locator('.sp-play');
  await play.waitFor({ state: 'visible' });

  await play.click();
  await page.locator('.sp-play[aria-label="Pause"]').waitFor();
  const seek = page.locator('.sp-seek');
  await page.waitForFunction(
    () => Number(document.querySelector('.sp-seek').getAttribute('aria-valuenow')) > 0,
  );
  await play.click();
  await page.locator('.sp-play[aria-label="Play"]').waitFor();

  await seek.focus();
  await page.keyboard.press('ArrowRight');
  await page.keyboard.press('End');
  await page.keyboard.press('Home');

  const speed = page.locator('.sp-speed');
  await speed.click();
  await page.locator('.sp-speedmenu:not([hidden])').waitFor();
  await page.keyboard.press('Escape');
  await page.locator('.sp-speedmenu').waitFor({ state: 'hidden' });
}

module.exports = { fileUrl, startServer, collectConsoleViolations, exercisePlayer };
