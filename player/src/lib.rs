//! The first-party BeeCast player as a crate: clean-room, dependency-free
//! asciicast (v1/v2/v3) player and VT100/xterm-subset terminal emulator. Consumers inline
//! the two constants whole — a `<script>` element and a `<style>` element — and get the
//! full player: parsing, emulation, playback with idle compression, chapter markers,
//! keyboard control, and live-follow `append` for recordings that are still growing.
//!
//! The JS globals are `BeeCastVT` (the DOM-free core) and `BeeCastPlayer` (the public
//! API); this crate is the component's canonical home.

/// The player bundle: `vt.js` (asciicast parsing + the VT emulator + the pacing map, all
/// DOM-free) then `player.js` (the renderer, playback clock, and controls). Inline it in
/// one `<script>` element; it performs no network requests and loads no workers.
pub const PLAYER_JS: &str = concat!(include_str!("vt.js"), "\n", include_str!("player.js"));

/// The player chrome and terminal palette. All colors are CSS variables, so the embedding
/// page can theme them; nothing is fetched (no fonts, no images).
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
    for banned in ["asciinema-player", "AsciinemaPlayer", "@license", "Apache"] {
      assert!(!PLAYER_JS.contains(banned) && !PLAYER_CSS.contains(banned), "third-party marker '{banned}'");
    }
  }

  /// Behavior tests for the DOM-free core (`BeeCastVT`), run under Node: parsing, the
  /// emulator subset, live-follow appends, and the pacing map. Skips silently when `node`
  /// is not installed — the structural assertions above still gate the bundle.
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

// v3: intervals sum; term size from the header; resize + marker events survive; # comments skip.
let c = VT.parseCast('{"version":3,"term":{"cols":10,"rows":3}}\n# note\n[0.5,"o","hi"]\n[0.5,"m","chapter"]\n[1.0,"r","20x5"]\n');
assert.strictEqual(c.cols, 10); assert.strictEqual(c.rows, 3);
assert.strictEqual(c.events.length, 3);
assert.strictEqual(c.duration, 2);

// v2: absolute times.
c = VT.parseCast('{"version":2,"width":80,"height":24}\n[0.5,"o","a"]\n[2.0,"o","b"]\n');
assert.strictEqual(c.duration, 2); assert.strictEqual(c.events[1].t, 2);

// v1: one JSON doc, stdout deltas.
c = VT.parseCast('{"version":1,"width":5,"height":2,"stdout":[[0.1,"x"],[0.2,"y"]]}');
assert.strictEqual(c.cols, 5); assert.strictEqual(c.events.length, 2);

// Live-follow: appendCast grows a v3 cast across arbitrary chunk boundaries.
c = VT.parseCast('{"version":3,"term":{"cols":10,"rows":3}}\n[1.0,"o","a"]\n');
assert.strictEqual(VT.appendCast(c, '[0.5,"o",'), 0);            // partial line buffers
assert.strictEqual(c.duration, 1);
assert.strictEqual(VT.appendCast(c, '"b"]\n[0.5,"m","x"]\n'), 2); // completed + one more
assert.strictEqual(c.duration, 2);
assert.strictEqual(c.events.length, 3);
assert.strictEqual(c.events[1].t, 1.5);

// v2 appends carry absolute times; stray comment and header lines are skipped.
c = VT.parseCast('{"version":2,"width":80,"height":24}\n[1.0,"o","a"]\n');
VT.appendCast(c, '# noise\n{"version":2}\n[3.0,"o","b"]\n');
assert.strictEqual(c.duration, 3); assert.strictEqual(c.events[1].t, 3);

// A load-time partial trailing line is held back and completed by the first append.
c = VT.parseCast('{"version":3,"term":{"cols":4,"rows":1}}\n[1.0,"o","hi"]\n[2.0,"o');
assert.strictEqual(c.events.length, 1);
VT.appendCast(c, '","yo"]\n');
assert.strictEqual(c.events.length, 2); assert.strictEqual(c.duration, 3);

// v1 casts never grow.
c = VT.parseCast('{"version":1,"width":5,"height":2,"stdout":[[0.1,"x"]]}');
assert.strictEqual(VT.appendCast(c, '[1.0,"o","y"]\n'), 0);

// The pacing map extends in place and keeps both directions consistent.
const ev = [{t:1},{t:2}];
const pacing = VT.buildPacing(ev, 2, null);
ev.push({t:10});
VT.extendPacing(pacing, ev, 2, 10);
assert.strictEqual(pacing.pacedDuration, 10);
assert.strictEqual(VT.mapTime(pacing.rec, pacing.paced, 10), 10);
const limited = VT.buildPacing([{t:1}], 1, 2);
VT.extendPacing(limited, [{t:1},{t:9}], 1, 9);                    // an 8s silence plays as 2s
assert.strictEqual(limited.pacedDuration, 3);
assert.strictEqual(VT.mapTime(limited.paced, limited.rec, 3), 9);

// Plain text, CR/LF, cursor addressing, erase.
let t = new VT.Term(10, 3);
t.write('hello\r\nworld');
assert.deepStrictEqual(t.textLines(), ['hello', 'world', '']);
t.write('\x1b[1;3Hga');
assert.strictEqual(t.textLines()[0], 'hegao');
t.write('\x1b[2J');
assert.deepStrictEqual(t.textLines(), ['', '', '']);

// SGR runs merge; 16/256/true color.
t = new VT.Term(10, 1);
t.write('\x1b[31mred\x1b[0m ok');
const runs = t.snapshot().rows[0];
assert.strictEqual(runs[0].text, 'red'); assert.strictEqual(runs[0].fg, 1);
t = new VT.Term(4, 1);
t.write('\x1b[38;5;196mX\x1b[38;2;1;2;3mY');
assert.strictEqual(t.snapshot().rows[0][0].fg, 196);
assert.strictEqual(t.snapshot().rows[0][1].fg, '#010203');

// Deferred wrap.
t = new VT.Term(3, 2);
t.write('abc');
assert.strictEqual(t.snapshot().cursor.y, 0);
t.write('d');
assert.deepStrictEqual(t.textLines(), ['abc', 'd']);

// Scroll region.
t = new VT.Term(5, 4);
t.write('aa\r\nbb\r\ncc\r\ndd');
t.write('\x1b[2;3r\x1b[3;1H\n');
assert.strictEqual(t.textLines()[0], 'aa');
assert.strictEqual(t.textLines()[1], 'cc');
assert.strictEqual(t.textLines()[3], 'dd');

// Alternate screen restores the primary.
t = new VT.Term(5, 2);
t.write('main');
t.write('\x1b[?1049h\x1b[Halt');
assert.strictEqual(t.textLines()[0], 'alt');
t.write('\x1b[?1049l');
assert.strictEqual(t.textLines()[0], 'main');

// DEC special graphics (tmux borders); OSC consumed; cursor visibility.
t = new VT.Term(4, 1);
t.write('\x1b(0qqx\x1b(B');
assert.strictEqual(t.textLines()[0], '──│');
t = new VT.Term(8, 1);
t.write('\x1b]0;title\x07ok');
assert.strictEqual(t.textLines()[0], 'ok');
t.write('\x1b[?25l');
assert.strictEqual(t.snapshot().cursor.visible, false);

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
