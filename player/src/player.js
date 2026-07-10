// beecast-player: the DOM half (see the crate README). Renders controller state,
// builds the default controls, registers <beecast-player>, and keeps BeeCastPlayer.create
// as a compatibility adapter over the same surface.
//
// Clean-room implementation, MIT like the rest of beecast. The time axis is ALWAYS
// recording time: idle compression only changes pacing, never the clock the API speaks.
'use strict';
(function (root) {

const VT = root.BeeCastVT;
const Controller = root.BeeCastController;
const SEEK_STEP_SECS = 5;
const SPEEDS = Controller.SPEEDS;

// The big center play glyph: a solid right-pointing triangle in block characters.
// Half-block edges soften the silhouette so it reads as a play icon, not a staircase.
const BIG_PLAY =
  '  ▄█\n' +
  ' ████\n' +
  '██████\n' +
  '████████\n' +
  '██████\n' +
  ' ████\n' +
  '  ▀█';

const ICON_PLAY = '▶';
const ICON_PAUSE = '⏸';

// ---- rendering -------------------------------------------------------------------------
const ATTR_CLASSES = [
  [VT.A_BOLD, 'sp-b'], [VT.A_DIM, 'sp-d'], [VT.A_ITALIC, 'sp-i'],
  [VT.A_UNDER, 'sp-u'], [VT.A_STRIKE, 'sp-s'],
];

function colorCss(c, bold) {
  if (c == null) return null;
  if (typeof c === 'string') return c;
  const idx = bold && c < 8 ? c + 8 : c;
  return idx < 16 ? 'var(--sp-c' + idx + ')' : VT.color256(idx);
}

function esc(s) {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function runHtml(run, hasCursor, cursorCol) {
  if (hasCursor && run.text.length > 1) {
    const before = { text: run.text.slice(0, cursorCol), fg: run.fg, bg: run.bg, attrs: run.attrs };
    const at = { text: run.text[cursorCol] || ' ', fg: run.fg, bg: run.bg, attrs: run.attrs };
    const after = { text: run.text.slice(cursorCol + 1), fg: run.fg, bg: run.bg, attrs: run.attrs };
    return (before.text ? runHtml(before, false, 0) : '') + runHtml(at, true, 0) +
      (after.text ? runHtml(after, false, 0) : '');
  }
  const inverse = (run.attrs & VT.A_INVERSE) !== 0;
  const bold = (run.attrs & VT.A_BOLD) !== 0;
  let fg = colorCss(run.fg, bold);
  let bg = colorCss(run.bg, false);
  if (inverse) { const t = fg || 'var(--sp-fg)'; fg = bg || 'var(--sp-bg)'; bg = t; }
  const classes = [];
  for (const pair of ATTR_CLASSES) if (run.attrs & pair[0]) classes.push(pair[1]);
  if (hasCursor) classes.push('sp-cur');
  let style = '';
  if (fg) style += 'color:' + fg + ';';
  if (bg) style += 'background:' + bg + ';';
  if (!classes.length && !style) return esc(run.text);
  return '<span' + (classes.length ? ' class="' + classes.join(' ') + '"' : '') +
    (style ? ' style="' + style + '"' : '') + '>' + esc(run.text) + '</span>';
}

function screenHtml(snap) {
  const lines = [];
  for (let y = 0; y < snap.rows.length; y++) {
    let x = 0, html = '';
    for (const run of snap.rows[y]) {
      const cursorHere = snap.cursor.visible && snap.cursor.y === y &&
        snap.cursor.x >= x && snap.cursor.x < x + run.text.length;
      html += runHtml(run, cursorHere, snap.cursor.x - x);
      x += run.text.length;
    }
    lines.push(html);
  }
  return lines.join('\n');
}

function fmtClock(secs) {
  secs = Math.max(0, Math.floor(secs));
  const m = Math.floor(secs / 60), s = secs % 60;
  return m + ':' + String(s).padStart(2, '0');
}

function parseControls(controls) {
  if (controls === false) {
    return { play: false, seek: false, time: false, speed: false, chapters: false, fullscreen: false };
  }
  const d = { play: true, seek: true, time: true, speed: true, chapters: true, fullscreen: true };
  if (controls && typeof controls === 'object') {
    for (const k of Object.keys(d)) {
      if (controls[k] != null) d[k] = !!controls[k];
    }
  }
  return d;
}

function dispatchBee(el, name, detail) {
  if (!el || typeof CustomEvent === 'undefined') return;
  try {
    el.dispatchEvent(new CustomEvent(name, {
      detail: detail || {},
      bubbles: true,
      composed: true,
    }));
  } catch (_) {}
}

// ---- player view -----------------------------------------------------------------------
function Player(src, mount, opts) {
  opts = opts || {};
  this.opts = opts;
  this.controlsCfg = parseControls(opts.controls);
  this.fit = opts.fit || null;
  this.fsEl = opts.fullscreenEl || null;
  this.accessibility = opts.accessibility || 'snapshot';
  this.disposed = false;
  this._lastAtEdge = null;

  const data = src && (src.data != null ? src.data : src.cast);
  this.controller = Controller.create({
    data: data,
    source: src && src.source,
    idleTimeLimit: opts.idleTimeLimit,
    markers: opts.markers,
    speed: opts.speed,
    startAt: opts.startAt,
    clock: opts.clock,
  });

  this.buildDom(mount, this.controlsCfg);
  this.bindController();
  this.layout();
  // Compatibility surface: read-only playing getter over controller state.
  Object.defineProperty(this, 'playing', {
    configurable: true,
    enumerable: true,
    get: function () { return this.controller.isPlaying(); },
  });
  // Non-public fields kept readable during the migration window only.
  Object.defineProperty(this, 'pacedPos', {
    configurable: true,
    get: function () { return this.controller.pacedPos; },
  });
  Object.defineProperty(this, 'eventIdx', {
    configurable: true,
    get: function () { return this.controller.eventIdx; },
  });
  Object.defineProperty(this, 'cast', {
    configurable: true,
    get: function () { return this.controller.cast; },
  });
  Object.defineProperty(this, 'speed', {
    configurable: true,
    get: function () { return this.controller.speed; },
    set: function (v) { this.controller.setSpeed(v); },
  });

  if (opts.autoPlay) this.play();
  const self = this;
  if (typeof ResizeObserver !== 'undefined') {
    this.resizeObs = new ResizeObserver(function () {
      if (!self._layouting) self.layout();
    });
    this.resizeObs.observe(this.root.parentNode || this.root);
  }
  this.fsHandler = function () { self.layout(); };
  if (typeof document !== 'undefined') {
    document.addEventListener('fullscreenchange', this.fsHandler);
  }
}

Player.prototype.bindController = function () {
  const self = this;
  this.unsubscribe = this.controller.subscribe(function (state, meta) {
    self.onState(state, meta || {});
  });
};

Player.prototype.onState = function (state, meta) {
  if (this.disposed) return;
  const type = meta.type || 'change';

  if (this.screenEl) {
    if (type === 'ready' || type === 'seek' || type === 'play' || type === 'pause' ||
        type === 'ended' || type === 'timeupdate' || type === 'durationchange' ||
        type === 'change' || meta.terminalChanged || meta.resized) {
      // High-frequency path: still render terminal when it may have changed.
      if (type !== 'timeupdate' || meta.terminalChanged || meta.resized || !this._paintedOnce) {
        this.screenEl.innerHTML = screenHtml(state.terminal);
        this._paintedOnce = true;
        if (this.accessibility === 'snapshot' && this.a11yEl) {
          this.a11yEl.textContent = terminalPlain(state.terminal);
        }
      }
    }
  }

  this.renderBar(state);
  this.syncOverlay(state);
  this.syncChaptersUi(state);
  // Declared-live: the bar pins full-width in the live color (see .sp-islive in the CSS).
  if (this.root) this.root.classList.toggle('sp-islive', !!state.live);
  if (meta.resized) this.layout();
  if (type === 'ready' || type === 'durationchange' || type === 'markerchange') {
    this.layoutMarkers(state);
  }

  // Integration events on the root element (Phase 2).
  const el = this.eventTarget || this.root;
  if (type === 'ready') dispatchBee(el, 'beecast-ready', { state: publicState(state) });
  if (type === 'play') dispatchBee(el, 'beecast-play', { origin: meta.origin, currentTime: state.currentTime });
  if (type === 'pause') dispatchBee(el, 'beecast-pause', { origin: meta.origin, currentTime: state.currentTime });
  if (type === 'ended') dispatchBee(el, 'beecast-ended', { currentTime: state.currentTime, duration: state.duration });
  if (type === 'seek') {
    dispatchBee(el, 'beecast-seek', {
      origin: meta.origin,
      currentTime: state.currentTime,
      duration: state.duration,
    });
  }
  if (type === 'timeupdate') {
    dispatchBee(el, 'beecast-timeupdate', {
      currentTime: state.currentTime,
      duration: state.duration,
      atLiveEdge: state.atLiveEdge,
    });
  }
  if (type === 'speedchange') {
    dispatchBee(el, 'beecast-speedchange', { speed: state.speed, origin: meta.origin });
  }
  if (type === 'durationchange') {
    dispatchBee(el, 'beecast-durationchange', { duration: state.duration });
  }
  if (type === 'livechange') {
    dispatchBee(el, 'beecast-livechange', { live: !!state.live, origin: meta.origin });
  }
  if (this._lastAtEdge != null && this._lastAtEdge !== state.atLiveEdge) {
    dispatchBee(el, 'beecast-liveedgechange', { atLiveEdge: state.atLiveEdge });
  }
  if (type === 'durationchange' || type === 'ready' || type === 'markerchange') {
    dispatchBee(el, 'beecast-markerchange', { markers: state.markers });
  }
  this._lastAtEdge = state.atLiveEdge;
};

function publicState(state) {
  return {
    status: state.status,
    currentTime: state.currentTime,
    duration: state.duration,
    speed: state.speed,
    atLiveEdge: state.atLiveEdge,
    live: state.live,
    canAppend: state.canAppend,
    markers: state.markers,
    dimensions: state.dimensions,
  };
}

function terminalPlain(snap) {
  const lines = [];
  for (let y = 0; y < snap.rows.length; y++) {
    let s = '';
    for (const run of snap.rows[y]) s += run.text;
    lines.push(s.replace(/\s+$/, ''));
  }
  return lines.join('\n');
}

Player.prototype.buildDom = function (mount, cfg) {
  const self = this;
  const root = document.createElement('div');
  root.className = 'beecast-player';
  root.setAttribute('part', 'root');
  root.tabIndex = 0;
  root.setAttribute('role', 'application');
  root.setAttribute('aria-label', 'Terminal recording player');

  let bar = '';
  if (cfg.play || cfg.seek || cfg.time || cfg.speed || cfg.chapters || cfg.fullscreen) {
    bar = '<div class="sp-bar" part="toolbar">';
    if (cfg.play) {
      bar += '<button class="sp-play" type="button" part="play-button" ' +
        'aria-label="Play" title="play/pause (space)">' + ICON_PLAY + '</button>';
    }
    if (cfg.time) bar += '<span class="sp-time" part="current-time" aria-hidden="true">0:00</span>';
    if (cfg.seek) {
      bar += '<div class="sp-seek" part="seek" role="slider" tabindex="0" ' +
        'aria-label="Seek" aria-valuemin="0" aria-valuemax="0" aria-valuenow="0">' +
        '<div class="sp-fill"></div><div class="sp-markers"></div></div>';
    }
    if (cfg.time) bar += '<span class="sp-dur" part="duration" aria-hidden="true">0:00</span>';
    if (cfg.chapters) {
      bar += '<button class="sp-chapbtn" type="button" part="chapter-button" ' +
        'aria-label="Chapters" aria-expanded="false" title="chapters (c)" hidden>☰</button>';
    }
    if (cfg.speed) {
      bar += '<span class="sp-speedwrap">' +
        '<button class="sp-speed" type="button" part="speed-button" ' +
        'aria-label="Playback speed" aria-haspopup="menu" aria-expanded="false" ' +
        'title="speed (&lt; / &gt;)">1×</button>' +
        '<div class="sp-speedmenu" part="speed-menu" role="menu" hidden></div></span>';
    }
    if (cfg.fullscreen) {
      bar += '<button class="sp-fs" type="button" part="fullscreen-button" ' +
        'aria-label="Fullscreen" title="fullscreen (f)">⛶</button>';
    }
    bar += '</div>';
  }

  root.innerHTML =
    '<div class="sp-screen-box" part="screen-box">' +
    '<pre class="sp-screen" part="screen" aria-hidden="true"></pre>' +
    (this.accessibility === 'snapshot'
      ? '<pre class="sp-a11y" part="terminal-text"></pre>'
      : '') +
    '<div class="sp-overlay" part="overlay" hidden role="button" tabindex="0" ' +
    'aria-label="Play recording"><pre class="sp-bigplay" aria-hidden="true">' + BIG_PLAY + '</pre></div>' +
    '<div class="sp-chapters" part="chapter-panel" role="menu" hidden></div>' +
    '</div>' + bar;

  mount.appendChild(root);
  this.root = root;
  this.screenEl = root.querySelector('.sp-screen');
  this.a11yEl = root.querySelector('.sp-a11y');
  this.playBtn = root.querySelector('.sp-play');
  this.timeEl = root.querySelector('.sp-time');
  this.durEl = root.querySelector('.sp-dur');
  this.seekEl = root.querySelector('.sp-seek');
  this.fillEl = root.querySelector('.sp-fill');
  this.speedBtn = root.querySelector('.sp-speed');
  this.speedMenuEl = root.querySelector('.sp-speedmenu');
  this.chapBtn = root.querySelector('.sp-chapbtn');
  this.chaptersEl = root.querySelector('.sp-chapters');
  this.fsBtn = root.querySelector('.sp-fs');
  this.overlayEl = root.querySelector('.sp-overlay');
  this.marksEl = root.querySelector('.sp-markers');

  if (this.playBtn) {
    this.playBtn.addEventListener('click', function () { self.toggle('pointer'); });
  }
  if (this.overlayEl) {
    const playFromOverlay = function (ev) {
      ev.stopPropagation();
      self.play('pointer');
      try { root.focus({ preventScroll: true }); } catch (_) { root.focus(); }
    };
    this.overlayEl.addEventListener('click', playFromOverlay);
    this.overlayEl.addEventListener('keydown', function (ev) {
      if (ev.key === 'Enter' || ev.key === ' ') { ev.preventDefault(); playFromOverlay(ev); }
    });
  }
  if (this.speedBtn) {
    this.speedBtn.addEventListener('click', function (ev) {
      ev.stopPropagation();
      self.toggleSpeedMenu();
    });
  }
  if (this.chapBtn) {
    this.chapBtn.addEventListener('click', function () { self.toggleChapters(); });
  }
  if (this.fsBtn) {
    this.fsBtn.addEventListener('click', function () { self.toggleFullscreen(); });
  }
  if (this.seekEl) {
    const seekTo = function (ev, origin) {
      const r = self.seekEl.getBoundingClientRect();
      const frac = Math.min(1, Math.max(0, (ev.clientX - r.left) / (r.width || 1)));
      const dur = self.controller.cast.duration;
      self.seek(frac * dur, origin || 'pointer');
    };
    this.seekEl.addEventListener('mousedown', function (ev) {
      seekTo(ev, 'pointer');
      const move = function (e) { seekTo(e, 'pointer'); };
      const up = function () {
        document.removeEventListener('mousemove', move);
        document.removeEventListener('mouseup', up);
      };
      document.addEventListener('mousemove', move);
      document.addEventListener('mouseup', up);
    });
    this.seekEl.addEventListener('keydown', function (ev) {
      const dur = self.controller.cast.duration;
      const now = self.getCurrentTime();
      let t = null;
      if (ev.key === 'ArrowLeft') t = now - SEEK_STEP_SECS;
      else if (ev.key === 'ArrowRight') t = now + SEEK_STEP_SECS;
      else if (ev.key === 'PageDown') t = now - 30;
      else if (ev.key === 'PageUp') t = now + 30;
      else if (ev.key === 'Home') t = 0;
      else if (ev.key === 'End') t = dur;
      else return;
      ev.preventDefault();
      self.seek(t, 'keyboard');
    });
  }
  this.keyHandler = function (ev) { self.onKey(ev); };
  root.addEventListener('keydown', this.keyHandler);
  root.addEventListener('click', function () {
    try { root.focus({ preventScroll: true }); } catch (_) { root.focus(); }
  });
};

Player.prototype.syncOverlay = function (state) {
  if (!this.overlayEl) return;
  // Show whenever playback is not running (start, paused mid-cast, ended) so the
  // big glyph is the obvious "press play" affordance — not only at t = 0.
  const show = state.status !== 'playing' && state.duration > 0;
  this.overlayEl.hidden = !show;
};

Player.prototype.renderBar = function (state) {
  if (this.timeEl) this.timeEl.textContent = fmtClock(state.currentTime);
  if (this.durEl) this.durEl.textContent = fmtClock(state.duration);
  if (this.fillEl) {
    this.fillEl.style.width = (state.duration > 0
      ? Math.min(100, (state.currentTime / state.duration) * 100)
      : 0) + '%';
  }
  if (this.seekEl) {
    this.seekEl.setAttribute('aria-valuemin', '0');
    this.seekEl.setAttribute('aria-valuemax', String(Math.floor(state.duration)));
    this.seekEl.setAttribute('aria-valuenow', String(Math.floor(state.currentTime)));
    this.seekEl.setAttribute('aria-valuetext', fmtClock(state.currentTime) + ' of ' + fmtClock(state.duration));
  }
  if (this.playBtn) {
    const playing = state.status === 'playing';
    this.playBtn.textContent = playing ? ICON_PAUSE : ICON_PLAY;
    this.playBtn.setAttribute('aria-label', playing ? 'Pause' : 'Play');
    this.playBtn.setAttribute('aria-pressed', playing ? 'true' : 'false');
  }
  if (this.speedBtn) {
    this.speedBtn.textContent = String(state.speed).replace(/\.0$/, '') + '\u00d7';
  }
};

Player.prototype.layoutMarkers = function (state) {
  if (!this.marksEl) return;
  this.marksEl.innerHTML = '';
  if (!(state.duration > 0)) return;
  for (const m of state.markers) {
    const tick = document.createElement('div');
    tick.className = 'sp-marker' + (m.type === 'annotation' ? ' sp-marker-ann' : '');
    tick.style.left = Math.min(100, (m.time / state.duration) * 100) + '%';
    if (m.color) tick.style.background = m.color;
    tick.title = fmtClock(m.time) + (m.label ? ' ' + m.label : '');
    this.marksEl.appendChild(tick);
  }
};

Player.prototype.toggleChapters = function (force) {
  if (!this.chaptersEl) return;
  const show = force != null ? force : this.chaptersEl.hidden;
  // Focus must be checked BEFORE hiding: hiding the focused row silently moves
  // focus to <body>, and the ☰ button would never get it back.
  const hadFocus = !show && typeof document !== 'undefined' &&
    this.chaptersEl.contains(document.activeElement);
  this.chaptersEl.hidden = !show;
  if (this.chapBtn) this.chapBtn.setAttribute('aria-expanded', show ? 'true' : 'false');
  if (show) this.renderChapters(this.controller.getState());
  else if (hadFocus && this.chapBtn) {
    try { this.chapBtn.focus({ preventScroll: true }); } catch (_) {}
  }
};

Player.prototype.renderChapters = function (state) {
  if (!this.chaptersEl) return;
  const self = this;
  this.chaptersEl.innerHTML = '';
  const now = state.currentTime;
  let currentId = null;
  for (let i = 0; i < state.markers.length; i++) {
    if (state.markers[i].time <= now + 1e-9) currentId = state.markers[i].id;
  }
  for (const m of state.markers) {
    const row = document.createElement('button');
    row.type = 'button';
    row.className = 'sp-chap' + (m.id === currentId ? ' sp-chap-on' : '') +
      (m.type === 'annotation' ? ' sp-chap-ann' : '');
    row.setAttribute('role', 'menuitem');
    const t = document.createElement('span');
    t.className = 'sp-chap-t';
    t.textContent = fmtClock(m.time);
    row.appendChild(t);
    row.appendChild(document.createTextNode(m.label || ''));
    row.addEventListener('click', (function (marker) {
      return function (ev) {
        ev.stopPropagation();
        const el = self.eventTarget || self.root;
        // Cancellable marker selection (Phase 5).
        let cancelled = false;
        if (el && typeof CustomEvent !== 'undefined') {
          try {
            const ce = new CustomEvent('beecast-markerselect', {
              detail: { marker: marker },
              bubbles: true,
              composed: true,
              cancelable: true,
            });
            cancelled = !el.dispatchEvent(ce);
          } catch (_) {}
        }
        if (cancelled) return;
        // Seek only — chapter picks must not start playback (same as [ ] / ←→).
        self.seek(marker.time, 'marker');
        self.toggleChapters(false);
      };
    })(m));
    this.chaptersEl.appendChild(row);
  }
};

Player.prototype.syncChaptersUi = function (state) {
  if (this.chapBtn) this.chapBtn.hidden = !(state.markers && state.markers.length);
  if (this.chaptersEl && !this.chaptersEl.hidden) this.renderChapters(state);
};

Player.prototype.toggleSpeedMenu = function (force) {
  if (!this.speedMenuEl) return;
  const show = force != null ? force : this.speedMenuEl.hidden;
  if (show === !this.speedMenuEl.hidden) return;
  const self = this;
  if (show) {
    this.renderSpeedMenu();
    this.speedMenuEl.hidden = false;
    if (this.speedBtn) this.speedBtn.setAttribute('aria-expanded', 'true');
    this.speedAway = function () { self.toggleSpeedMenu(false); };
    document.addEventListener('click', this.speedAway);
  } else {
    this.speedMenuEl.hidden = true;
    if (this.speedBtn) {
      this.speedBtn.setAttribute('aria-expanded', 'false');
      try { this.speedBtn.focus({ preventScroll: true }); } catch (_) {}
    }
    if (this.speedAway) { document.removeEventListener('click', this.speedAway); this.speedAway = null; }
  }
};

Player.prototype.renderSpeedMenu = function () {
  if (!this.speedMenuEl) return;
  const self = this;
  const speed = this.controller.speed;
  this.speedMenuEl.innerHTML = '';
  for (let i = SPEEDS.length - 1; i >= 0; i--) {
    const v = SPEEDS[i];
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'sp-speedopt' + (v === speed ? ' sp-on' : '');
    b.setAttribute('role', 'menuitemradio');
    b.setAttribute('aria-checked', v === speed ? 'true' : 'false');
    b.textContent = String(v).replace(/\.0$/, '') + '×';
    b.addEventListener('click', (function (s) {
      return function (ev) {
        ev.stopPropagation();
        self.setSpeed(s, 'pointer');
        self.toggleSpeedMenu(false);
      };
    })(v));
    this.speedMenuEl.appendChild(b);
  }
};

Player.prototype.onKey = function (ev) {
  if (ev.metaKey || ev.ctrlKey || ev.altKey) return;
  // Escape closes menus first.
  if (ev.key === 'Escape') {
    if (this.speedMenuEl && !this.speedMenuEl.hidden) {
      this.toggleSpeedMenu(false);
      ev.preventDefault();
      return;
    }
    if (this.chaptersEl && !this.chaptersEl.hidden) {
      this.toggleChapters(false);
      ev.preventDefault();
      return;
    }
  }
  const k = ev.key;
  if (k === ' ') this.toggle('keyboard');
  else if (k === 'ArrowLeft') this.seek(this.getCurrentTime() - SEEK_STEP_SECS, 'keyboard');
  else if (k === 'ArrowRight') this.seek(this.getCurrentTime() + SEEK_STEP_SECS, 'keyboard');
  else if (k === '<' || k === ',') this.controller.cycleSpeed(-1, 'keyboard');
  else if (k === '>' || k === '.') this.controller.cycleSpeed(1, 'keyboard');
  else if (k === '[') this.controller.jumpMarker(-1, 'keyboard');
  else if (k === ']') this.controller.jumpMarker(1, 'keyboard');
  else if (k === 'c' || k === 'C') this.toggleChapters();
  else if (k === 'f' || k === 'F') this.toggleFullscreen();
  else return;
  ev.preventDefault();
  ev.stopPropagation();
};

// True when `mount`'s height does not depend on `box` — i.e. the embedding page gave the
// mount a definite height (%, vh, flex/grid stretch, …). A content-sized mount shrinks when
// the box does; vertical fit against that height is a ResizeObserver shrink ratchet.
Player.prototype.mountHeightIsDefinite = function (mount, box) {
  if (!mount || !box) return false;
  const prev = box.style.height;
  const before = mount.clientHeight;
  box.style.height = '0px';
  const after = mount.clientHeight;
  box.style.height = prev;
  return before > 0 && before === after;
};

Player.prototype.layout = function () {
  if (!this.fit || !this.root || this._layouting) return;
  this._layouting = true;
  try {
    const box = this.root.querySelector('.sp-screen-box');
    if (!box || !this.screenEl) return;
    this.screenEl.style.transform = '';
    this.screenEl.style.marginLeft = '';
    const rect = this.screenEl.getBoundingClientRect();
    const naturalW = rect.width, naturalH = rect.height;
    if (!(naturalW > 0 && naturalH > 0)) return;

    const fs = typeof document !== 'undefined' ? document.fullscreenElement : null;
    const rootFs = fs === this.root;
    const wrapFs = !!(this.fsEl && fs === this.fsEl);
    const bar = this.root.querySelector('.sp-bar');
    const barH = bar ? Math.max(bar.offsetHeight, Math.ceil(bar.getBoundingClientRect().height)) : 0;

    // Width budget: the screen pane, falling back to the mount/fullscreen host.
    let availW = box.clientWidth;
    if (!(availW > 0)) {
      const host = rootFs ? this.root : (wrapFs ? this.fsEl : this.root.parentNode);
      availW = host ? host.clientWidth : 0;
    }
    let scale = availW > 0 && naturalW > availW ? availW / naturalW : 1;

    // fit:'both' honors a definite mount height only. Fullscreen hosts are definite by
    // construction; a content-sized parent (scsh's live dashboard pane, etc.) must not
    // drive vertical scale — measure→scale→write→ResizeObserver would ratchet forever.
    if (this.fit === 'both') {
      let availH = 0;
      let definite = false;
      if (rootFs) {
        availH = this.root.clientHeight - barH;
        definite = true;
      } else if (wrapFs && this.fsEl) {
        availH = this.fsEl.clientHeight - barH;
        definite = true;
      } else if (this.root.parentNode) {
        const mount = this.root.parentNode;
        definite = this.mountHeightIsDefinite(mount, box);
        if (definite) availH = mount.clientHeight - barH;
      }
      if (definite && availH > 40 && naturalH * scale > availH) {
        scale = Math.min(scale, availH / naturalH);
      }
    }

    const displayH = naturalH * scale;
    const displayW = naturalW * scale;
    const transform = scale < 1 ? 'scale(' + scale + ')' : '';
    const height = displayH + 'px';
    const paneW = box.clientWidth || availW;
    const margin = paneW > displayW ? (paneW - displayW) / 2 + 'px' : '';
    // Idempotent: skip style writes that would only re-arm ResizeObserver.
    if (this._layoutScale === scale && this.screenEl.style.transform === transform &&
        box.style.height === height && this.screenEl.style.marginLeft === margin) {
      return;
    }
    this._layoutScale = scale;
    this.screenEl.style.transform = transform;
    // The layout box must match the DISPLAY size: scale() does not shrink layout, and a
    // taller box was what pushed the control bar off-screen in fullscreen.
    box.style.height = height;
    this.screenEl.style.marginLeft = margin;
  } finally {
    this._layouting = false;
  }
};

Player.prototype.toggleFullscreen = function () {
  const el = this.fsEl || this.root;
  if (!el) return;
  if (document.fullscreenElement === el) {
    if (document.exitFullscreen) document.exitFullscreen();
  } else if (el.requestFullscreen) {
    el.requestFullscreen();
  }
};

Player.prototype.play = function (origin) { this.controller.play(origin); };
Player.prototype.pause = function (origin) { this.controller.pause(origin); };
Player.prototype.toggle = function (origin) { this.controller.toggle(origin); };
Player.prototype.seek = function (t, origin) {
  this.controller.seek(t, { origin: origin || 'api' });
};
Player.prototype.setSpeed = function (v, origin) { this.controller.setSpeed(v, origin); };
Player.prototype.getCurrentTime = function () { return this.controller.getCurrentTime(); };
Player.prototype.getState = function () { return this.controller.getState(); };
Player.prototype.append = function (text) { this.controller.append(text); };
Player.prototype.setLive = function (on, origin) { this.controller.setLive(on, origin || 'api'); };
Player.prototype.subscribe = function (fn) { return this.controller.subscribe(fn); };

Player.prototype.dispose = function () {
  if (this.disposed) return;
  this.disposed = true;
  if (this.unsubscribe) { this.unsubscribe(); this.unsubscribe = null; }
  this.controller.dispose();
  if (this.speedAway) { document.removeEventListener('click', this.speedAway); this.speedAway = null; }
  if (this.resizeObs) { try { this.resizeObs.disconnect(); } catch (_) {} this.resizeObs = null; }
  if (this.fsHandler) {
    try { document.removeEventListener('fullscreenchange', this.fsHandler); } catch (_) {}
    this.fsHandler = null;
  }
  if (this.root && this.root.parentNode) this.root.parentNode.removeChild(this.root);
  this.root = null;
};

// ---- Web Component ---------------------------------------------------------------------
// Preferred browser integration. Light DOM so the embedding page's inlined PLAYER_CSS
// styles the controls (Shadow DOM would require shipping CSS inside the JS bundle).
// part attributes mark stable styling hooks for when an open shadow root is added later.
function registerComponent() {
  if (typeof customElements === 'undefined' || typeof HTMLElement === 'undefined') return;
  if (customElements.get('beecast-player')) return;

  class BeeCastPlayerElement extends HTMLElement {
    static get observedAttributes() {
      // Only the attributes that are live after mount; the rest are read at mount time.
      return ['fit', 'speed', 'theme'];
    }

    constructor() {
      super();
      this._player = null;
      this._pending = {};
      this._connected = false;
    }

    connectedCallback() {
      this._connected = true;
      this.style.display = this.style.display || 'block';
      if (!this._player) this._mountPlayer();
    }

    disconnectedCallback() {
      this._connected = false;
      if (this._player) {
        this._player.dispose();
        this._player = null;
      }
    }

    attributeChangedCallback(name, oldV, newV) {
      if (oldV === newV || !this._player) return;
      if (name === 'speed') this._player.setSpeed(Number(newV) || 1);
      if (name === 'fit') { this._player.fit = newV || null; this._player.layout(); }
      if (name === 'theme' && this._player.root) {
        if (newV) this._player.root.setAttribute('data-theme', newV);
        else this._player.root.removeAttribute('data-theme');
      }
    }

    _optsFromAttributes() {
      const opts = {};
      if (this.hasAttribute('fit')) opts.fit = this.getAttribute('fit');
      if (this.hasAttribute('autoplay')) opts.autoPlay = this.getAttribute('autoplay') !== 'false';
      if (this.hasAttribute('idle-time-limit')) {
        opts.idleTimeLimit = Number(this.getAttribute('idle-time-limit'));
      }
      if (this.hasAttribute('speed')) opts.speed = Number(this.getAttribute('speed'));
      if (this.hasAttribute('start-at')) opts.startAt = this.getAttribute('start-at');
      if (this.hasAttribute('controls')) {
        opts.controls = this.getAttribute('controls') === 'false' ? false : true;
      }
      if (this.hasAttribute('accessibility')) {
        opts.accessibility = this.getAttribute('accessibility');
      }
      if (this._pending.markers) opts.markers = this._pending.markers;
      if (this._pending.speed != null) opts.speed = this._pending.speed;
      if (this._pending.startAt != null) opts.startAt = this._pending.startAt;
      if (this._pending.fit != null) opts.fit = this._pending.fit;
      if (this._pending.controls != null) opts.controls = this._pending.controls;
      if (this._pending.autoPlay != null) opts.autoPlay = this._pending.autoPlay;
      if (this._pending.idleTimeLimit != null) opts.idleTimeLimit = this._pending.idleTimeLimit;
      if (this._pending.fullscreenEl) opts.fullscreenEl = this._pending.fullscreenEl;
      if (this._pending.clock) opts.clock = this._pending.clock;
      if (this._pending.accessibility) opts.accessibility = this._pending.accessibility;
      return opts;
    }

    _mountPlayer() {
      while (this.firstChild) this.removeChild(this.firstChild);
      const data = this._pending.cast != null ? this._pending.cast
        : (this._pending.data != null ? this._pending.data : '');
      const opts = this._optsFromAttributes();
      const player = new Player({ data: data, source: this._pending.source }, this, opts);
      player.eventTarget = this;
      this._player = player;
      const theme = this.getAttribute('theme') || this._pending.theme;
      if (theme && player.root) player.root.setAttribute('data-theme', theme);
    }

    load(opts) {
      opts = opts || {};
      if (opts.cast != null) this._pending.cast = opts.cast;
      if (opts.data != null) this._pending.cast = opts.data;
      if (opts.metadata) this._pending.metadata = opts.metadata;
      if (opts.markers) this._pending.markers = opts.markers;
      if (opts.source) this._pending.source = opts.source;
      if (opts.speed != null) this._pending.speed = opts.speed;
      if (opts.startAt != null) this._pending.startAt = opts.startAt;
      if (opts.autoPlay != null) this._pending.autoPlay = opts.autoPlay;
      if (!this._connected) return;
      if (this._player) {
        this._player.controller.load({
          data: this._pending.cast,
          markers: this._pending.markers,
          startAt: this._pending.startAt,
          autoPlay: this._pending.autoPlay,
        });
        if (opts.speed != null) this._player.setSpeed(opts.speed);
      } else {
        this._mountPlayer();
      }
    }

    play() { if (this._player) this._player.play('api'); }
    pause() { if (this._player) this._player.pause('api'); }
    toggle() { if (this._player) this._player.toggle('api'); }
    seek(t) { if (this._player) this._player.seek(t, 'api'); }
    setSpeed(v) { if (this._player) this._player.setSpeed(v, 'api'); }
    append(t) { if (this._player) this._player.append(t); }
    getCurrentTime() { return this._player ? this._player.getCurrentTime() : 0; }
    dispose() {
      if (this._player) { this._player.dispose(); this._player = null; }
    }

    get state() { return this._player ? publicState(this._player.getState()) : null; }
    get cast() { return this._pending.cast; }
    set cast(v) { this._pending.cast = v; if (this._player) this.load({ cast: v }); }
    get markers() { return this._pending.markers; }
    set markers(v) {
      this._pending.markers = v;
      if (this._player) this._player.controller.setMarkers(v);
    }
    get speed() {
      return this._player ? this._player.controller.speed : Number(this.getAttribute('speed')) || 1;
    }
    set speed(v) { this._pending.speed = v; if (this._player) this._player.setSpeed(v); }
  }

  try {
    customElements.define('beecast-player', BeeCastPlayerElement);
    root.BeeCastPlayerElement = BeeCastPlayerElement;
  } catch (_) {
    // Legacy create() still works without custom elements.
  }
}

registerComponent();

// ---- public API ------------------------------------------------------------------------
// BeeCastPlayer.create remains the compatibility factory: a thin Player over the controller.
// New integrations should prefer <beecast-player> or BeeCastController directly.
root.BeeCastPlayer = {
  create: function (src, mount, opts) { return new Player(src, mount, opts); },
  Player: Player,
  elementName: 'beecast-player',
  SPEEDS: SPEEDS,
  // Documented public surface snapshot for compatibility fixtures (Phase 0).
  publicMethods: [
    'create', 'play', 'pause', 'toggle', 'seek', 'getCurrentTime', 'append', 'dispose',
    'setSpeed', 'getState', 'subscribe',
  ],
  supportedCssVariables: [
    '--beecast-color-surface',
    '--beecast-color-surface-raised',
    '--beecast-color-text',
    '--beecast-color-text-muted',
    '--beecast-color-accent',
    '--beecast-color-focus',
    '--beecast-color-marker',
    '--beecast-color-error',
    '--beecast-control-height',
    '--beecast-radius',
    '--beecast-font-ui',
    '--beecast-font-terminal',
    '--sp-bg', '--sp-fg',
    '--sp-c0', '--sp-c1', '--sp-c2', '--sp-c3', '--sp-c4', '--sp-c5', '--sp-c6', '--sp-c7',
    '--sp-c8', '--sp-c9', '--sp-c10', '--sp-c11', '--sp-c12', '--sp-c13', '--sp-c14', '--sp-c15',
  ],
  // Readable during migration but not public API — do not depend on these.
  nonPublicFields: ['playing', 'pacedPos', 'eventIdx', 'cast', 'term', 'pacing', 'raf'],
};

})(typeof window !== 'undefined' ? window : globalThis);
