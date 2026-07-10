#!/usr/bin/env node
'use strict';

const path = require('path');
const fs = require('fs');
const { performance } = require('perf_hooks');

const bundle = process.argv[2];
if (!bundle) {
  console.error('usage: node bench.js path/to/player-bundle.js [10k|100k|1m|all]');
  process.exit(2);
}
const resolved = path.resolve(bundle);
if (fs.statSync(resolved).isDirectory()) {
  require(path.join(resolved, 'vt.js'));
  require(path.join(resolved, 'controller.js'));
} else {
  require(resolved);
}
const VT = globalThis.BeeCastVT;
const Controller = globalThis.BeeCastController;
if (!VT || !Controller) throw new Error('bundle does not expose BeeCastVT and BeeCastController');

const ESC = '\x1b';
const shapes = {
  scroll: i => `line ${i}\r\n`,
  sgr: i => `${ESC}[${30 + (i % 8)};1mvalue ${i}${ESC}[0m\r${ESC}[2K`,
  alt: i => `${ESC}[?1049h${ESC}[Hframe ${i}${ESC}[2J${ESC}[?1049l`,
  sparse: i => `event ${i}\r\n`,
};

function generate(count, shape) {
  const lines = [`{"version":2,"width":80,"height":24}`];
  const render = shapes[shape];
  for (let i = 0; i < count; i++) {
    const t = shape === 'sparse' ? i * 60 : i / 1000;
    lines.push(JSON.stringify([t, 'o', render(i)]));
  }
  return lines.join('\n') + '\n';
}

function timed(fn) {
  const start = performance.now();
  const value = fn();
  return [performance.now() - start, value];
}
function mb(n) { return (n / 1024 / 1024).toFixed(1); }
function ms(n) { return n.toFixed(1); }
function rate(n, elapsed) { return Math.round(n / (elapsed / 1000)).toLocaleString('en-US'); }

function appendHostile(controller, text) {
  const widths = [1, 2, 7, 31, 3, 127, 11, 509];
  for (let p = 0, i = 0; p < text.length; i++) {
    const end = Math.min(text.length, p + widths[i % widths.length]);
    controller.append(text.slice(p, end));
    p = end;
  }
}

function fakeClock() {
  let now = 0;
  let queue = [];
  return {
    now: () => now,
    requestAnimationFrame: cb => { queue.push(cb); return queue.length; },
    cancelAnimationFrame: () => { queue = []; },
    flush: step => { now += step; const q = queue; queue = []; q.forEach(cb => cb(now)); },
  };
}

function run(count, shape) {
  if (global.gc) global.gc();
  const before = process.memoryUsage().heapUsed;
  const castText = generate(count, shape);
  const [parseMs, cast] = timed(() => VT.parseCast(castText));
  const resident = process.memoryUsage().heapUsed - before;
  const ctrl = Controller.create({ data: castText });
  ctrl.seek(cast.duration);
  const [seekMs] = timed(() => ctrl.seek(Math.max(0, cast.duration - 0.001)));

  const header = `{"version":2,"width":80,"height":24}\n`;
  const body = castText.slice(header.length);
  const live = Controller.create({ data: header });
  const [appendMs] = timed(() => appendHostile(live, body));

  const clock = fakeClock();
  const playing = Controller.create({ data: castText, clock });
  playing.play();
  clock.flush(0);
  const deliveries = 1000;
  const [stateMs] = timed(() => {
    for (let i = 0; i < deliveries; i++) {
      clock.flush(16);
      playing.getState();
    }
  });
  ctrl.dispose(); live.dispose(); playing.dispose();
  return [count.toLocaleString('en-US'), shape, ms(parseMs), mb(resident), ms(seekMs), rate(count, appendMs), (stateMs / deliveries).toFixed(3)];
}

const requested = process.argv[3] || 'all';
const sizes = requested === 'all' ? [10000, 100000, 1000000]
  : [({ '10k': 10000, '100k': 100000, '1m': 1000000 })[requested] || Number(requested)];
const rows = [];
for (const count of sizes) for (const shape of Object.keys(shapes)) rows.push(run(count, shape));
console.table(rows.map(r => ({ events: r[0], shape: r[1], 'parse ms': r[2], 'heap MB': r[3], 'backseek ms': r[4], 'append ev/s': r[5], 'state ms': r[6] })));
