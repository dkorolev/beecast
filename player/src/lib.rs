//! The first-party beecast player as a crate: clean-room, dependency-free
//! asciicast (v1/v2/v3) player and VT100/xterm-subset terminal emulator. Consumers inline
//! the two constants whole — a `<script>` element and a `<style>` element — and get the
//! full player: parsing, emulation, headless controller, default controls, Web Component,
//! chapter markers, keyboard control, and live-follow `append` for recordings that are
//! still growing.
//!
//! The JS globals are `BeeCastVT` (DOM-free core), `BeeCastController` (headless playback),
//! and `BeeCastPlayer` (DOM factory + `<beecast-player>`); this crate is the component's
//! canonical home.

/// The player bundle: `vt.js` (core) + `controller.js` (headless playback) + `player.js`
/// (DOM view, Web Component, legacy factory). Inline it in one `<script>` element; it
/// performs no network requests and loads no workers.
pub const PLAYER_JS: &str =
  concat!(include_str!("vt.js"), "\n", include_str!("controller.js"), "\n", include_str!("player.js"),);

/// The player chrome and terminal palette. Semantic `--beecast-*` tokens are the stable
/// theming surface; `--sp-*` is the terminal palette. Nothing is fetched (no fonts, no images).
pub const PLAYER_CSS: &str = include_str!("player.css");

#[cfg(test)]
mod tests {
  use super::*;

  /// Every embedder's self-containment claim depends on the bundle itself: it must not
  /// contain a literal `</script`, must not reference workers, and its CSS must not pull
  /// fonts or images. Asserted here so a player change that breaks any of it fails loudly.
  /// And the bundle must be the first-party clean-room player — no third-party code, and
  /// therefore no third-party license, rides into any embedding page.
  #[test]
  fn player_bundle_is_inline_safe_and_first_party() {
    assert!(!PLAYER_JS.contains("</script"), "bundle would terminate an inline <script>");
    assert!(!PLAYER_JS.contains("<!--"), "bundle would enter the script double-escaped state");
    assert!(!PLAYER_JS.to_lowercase().contains("worker"), "bundle must not load a worker sidecar");
    assert!(!PLAYER_CSS.contains("url("), "player CSS must not fetch fonts/images");
    assert!(PLAYER_JS.contains("BeeCastPlayer") && PLAYER_JS.contains("Clean-room implementation"));
    assert!(PLAYER_JS.contains("BeeCastController"));
    assert!(PLAYER_JS.contains("beecast-player"));
    for banned in ["asciinema-player", "AsciinemaPlayer", "@license", "Apache"] {
      assert!(!PLAYER_JS.contains(banned) && !PLAYER_CSS.contains(banned), "third-party marker '{banned}'");
    }
  }

  /// The center play overlay must appear whenever playback is not running (and not in
  /// declared-live mode), and marker jumps must seek without calling play.
  #[test]
  fn overlay_and_marker_jumps_do_not_autoplay() {
    assert!(
      PLAYER_JS.contains("!state.live && state.status !== 'playing' && state.duration > 0"),
      "overlay must show when not playing and not live, not only at t = 0"
    );
    assert!(!PLAYER_JS.contains("currentTime <= 1e-9 && state.duration"), "the t≈0-only overlay gate must stay gone");
    // jumpMarker used to end with play(origin); chapter rows did too.
    assert!(
      !PLAYER_JS.contains("this.play(origin || 'marker')") && !PLAYER_JS.contains("self.play('marker')"),
      "marker/chapter navigation must not force play"
    );
    assert!(
      PLAYER_JS.contains("sp-bigplay-icon") && PLAYER_JS.contains("viewBox=\"0 0 80 80\""),
      "play overlay must be an equal-height SVG |> (bar + chevron), not monospace text"
    );
    assert!(
      !PLAYER_JS.contains("'|&gt;'") && !PLAYER_JS.contains("\"|&gt;\""),
      "monospace |> text glyph must stay gone"
    );
    assert!(!PLAYER_JS.contains("▄█"), "the block-character triangle glyph must stay gone");
    assert!(
      PLAYER_JS.contains("sp-live") && PLAYER_CSS.contains(".sp-live"),
      "Live control lives in the player toolbar when controls.live is enabled"
    );
  }

