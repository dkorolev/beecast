'use strict';

const { execFileSync } = require('child_process');
const fs = require('fs');
const path = require('path');

// Build the page under test from the checked-in fixture with the real CLI. The sidecar
// (sample.meta.json) is discovered next to the cast, so the page carries the title,
// summary, and chapters — the same document the CLI byte-pin tests assert on.
module.exports = function globalSetup() {
  const repoRoot = path.join(__dirname, '..');
  const artifacts = path.join(__dirname, '.artifacts');
  fs.mkdirSync(artifacts, { recursive: true });
  execFileSync(
    'cargo',
    [
      'run', '-q', '-p', 'beecast', '--',
      'build', path.join(repoRoot, 'cli', 'tests', 'fixtures', 'sample.cast'),
      '-o', path.join(artifacts, 'sample.html'),
    ],
    { cwd: repoRoot, stdio: 'inherit' },
  );
};
