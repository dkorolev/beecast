'use strict';

const { execFileSync } = require('child_process');
const fs = require('fs');
const path = require('path');

// Build the pages under test from the checked-in fixtures with the real CLI. The sample
// sidecar is discovered next to its cast, so that page carries the same title, summary,
// and chapters as the document the CLI byte-pin tests assert on.
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
  execFileSync(
    'cargo',
    [
      'run', '-q', '-p', 'beecast', '--',
      'build', path.join(repoRoot, 'cli', 'tests', 'fixtures', 'long-sparse.cast'),
      '-o', path.join(artifacts, 'long-sparse.html'),
    ],
    { cwd: repoRoot, stdio: 'inherit' },
  );
};
