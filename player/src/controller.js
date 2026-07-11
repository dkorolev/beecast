// beecast-player: headless playback controller (see the crate README).
//
// Owns cast state, the VT terminal, pacing, the playback clock, markers, and
// subscribers. No DOM, no CSS. Injectable scheduling makes tests deterministic.
// Clean-room implementation, MIT like the rest of beecast.
'use strict';
(function (root) {

const VT = root.BeeCastVT;
const SPEEDS = [0.5, 1, 1.5, 2, 3, 5];

// ---- markers ---------------------------------------------------------------------------
// Public marker model (Phase 5). Tuples [time, label] remain accepted and are normalized.
function normalizeMarker(raw, source, index) {
  if (raw == null) return null;
  if (Array.isArray(raw)) {
    return {
      id: 'ext-' + index + '-' + (Number(raw[0]) || 0),
      time: Number(raw[0]) || 0,
      type: 'chapter',
      label: String(raw[1] || ''),
      source: source || 'sidecar',
    };
  }
  if (typeof raw === 'object') {
    const time = Number(raw.time != null ? raw.time : raw.t) || 0;
    const id = raw.id != null ? String(raw.id) : (source || 'm') + '-' + index + '-' + time;
    return {
      id: id,
      time: time,
      type: raw.type != null ? String(raw.type) : 'chapter',
      label: String(raw.label != null ? raw.label : raw.title || ''),
      description: raw.description != null ? String(raw.description) : undefined,
      color: raw.color != null ? String(raw.color) : undefined,
      source: raw.source != null ? String(raw.source) : (source || 'integration'),
      data: raw.data,
    };
  }
  return null;
}

function normalizeMarkers(list, source) {
  const out = [];
  const arr = list || [];
  for (let i = 0; i < arr.length; i++) {
    const m = normalizeMarker(arr[i], source, i);
    if (m) out.push(m);
  }
  return out;
}

function sortMarkers(markers) {
  markers.sort(function (a, b) {
    if (a.time !== b.time) return a.time - b.time;
    // Stable by source priority then id so duplicate timestamps stay deterministic.
    const sa = a.source || '', sb = b.source || '';
    if (sa !== sb) return sa < sb ? -1 : 1;
    return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
  });
  return markers;
}

// ---- clock -----------------------------------------------------------------------------
function defaultClock() {
  return {
    now: function () {
      return (typeof performance !== 'undefined' && performance.now)
        ? performance.now()
        : Date.now();
    },
    requestAnimationFrame: function (cb) {
      if (typeof requestAnimationFrame !== 'undefined') return requestAnimationFrame(cb);
      return setTimeout(function () { cb(Date.now()); }, 16);
    },
    cancelAnimationFrame: function (id) {
      if (typeof cancelAnimationFrame !== 'undefined') cancelAnimationFrame(id);
      else clearTimeout(id);
    },
  };
}

// ---- controller ------------------------------------------------------------------------
function Controller(opts) {
  opts = opts || {};
  this.clock = opts.clock || defaultClock();
  this.speedList = SPEEDS.slice();
  this.listeners = [];
  this.disposed = false;
  this.raf = null;
  this.lastTick = null;
  this.emitScheduled = false;
  this._pendingTimeupdate = null;
  this._castMarkersFrom = 0;

  const src = resolveSource(opts);
  const cast = VT.parseCast(src);
  this.cast = cast;
  this.term = new VT.Term(cast.cols, cast.rows);
  this._terminalSnapshot = null;
  this._terminalSnapshotDirty = true;
  this.pacing = VT.buildPacing(cast.events, cast.duration, opts.idleTimeLimit);
  this.idleTimeLimit = opts.idleTimeLimit == null ? null : opts.idleTimeLimit;

  this.externalMarkers = normalizeMarkers(opts.markers, 'sidecar');
  this.castMarkers = [];
  this.markers = [];
  this.absorbCastMarkers(0);

  this.speed = this.speedList.indexOf(Number(opts.speed)) >= 0 ? Number(opts.speed) : 1;
  this.status = 'idle'; // idle | playing | paused | ended
  this.pacedPos = 0;
  this.eventIdx = 0;
  this.atLiveEdge = cast.duration <= 0;
  this.live = false; // declared-live mode (see setLive), distinct from the positional atLiveEdge
  this.returnToLiveAtEnd = false;

  this.applyEventsUpTo(0);
  if (opts.startAt != null) this.seek(parseTime(opts.startAt), { origin: 'api', silent: true });
  // Initial status after optional startAt.
  if (this.pacedPos >= this.pacing.pacedDuration && this.pacing.pacedDuration > 0) {
    this.status = 'ended';
  } else if (this.pacedPos > 0) {
    this.status = 'paused';
  }
  this.syncLiveEdge();
}

function resolveSource(opts) {
  // Explicit source adapters (Phase 6). The default is inline text; network is never implied.
  if (opts.source) {
    const s = opts.source;
    if (s.type === 'text') return String(s.data || '');
    if (s.type === 'custom') return ''; // custom sources start empty and append
    // file / stream need the DOM/fetch layer — reject with a stable error code via throw
    // only when actively used; here treat unknown as empty and let callers use load().
    if (s.type === 'file' || s.type === 'stream') return '';
  }
  if (opts.data != null) return String(opts.data);
  return '';
}

function parseTime(v) {
  if (typeof v === 'number' && isFinite(v)) return v;
  const m = /^(\d+):(\d{1,2})$/.exec(String(v || '').trim());
  if (m) return Number(m[1]) * 60 + Number(m[2]);
  const n = parseFloat(v);
  return isFinite(n) ? n : 0;
}

Controller.create = function (opts) {
  return new Controller(opts);
};

Controller.SPEEDS = SPEEDS;

Controller.prototype.absorbCastMarkers = function (fromIdx) {
  for (let i = fromIdx; i < this.cast.events.length; i++) {
    const ev = this.cast.events[i];
    if (ev.type === 'm') {
      this.castMarkers.push({
        id: 'cast-' + i + '-' + ev.t,
        time: ev.t,
        type: 'chapter',
        label: String(ev.data || ''),
        source: 'cast',
      });
    }
  }
  this.rebuildMarkers();
};

Controller.prototype.rebuildMarkers = function () {
  this.markers = sortMarkers(this.externalMarkers.concat(this.castMarkers));
};

// Integration markers without mutating cast-sourced ones.
Controller.prototype.setMarkers = function (list) {
  if (this.disposed) return;
  this.externalMarkers = normalizeMarkers(list, 'integration');
  this.rebuildMarkers();
  this.emit({ type: 'markerchange', origin: 'api' });
};

Controller.prototype.applyEventsUpTo = function (t) {
  const evs = this.cast.events;
  if (this.eventIdx > 0 && evs[this.eventIdx - 1].t > t) {
    this.term = new VT.Term(this.cast.cols, this.cast.rows);
    this.eventIdx = 0;
    this._terminalSnapshotDirty = true;
  }
  let applied = false;
  let resized = false;
  while (this.eventIdx < evs.length && evs[this.eventIdx].t <= t) {
    const ev = evs[this.eventIdx++];
    if (ev.type === 'o') this.term.write(ev.data);
    else if (ev.type === 'r') {
      const m = /^(\d+)x(\d+)$/.exec(ev.data.trim());
      if (m) { this.term.resize(Number(m[1]), Number(m[2])); resized = true; }
    }
    applied = true;
  }
  if (applied || resized) this._terminalSnapshotDirty = true;
  return { applied: applied, resized: resized };
};

Controller.prototype.snapshotTerminal = function () {
  if (!this._terminalSnapshotDirty && this._terminalSnapshot) return this._terminalSnapshot;
  const snap = this.term.snapshot();
  // Defensive copy so subscribers cannot mutate internal term state.
  const rows = [];
  for (let y = 0; y < snap.rows.length; y++) {
    const line = [];
    for (let i = 0; i < snap.rows[y].length; i++) {
      const r = snap.rows[y][i];
      line.push({ text: r.text, fg: r.fg, bg: r.bg, attrs: r.attrs });
    }
    rows.push(line);
  }
  this._terminalSnapshot = {
    rows: rows,
    cursor: {
      x: snap.cursor.x,
      y: snap.cursor.y,
      visible: snap.cursor.visible,
    },
  };
  this._terminalSnapshotDirty = false;
  return this._terminalSnapshot;
};

Controller.prototype.getCurrentTime = function () {
  return VT.mapTime(this.pacing.paced, this.pacing.rec, this.pacedPos);
};

Controller.prototype.syncLiveEdge = function () {
  const edge = this.pacing.pacedDuration <= 0
    || this.pacedPos >= this.pacing.pacedDuration - 1e-9;
  this.atLiveEdge = edge;
};

Controller.prototype.getState = function () {
  const t = this.getCurrentTime();
  const markers = this.markers.map(function (m) {
    return {
      id: m.id,
      time: m.time,
      type: m.type,
      label: m.label,
      description: m.description,
      color: m.color,
      source: m.source,
      data: m.data,
    };
  });
  return {
    status: this.status,
    currentTime: t,
    duration: this.cast.duration,
    speed: this.speed,
    atLiveEdge: this.atLiveEdge,
    live: this.live,
    canAppend: this.cast.version !== 1,
    markers: markers,
    terminal: this.snapshotTerminal(),
    dimensions: { columns: this.term.cols, rows: this.term.rows },
  };
};

// Live mode: the EMBEDDER declares the recording is still being produced (it knows; the
// controller only sees text). The playhead parks at the growing edge — every append
// renders immediately — and the view pins the seek bar full-width in the live color, so
// the bar reads "now", not a position that jitters as the duration grows. Any explicit
// rewind — a seek before the edge, or play() (which would replay from the top) — drops
// live mode: the viewer chose a position, and the bar must tell the truth again.
Controller.prototype.setLive = function (on, origin, preserveReturn) {
  on = !!on;
  if (this.disposed) return;
  if (!preserveReturn) this.returnToLiveAtEnd = on;
  if (this.live === on) return;
  this.live = on;
  if (on) {
    this.pause(origin || 'api');
    this.pacedPos = this.pacing.pacedDuration;
    this.applyEventsUpTo(this.getCurrentTime());
    this.syncLiveEdge();
  }
  this.emit({ type: 'livechange', origin: origin || 'api', live: on });
};

Controller.prototype.subscribe = function (listener) {
  if (typeof listener !== 'function') return function () {};
  const self = this;
  if (this.disposed) {
    // Disposed controllers still deliver one final state snapshot, then never again.
    try { listener(this.getState(), { type: 'disposed' }); } catch (_) {}
    return function () {};
  }
  this.listeners.push(listener);
  try { listener(this.getState(), { type: 'ready' }); } catch (_) {}
  let active = true;
  return function unsubscribe() {
    if (!active) return;
    active = false;
    const i = self.listeners.indexOf(listener);
    if (i >= 0) self.listeners.splice(i, 1);
  };
};

Controller.prototype.emit = function (meta) {
  if (this.disposed) return;
  meta = meta || { type: 'change' };
  // Discrete events (play, seek, speedchange, durationchange, …) always deliver
  // immediately: last-write-wins coalescing must never drop them. Only the
  // high-frequency timeupdate stream coalesces to animation-frame rate, OR-ing
  // the terminalChanged/resized flags of any skipped frames.
  if (meta.type !== 'timeupdate') {
    this.deliver(meta);
    return;
  }
  const pending = this._pendingTimeupdate;
  if (pending) {
    pending.terminalChanged = pending.terminalChanged || meta.terminalChanged;
    pending.resized = pending.resized || meta.resized;
  } else {
    this._pendingTimeupdate = meta;
  }
  if (this.emitScheduled) return;
  this.emitScheduled = true;
  const self = this;
  this.clock.requestAnimationFrame(function () {
    self.emitScheduled = false;
    const m = self._pendingTimeupdate;
    self._pendingTimeupdate = null;
    if (m && !self.disposed) self.deliver(m);
  });
};

Controller.prototype.deliver = function (meta) {
  if (!this.listeners.length) return;
  const state = this.getState();
  const list = this.listeners.slice();
  for (let i = 0; i < list.length; i++) {
    try { list[i](state, meta || { type: 'change' }); } catch (_) {}
  }
};

Controller.prototype.play = function (origin) {
  if (this.disposed) return;
  if (this.status === 'playing') return;
  // Playing from live mode is a rewind (parked at the edge, play replays from the top).
  if (this.live) this.setLive(false, origin || 'api', true);
  if (this.pacedPos >= this.pacing.pacedDuration) {
    this.pacedPos = 0;
    this.applyEventsUpTo(0);
  }
  this.status = 'playing';
  this.lastTick = null;
  this.syncLiveEdge();
  this.emit({ type: 'play', origin: origin || 'api' });
  const self = this;
  this.raf = this.clock.requestAnimationFrame(function (ts) { self.tick(ts); });
};

Controller.prototype.pause = function (origin) {
  if (this.disposed) return;
  if (this.status !== 'playing') {
    if (this.status === 'idle' && this.pacedPos > 0) this.status = 'paused';
    return;
  }
  this.status = this.pacedPos >= this.pacing.pacedDuration && this.pacing.pacedDuration > 0
    ? 'ended'
    : 'paused';
  if (this.raf != null) {
    this.clock.cancelAnimationFrame(this.raf);
    this.raf = null;
  }
  this.lastTick = null;
  this.syncLiveEdge();
  this.emit({ type: this.status === 'ended' ? 'ended' : 'pause', origin: origin || 'api' });
};

Controller.prototype.toggle = function (origin) {
  if (this.status === 'playing') this.pause(origin);
  else this.play(origin);
};

Controller.prototype.tick = function (nowMs) {
  if (this.disposed || this.status !== 'playing') return;
  const dt = this.lastTick == null ? 0 : (nowMs - this.lastTick) / 1000;
  this.lastTick = nowMs;
  this.pacedPos = Math.min(this.pacing.pacedDuration, this.pacedPos + dt * this.speed);
  const result = this.applyEventsUpTo(this.getCurrentTime());
  this.syncLiveEdge();
  if (this.pacedPos >= this.pacing.pacedDuration) {
    this.status = 'ended';
    if (this.raf != null) {
      this.clock.cancelAnimationFrame(this.raf);
      this.raf = null;
    }
    if (this.returnToLiveAtEnd) this.setLive(true, 'source');
    this.emit({ type: 'ended', origin: 'source', resized: result.resized });
    return;
  }
  this.emit({
    type: 'timeupdate',
    origin: 'source',
    resized: result.resized,
    terminalChanged: result.applied,
  });
  const self = this;
  this.raf = this.clock.requestAnimationFrame(function (ts) { self.tick(ts); });
};

Controller.prototype.seek = function (t, opts) {
  if (this.disposed) return;
  opts = opts || {};
  t = Math.min(this.cast.duration, Math.max(0, parseTime(t)));
  this.pacedPos = VT.mapTime(this.pacing.rec, this.pacing.paced, t);
  this.applyEventsUpTo(t);
  if (this.status === 'ended' && t < this.cast.duration) this.status = 'paused';
  if (this.status === 'idle' && t > 0) this.status = 'paused';
  if (t <= 0 && this.status !== 'playing') this.status = 'idle';
  this.syncLiveEdge();
  // Seeking away from the edge is the viewer choosing a position: live mode ends. A seek
  // TO the edge (e.g. seek(Infinity) while entering live) keeps it.
  if (this.live && t < this.cast.duration - 0.25) {
    this.setLive(false, opts.origin || 'api', true);
  }
  if (!opts.silent) this.emit({ type: 'seek', origin: opts.origin || 'api', time: t });
};

Controller.prototype.setSpeed = function (v, origin) {
  if (this.disposed) return;
  const n = Number(v);
  if (this.speedList.indexOf(n) < 0) return;
  if (this.speed === n) return;
  this.speed = n;
  this.emit({ type: 'speedchange', origin: origin || 'api', speed: n });
};

Controller.prototype.cycleSpeed = function (dir, origin) {
  const i = this.speedList.indexOf(this.speed);
  const next = this.speedList[Math.min(this.speedList.length - 1, Math.max(0, (i < 0 ? 1 : i) + dir))];
  this.setSpeed(next, origin);
};

// Returns the seek target so the UI can name it: the marker jumped to, a synthetic
// `{ time: 0, label: '' }` when [ falls back to the start, or null when nothing moved.
Controller.prototype.jumpMarker = function (dir, origin) {
  if (!this.markers.length) return null;
  const now = this.getCurrentTime();
  let target = null;
  if (dir > 0) {
    for (let i = 0; i < this.markers.length; i++) {
      if (this.markers[i].time > now + 0.25) { target = this.markers[i]; break; }
    }
  } else {
    for (let i = 0; i < this.markers.length; i++) {
      if (this.markers[i].time < now - 0.25) target = this.markers[i];
    }
    if (target == null) {
      this.seek(0, { origin: origin || 'marker' });
      return { time: 0, label: '' };
    }
  }
  // Seek only — leave playing/paused alone. [ ] / chapter jumps must not autoplay.
  if (target != null) this.seek(target.time, { origin: origin || 'marker' });
  return target;
};

// Live-follow append (v2/v3). Same positional tail -f policy as before.
Controller.prototype.append = function (text) {
  if (this.disposed) return;
  const wasPlaying = this.status === 'playing';
  // Declared-live pins unconditionally; otherwise the positional tail-f rule applies.
  const atEdge = this.live || (!wasPlaying && this.pacedPos >= this.pacing.pacedDuration - 1e-9);
  const fromIdx = this.cast.events.length;
  const prevDuration = this.cast.duration;
  VT.appendCast(this.cast, text);
  if (this.cast.events.length === fromIdx && this.cast.duration === prevDuration) return;
  VT.extendPacing(this.pacing, this.cast.events, fromIdx, this.cast.duration);
  this.absorbCastMarkers(fromIdx);
  if (this.status === 'ended' && this.cast.duration > prevDuration) {
    // Longer recording: ended at the old edge becomes paused-at-edge readiness for follow.
    if (atEdge) this.status = 'paused';
  }
  if (atEdge) {
    this.pacedPos = this.pacing.pacedDuration;
    this.applyEventsUpTo(this.getCurrentTime());
  }
  this.syncLiveEdge();
  this.emit({
    type: 'durationchange',
    origin: 'source',
    duration: this.cast.duration,
    atEdge: atEdge,
  });
};

// Load replacement cast text (full reparse). Used by the component's load().
Controller.prototype.load = function (opts) {
  if (this.disposed) return;
  opts = opts || {};
  const wasPlaying = this.status === 'playing';
  if (this.raf != null) {
    this.clock.cancelAnimationFrame(this.raf);
    this.raf = null;
  }
  const data = opts.data != null ? String(opts.data)
    : (opts.source && opts.source.type === 'text' ? String(opts.source.data || '') : '');
  this.cast = VT.parseCast(data);
  this.term = new VT.Term(this.cast.cols, this.cast.rows);
  this._terminalSnapshot = null;
  this._terminalSnapshotDirty = true;
  this.pacing = VT.buildPacing(this.cast.events, this.cast.duration, this.idleTimeLimit);
  this.castMarkers = [];
  if (opts.markers) this.externalMarkers = normalizeMarkers(opts.markers, 'sidecar');
  this.absorbCastMarkers(0);
  this.pacedPos = 0;
  this.eventIdx = 0;
  this.status = 'idle';
  this.applyEventsUpTo(0);
  if (opts.startAt != null) this.seek(parseTime(opts.startAt), { silent: true });
  this.syncLiveEdge();
  this.emit({ type: 'ready', origin: 'api' });
  if (opts.autoPlay || wasPlaying && opts.resume) this.play('api');
};

Controller.prototype.dispose = function () {
  if (this.disposed) return;
  this.disposed = true;
  if (this.raf != null) {
    this.clock.cancelAnimationFrame(this.raf);
    this.raf = null;
  }
  this.listeners = [];
  this.status = 'idle';
};

// Compatibility helpers for the legacy Player view.
Controller.prototype.isPlaying = function () {
  return this.status === 'playing';
};

root.BeeCastController = {
  create: Controller.create,
  SPEEDS: SPEEDS,
  normalizeMarkers: normalizeMarkers,
  parseTime: parseTime,
  // Internal constructor for advanced tests.
  Controller: Controller,
};

})(typeof window !== 'undefined' ? window : globalThis);