  /// `fit: 'both'` must never vertically scale against a content-sized mount: that path
  /// was a ResizeObserver shrink ratchet (scsh's live dashboard). The definite-height
  /// probe and the absence of the old `availH - 4` trigger pin the fix in the bundle.
  #[test]
  fn fit_both_refuses_the_content_sized_shrink_ratchet() {
    assert!(
      PLAYER_JS.contains("mountHeightIsDefinite"),
      "layout must probe whether the mount's height is independent of the screen box"
    );
    assert!(
      PLAYER_JS.contains("before > 0 && before === after"),
      "definite-height probe must compare mount clientHeight before/after collapsing the box"
    );
    assert!(
      !PLAYER_JS.contains("availH - 4"),
      "the availH-4 trigger forced another shrink on every ResizeObserver tick"
    );
    assert!(
      PLAYER_JS.contains("mount && mount.clientHeight > 0") && PLAYER_JS.contains("wrapFs"),
      "wrap-fullscreen layout must prefer the mount height over the outer fullscreen host"
    );
  }

  /// Phase 0: the public surface must not shrink without an intentional change.
  #[test]
  fn public_api_surface_is_documented() {
    for key in [
      "BeeCastVT",
      "BeeCastController",
      "BeeCastPlayer",
      "parseCast",
      "appendCast",
      "buildPacing",
      "extendPacing",
      "mapTime",
      "create",
      "getState",
      "subscribe",
      "setSpeed",
      "publicMethods",
      "supportedCssVariables",
      "nonPublicFields",
    ] {
      assert!(PLAYER_JS.contains(key), "public surface missing {key}");
    }
    for token in
      ["--beecast-color-surface", "--beecast-color-accent", "--beecast-color-focus", "--beecast-font-terminal"]
    {
      assert!(PLAYER_CSS.contains(token), "theme token missing {token}");
    }
  }

  /// Behavior tests for the DOM-free core and headless controller, run under Node.
  /// Skips silently when `node` is not installed — the structural assertions above still gate the bundle.
  #[test]
  fn vt_core_node_selftest() {
    let dir = std::env::temp_dir().join(format!("beecast-vt-selftest-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("player.js");
    std::fs::write(&bundle, PLAYER_JS).unwrap();
    let script = r#"
const assert = require('assert');
require(process.argv[2]);
const VT = globalThis.BeeCastVT;
const C = globalThis.BeeCastController;
const P = globalThis.BeeCastPlayer;

// ---- Phase 0: public keys --------------------------------------------------------------
for (const k of ['parseCast','appendCast','buildPacing','extendPacing','mapTime','Term']) {
  assert.strictEqual(typeof VT[k], 'function', 'BeeCastVT.' + k);
}
assert.strictEqual(typeof C.create, 'function');
assert.strictEqual(typeof P.create, 'function');
assert.ok(Array.isArray(P.publicMethods));
for (const m of ['create','play','pause','toggle','seek','getCurrentTime','append','dispose']) {
  assert.ok(P.publicMethods.includes(m), 'publicMethods missing ' + m);
}
assert.ok(P.nonPublicFields.includes('playing'));
assert.ok(P.nonPublicFields.includes('pacedPos'));
assert.ok(P.supportedCssVariables.includes('--beecast-color-accent'));

// ---- VT core (existing coverage) -------------------------------------------------------
let c = VT.parseCast('{"version":3,"term":{"cols":10,"rows":3}}\n# note\n[0.5,"o","hi"]\n[0.5,"m","chapter"]\n[1.0,"r","20x5"]\n');
assert.strictEqual(c.cols, 10); assert.strictEqual(c.rows, 3);
assert.strictEqual(c.events.length, 3);
assert.strictEqual(c.duration, 2);

c = VT.parseCast('{"version":2,"width":80,"height":24}\n[0.5,"o","a"]\n[2.0,"o","b"]\n');
assert.strictEqual(c.duration, 2); assert.strictEqual(c.events[1].t, 2);

c = VT.parseCast('{"version":1,"width":5,"height":2,"stdout":[[0.1,"x"],[0.2,"y"]]}');
assert.strictEqual(c.cols, 5); assert.strictEqual(c.events.length, 2);

c = VT.parseCast('{"version":3,"term":{"cols":10,"rows":3}}\n[1.0,"o","a"]\n');
assert.strictEqual(VT.appendCast(c, '[0.5,"o",'), 0);
assert.strictEqual(c.duration, 1);
assert.strictEqual(VT.appendCast(c, '"b"]\n[0.5,"m","x"]\n'), 2);
assert.strictEqual(c.duration, 2);
assert.strictEqual(c.events.length, 3);
assert.strictEqual(c.events[1].t, 1.5);

c = VT.parseCast('{"version":2,"width":80,"height":24}\n[1.0,"o","a"]\n');
VT.appendCast(c, '# noise\n{"version":2}\n[3.0,"o","b"]\n');
assert.strictEqual(c.duration, 3); assert.strictEqual(c.events[1].t, 3);

c = VT.parseCast('{"version":3,"term":{"cols":4,"rows":1}}\n[1.0,"o","hi"]\n[2.0,"o');
assert.strictEqual(c.events.length, 1);
VT.appendCast(c, '","yo"]\n');
assert.strictEqual(c.events.length, 2); assert.strictEqual(c.duration, 3);

c = VT.parseCast('{"version":1,"width":5,"height":2,"stdout":[[0.1,"x"]]}');
assert.strictEqual(VT.appendCast(c, '[1.0,"o","y"]\n'), 0);

const ev = [{t:1},{t:2}];
const pacing = VT.buildPacing(ev, 2, null);
ev.push({t:10});
VT.extendPacing(pacing, ev, 2, 10);
assert.strictEqual(pacing.pacedDuration, 10);
assert.strictEqual(VT.mapTime(pacing.rec, pacing.paced, 10), 10);
const limited = VT.buildPacing([{t:1}], 1, 2);
VT.extendPacing(limited, [{t:1},{t:9}], 1, 9);
assert.strictEqual(limited.pacedDuration, 3);
assert.strictEqual(VT.mapTime(limited.paced, limited.rec, 3), 9);

let t = new VT.Term(10, 3);
t.write('hello\r\nworld');
assert.deepStrictEqual(t.textLines(), ['hello', 'world', '']);
t.write('\x1b[1;3Hga');
assert.strictEqual(t.textLines()[0], 'hegao');
t.write('\x1b[2J');
assert.deepStrictEqual(t.textLines(), ['', '', '']);

t = new VT.Term(10, 1);
t.write('\x1b[31mred\x1b[0m ok');
const runs = t.snapshot().rows[0];
assert.strictEqual(runs[0].text, 'red'); assert.strictEqual(runs[0].fg, 1);
t = new VT.Term(4, 1);
t.write('\x1b[38;5;196mX\x1b[38;2;1;2;3mY');
assert.strictEqual(t.snapshot().rows[0][0].fg, 196);
assert.strictEqual(t.snapshot().rows[0][1].fg, '#010203');

t = new VT.Term(3, 2);
t.write('abc');
assert.strictEqual(t.snapshot().cursor.y, 0);
t.write('d');
assert.deepStrictEqual(t.textLines(), ['abc', 'd']);

t = new VT.Term(5, 4);
t.write('aa\r\nbb\r\ncc\r\ndd');
t.write('\x1b[2;3r\x1b[3;1H\n');
assert.strictEqual(t.textLines()[0], 'aa');
assert.strictEqual(t.textLines()[1], 'cc');
assert.strictEqual(t.textLines()[3], 'dd');

t = new VT.Term(5, 2);
t.write('main');
t.write('\x1b[?1049h\x1b[Halt');
assert.strictEqual(t.textLines()[0], 'alt');
t.write('\x1b[?1049l');
assert.strictEqual(t.textLines()[0], 'main');

t = new VT.Term(4, 1);
t.write('\x1b(0qqx\x1b(B');
assert.strictEqual(t.textLines()[0], '──│');
t = new VT.Term(8, 1);
t.write('\x1b]0;title\x07ok');
assert.strictEqual(t.textLines()[0], 'ok');
t.write('\x1b[?25l');
assert.strictEqual(t.snapshot().cursor.visible, false);

// ---- Phase 1: headless controller (no DOM) ---------------------------------------------
function fakeClock() {
  let now = 0;
  const q = [];
  return {
    now: () => now,
    requestAnimationFrame: (cb) => { q.push(cb); return q.length; },
    cancelAnimationFrame: () => { q.length = 0; },
    flush: (ms) => {
      now += ms;
      const batch = q.splice(0, q.length);
      for (const cb of batch) cb(now);
    },
  };
}

const castText = '{"version":3,"term":{"cols":8,"rows":2}}\n[0,"o","hi"]\n[1.0,"o","!"]\n[1.0,"m","mid"]\n';
const clock = fakeClock();
const ctrl = C.create({
  data: castText,
  idleTimeLimit: 2,
  speed: 1,
  markers: [[0, 'start']],
  clock: clock,
});

// Initial state
let state = ctrl.getState();
assert.strictEqual(state.status, 'idle');
assert.strictEqual(state.currentTime, 0);
assert.strictEqual(state.duration, 2);
assert.strictEqual(state.speed, 1);
assert.strictEqual(state.canAppend, true);
assert.ok(state.markers.length >= 2); // sidecar + in-band
assert.ok(state.markers.every(m => m.id && typeof m.time === 'number' && m.label != null));
assert.ok(state.terminal.rows.length === 2);
assert.strictEqual(state.dimensions.columns, 8);

// Terminal snapshots are cached until output or resize dirties the terminal.
let snapshotCalls = 0;
const originalSnapshot = ctrl.term.snapshot;
ctrl.term.snapshot = function () { snapshotCalls++; return originalSnapshot.call(this); };
const cachedTerminal = ctrl.getState().terminal;
assert.strictEqual(ctrl.getState().terminal, cachedTerminal);
assert.strictEqual(snapshotCalls, 0, 'initial snapshot remains cached');
ctrl.seek(1.5);
const changedTerminal = ctrl.getState().terminal;
assert.notStrictEqual(changedTerminal, cachedTerminal);
assert.strictEqual(snapshotCalls, 1, 'events invalidate the cached snapshot once');
assert.strictEqual(ctrl.getState().terminal, changedTerminal);
assert.strictEqual(snapshotCalls, 1, 'unchanged state does not copy the terminal again');
ctrl.seek(0);

// subscribe delivers immediately and is removable
const seen = [];
const unsub = ctrl.subscribe((s, meta) => { seen.push(meta.type); });
assert.ok(seen.includes('ready'));
unsub();
unsub(); // idempotent
const n = seen.length;
ctrl.setSpeed(2);
// no more callbacks after unsubscribe
assert.strictEqual(seen.length, n);

// setSpeed does not rebuild terminal / changes state
ctrl.setSpeed(1.5);
assert.strictEqual(ctrl.getState().speed, 1.5);

// play / pause / seek / getCurrentTime
// First animation frame establishes lastTick (dt=0); the second advances the clock.
ctrl.play();
assert.strictEqual(ctrl.getState().status, 'playing');
clock.flush(0);
clock.flush(500); // 0.5s wall * 1.5 speed ≈ 0.75s paced
assert.ok(ctrl.getCurrentTime() > 0);
ctrl.pause();
assert.strictEqual(ctrl.getState().status, 'paused');

ctrl.seek(1.5);
assert.ok(Math.abs(ctrl.getCurrentTime() - 1.5) < 1e-9);
ctrl.seek(0);
assert.ok(ctrl.getCurrentTime() < 1e-9);

// toggle
ctrl.toggle();
assert.strictEqual(ctrl.getState().status, 'playing');
ctrl.toggle();
assert.strictEqual(ctrl.getState().status, 'paused');

// replay after end
ctrl.seek(2);
ctrl.play();
// playing from end rewinds
assert.strictEqual(ctrl.getState().status, 'playing');
assert.ok(ctrl.getCurrentTime() < 1e-6);

// append live-follow at edge
ctrl.pause();
ctrl.seek(2);
assert.ok(ctrl.getState().atLiveEdge);
ctrl.append('[0.5,"o","more"]\n');
assert.strictEqual(ctrl.getState().duration, 2.5);

// Declared-live mode: parked mid-recording, setLive pins the playhead to the edge; an
// append keeps it pinned (unconditionally — no positional check); a rewinding seek or
// play() drops live; a seek TO the edge keeps it.
ctrl.seek(1);
ctrl.setLive(true);
let live = ctrl.getState();
assert.strictEqual(live.live, true);
assert.ok(live.atLiveEdge, 'setLive parks at the edge');
assert.ok(Math.abs(live.currentTime - live.duration) < 1e-9);
ctrl.append('[0.5,"o","live"]\n');
live = ctrl.getState();
assert.strictEqual(live.live, true);
assert.ok(Math.abs(live.currentTime - live.duration) < 1e-9, 'append keeps the pin');
ctrl.seek(live.duration); // to the edge: still live
assert.strictEqual(ctrl.getState().live, true);
const liveEvents = [];
const unsubLive = ctrl.subscribe((st, meta) => { liveEvents.push(meta.type); });
ctrl.seek(0.5); // a rewind: live drops, with a livechange event
assert.strictEqual(ctrl.getState().live, false);
assert.ok(liveEvents.includes('livechange'));
unsubLive();
ctrl.setLive(true);
ctrl.play(); // play from the parked edge is a rewind: live drops
assert.strictEqual(ctrl.getState().live, false);
ctrl.pause();

// getState is a snapshot (mutating returned markers does not corrupt internal list)
const s1 = ctrl.getState();
s1.markers.push({ id: 'x', time: 99, type: 'chapter', label: 'x' });
assert.ok(ctrl.getState().markers.every(m => m.id !== 'x'));

// dispose makes commands no-ops
ctrl.dispose();
ctrl.play();
ctrl.seek(1);
ctrl.append('[1,"o","z"]\n');
assert.strictEqual(ctrl.getState().status, 'idle');

// Marker object form + tuple compatibility
const c2 = C.create({
  data: '{"version":3,"term":{"cols":4,"rows":1}}\n[0,"o","a"]\n',
  markers: [{ time: 0, label: 'A', type: 'chapter' }, [1, 'B']],
  clock: fakeClock(),
});
const marks = c2.getState().markers;
assert.strictEqual(marks[0].label, 'A');
assert.strictEqual(marks[1].label, 'B');
c2.dispose();

// Source type text
const c3 = C.create({
  source: { type: 'text', data: '{"version":3,"term":{"cols":4,"rows":1}}\n[0,"o","z"]\n' },
  clock: fakeClock(),
});
assert.strictEqual(c3.getState().duration, 0);
c3.dispose();

// Discrete events survive playback: seek/speedchange/durationchange/markerchange emitted
// WHILE PLAYING must each reach subscribers with their own meta type — timeupdate
// coalescing must never swallow them.
const clock4 = fakeClock();
const c4 = C.create({ data: castText, idleTimeLimit: 2, clock: clock4 });
const types = [];
c4.subscribe(function (s, meta) { types.push(meta.type); });
c4.play();
clock4.flush(0);
clock4.flush(200);
assert.ok(types.includes('timeupdate'), 'timeupdate flows while playing');
assert.strictEqual(c4.getState().status, 'playing');
c4.seek(1.5, { origin: 'api' });
assert.ok(types.includes('seek'), 'seek delivered while playing, saw: ' + types.join(','));
c4.append('[5,"o","tail"]\n');
assert.ok(types.includes('durationchange'), 'durationchange delivered while playing');
c4.setSpeed(2);
assert.ok(types.includes('speedchange'), 'speedchange delivered while playing');
c4.setMarkers([[0.5, 'half']]);
assert.ok(types.includes('markerchange'), 'markerchange delivered');
assert.strictEqual(c4.getState().markers.filter(m => m.source === 'integration').length, 1);
c4.dispose();

// ---- marker jumps seek without autoplay ----------------------------------------------
const clockM = fakeClock();
const cM = C.create({
  data: '{"version":3,"term":{"cols":4,"rows":1}}\n[0,"o","a"]\n[1.0,"o","b"]\n[2.0,"o","c"]\n',
  markers: [[0, 'start'], [1, 'mid'], [2, 'end']],
  clock: clockM,
});
cM.play();
clockM.flush(0);
cM.pause();
assert.strictEqual(cM.getState().status, 'paused');
cM.seek(0);
cM.jumpMarker(1, 'keyboard');
assert.ok(Math.abs(cM.getCurrentTime() - 1) < 1e-9, ' ] seeks to next marker');
assert.strictEqual(cM.getState().status, 'paused', '[ ] must not autoplay');
cM.play();
assert.strictEqual(cM.getState().status, 'playing');
cM.jumpMarker(1, 'keyboard');
assert.ok(Math.abs(cM.getCurrentTime() - 2) < 1e-9);
assert.strictEqual(cM.getState().status, 'playing', 'jump while playing stays playing');
cM.pause();
cM.jumpMarker(-1, 'keyboard');
assert.ok(Math.abs(cM.getCurrentTime() - 1) < 1e-9);
assert.strictEqual(cM.getState().status, 'paused', '[ while paused stays paused');
cM.dispose();

// ---- fit:'both' scale rule: content-sized mounts must not ratchet -----------------
// Mirrors layout()'s definite-height gate (vertical fit only when the mount's height
// does not track the screen box). The old `availH - 4` trigger shrank forever on
// content-sized embeds.
function fitScale(naturalH, availH, definite) {
  let scale = 1;
  if (definite && availH > 40 && naturalH * scale > availH) {
    scale = Math.min(scale, availH / naturalH);
  }
  return scale;
}
let contentH = 400;
const natural = 400, bar = 32;
for (let i = 0; i < 30; i++) {
  // Content-sized: mount height tracks the box — vertical fit must stay off.
  const s = fitScale(natural, contentH - bar, false);
  assert.strictEqual(s, 1, 'content-sized mount must not vertically scale');
  contentH = natural * s + bar;
}
const short = fitScale(400, 200 - 32, true);
assert.ok(short < 1 && short > 0, 'definite short mount scales down once');
assert.strictEqual(fitScale(400, 200 - 32, true), short, 'definite scale is stable');
// The banned ratchet: subtracting 4 from availH forces another shrink every pass.
let ratchetH = 400;
for (let i = 0; i < 5; i++) {
  const avail = ratchetH - bar;
  if (natural > avail - 4) ratchetH = natural * ((avail - 4) / natural) + bar;
}
assert.ok(ratchetH < 390, 'sanity: the old availH-4 rule really does ratchet');

console.log('vt selftest OK');
"#;
    let spawned = std::process::Command::new("node")
      .arg("-")
      .arg(&bundle)
      .stdin(std::process::Stdio::piped())
      .stdout(std::process::Stdio::piped())
      .stderr(std::process::Stdio::piped())
      .spawn();
    let Ok(mut child) = spawned else {
      let _ = std::fs::remove_dir_all(&dir);
      return; // no node on this machine — the structural tests above still ran
    };
    use std::io::Write;
    child.stdin.take().unwrap().write_all(script.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
      out.status.success() && String::from_utf8_lossy(&out.stdout).contains("vt selftest OK"),
      "vt selftest failed:\nstdout: {}\nstderr: {}",
      String::from_utf8_lossy(&out.stdout),
      String::from_utf8_lossy(&out.stderr)
    );
  }
}
