//! Renders the one deliverable: a single `.html` page with the player, its styles, the
//! recording, and the metadata all inlined. The page performs **zero** network requests —
//! no CDN, no sidecar fetches, not even a favicon hit (it is a `data:` URI) — so a saved
//! copy behaves identically to the original, fully offline (the whole point of BeeCast).
//!
//! The page's structure and behavior live in `page.html` (per ENG-PRINCIPLES §4 the glue
//! is deliberately trivial vanilla JS); this module fills its `@@BEECAST_*@@` tokens.

use crate::json;

const TEMPLATE: &str = include_str!("page.html");
// The first-party scsh-cast-player (see `player/README.md` — clean-room, MIT like the
// rest of BeeCast): the DOM-free VT core and the DOM half, concatenated into one bundle.
const PLAYER_JS: &str = concat!(include_str!("player/vt.js"), "\n", include_str!("player/player.js"));
const PLAYER_CSS: &str = include_str!("player/player.css");

/// The metadata one page renders: plain strings and floats, never serde types, so a caller does
/// not inherit any dependencies. Chapters are `(seconds, title)` pairs. Validating the metadata
/// (ascending timekeys, the first chapter at 0) stays the caller's job — the `beecast` CLI runs
/// `beecast_dto::CastMeta::validate` and then hands over borrowed strings at this boundary.
#[derive(Debug, Clone, Default)]
pub struct PageMeta<'a> {
  /// Short human title for the recording; `None` falls back to the recording's filename.
  pub title: Option<&'a str>,
  /// One- or two-sentence description, rendered under the title; `None` stays hidden.
  pub summary: Option<&'a str>,
  /// Chapter markers as `(seconds, title)`, strictly ascending, the first at 0.
  pub chapters: &'a [(f64, &'a str)],
}

/// Build the self-contained page. `fallback_title` (the recording's filename) is used
/// when the metadata has no title.
pub fn build_page(cast_ndjson: &str, meta: &PageMeta, fallback_title: &str) -> String {
  let title = meta.title.unwrap_or(fallback_title);
  let summary = meta.summary.unwrap_or("");
  render(
    TEMPLATE,
    &[
      ("@@BEECAST_TITLE@@", &esc_html(title)),
      ("@@BEECAST_SUMMARY_HIDDEN@@", if summary.is_empty() { " hidden" } else { "" }),
      ("@@BEECAST_SUMMARY@@", &esc_html(summary)),
      ("@@BEECAST_PLAYER_CSS@@", PLAYER_CSS),
      ("@@BEECAST_PLAYER_JS@@", PLAYER_JS),
      ("@@BEECAST_CAST_JSON@@", &js_string_literal(cast_ndjson)),
      ("@@BEECAST_META_JSON@@", &script_safe(&meta_json(meta))),
      // The version is inherited from the one workspace-wide `[workspace.package]`, so this is
      // also the `beecast` CLI's version, and the footer names the tool, not this library crate.
      ("@@BEECAST_FOOTER@@", &format!("beecast v{}", env!("CARGO_PKG_VERSION"))),
    ],
  )
}

/// Serialize the metadata exactly as serde_json serialized the CLI's `CastMeta` before the page
/// pipeline became this crate: fields in declaration order, absent or empty ones omitted (`{}`
/// when nothing is set), strings and floats in serde's own renderings (see `json`).
fn meta_json(meta: &PageMeta) -> String {
  let mut fields: Vec<String> = Vec::new();
  if let Some(title) = meta.title {
    fields.push(format!("\"title\":{}", json::string_literal(title)));
  }
  if let Some(summary) = meta.summary {
    fields.push(format!("\"summary\":{}", json::string_literal(summary)));
  }
  if !meta.chapters.is_empty() {
    let chapters: Vec<String> = meta
      .chapters
      .iter()
      .map(|(t, title)| format!("{{\"t\":{},\"title\":{}}}", json::fmt_f64(*t), json::string_literal(title)))
      .collect();
    fields.push(format!("\"chapters\":[{}]", chapters.join(",")));
  }
  format!("{{{}}}", fields.join(","))
}

/// Substitute tokens by scanning the *template only*: substituted values are appended,
/// never re-scanned, so a recording that happens to contain `@@BEECAST_…@@` (or another
/// token inside a value) cannot be double-expanded.
fn render(template: &str, subs: &[(&str, &str)]) -> String {
  let mut out = String::with_capacity(template.len() + subs.iter().map(|(_, v)| v.len()).sum::<usize>());
  let mut rest = template;
  loop {
    let hit = subs.iter().filter_map(|(k, v)| rest.find(k).map(|i| (i, *k, *v))).min_by_key(|(i, _, _)| *i);
    match hit {
      Some((i, k, v)) => {
        out.push_str(&rest[..i]);
        out.push_str(v);
        rest = &rest[i + k.len()..];
      }
      None => {
        out.push_str(rest);
        return out;
      }
    }
  }
}

/// Escape text for an HTML element/attribute context.
fn esc_html(s: &str) -> String {
  s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Encode arbitrary text as a JS string literal that is safe *inside a `<script>` element*:
/// the JSON encoding escapes quotes, backslashes, and control characters; on top of that every
/// `<` becomes the JSON unicode escape for U+003C, so no `</script>` (or `<!--`) sequence
/// from the recording can terminate the script early.
fn js_string_literal(s: &str) -> String {
  json::string_literal(s).replace('<', "\\u003c")
}

/// Make a JSON document safe to embed as a JS object literal inside `<script>`: `<` only
/// ever occurs within string values, where the U+003C unicode escape is equivalent.
fn script_safe(json: &str) -> String {
  json.replace('<', "\\u003c")
}

#[cfg(test)]
mod tests {
  use super::*;

  fn demo_meta() -> PageMeta<'static> {
    PageMeta { title: Some("Demo <run>"), summary: Some("A & B."), chapters: &[(0.0, "Start"), (12.5, "Mid")] }
  }

  #[test]
  fn page_is_self_contained() {
    let page =
      build_page("{\"version\":2,\"width\":80,\"height\":24}\n[1.0,\"o\",\"hi\"]\n", &demo_meta(), "demo.cast");
    // No external resource loads of any kind: every script and stylesheet is inline, and
    // the favicon is a data: URI (an absent favicon line would make browsers request
    // /favicon.ico — a 404 and a console warning when offline).
    assert!(!page.contains("<script src"), "no external scripts");
    assert!(!page.contains("<link rel=\"stylesheet\""), "no external stylesheets");
    assert!(page.contains("<link rel=\"icon\" href=\"data:image/svg+xml,"), "inline favicon");
    // The player bundle and its CSS are embedded whole.
    assert!(page.contains(&PLAYER_JS[..200]) && page.len() > PLAYER_JS.len() + PLAYER_CSS.len());
  }

  #[test]
  fn page_respects_title_summary_and_chapters() {
    let page = build_page("{\"version\":2,\"width\":80,\"height\":24}\n", &demo_meta(), "demo.cast");
    assert!(page.contains("<title>Demo &lt;run&gt;</title>"), "title, escaped");
    assert!(page.contains("<p id=\"summary\">A &amp; B.</p>"), "summary, escaped, not hidden");
    assert!(page.contains("\"chapters\":[{\"t\":0.0,\"title\":\"Start\"},{\"t\":12.5,\"title\":\"Mid\"}]"));
  }

  #[test]
  fn page_without_meta_falls_back_to_the_filename() {
    let page = build_page("{\"version\":2,\"width\":80,\"height\":24}\n", &PageMeta::default(), "rec.cast");
    assert!(page.contains("<title>rec.cast</title>"));
    assert!(page.contains("<p id=\"summary\" hidden></p>"), "empty summary stays hidden");
    assert!(page.contains("const META = {};"), "an empty metadata object, exactly as serde rendered one");
  }

  #[test]
  fn hostile_cast_content_cannot_break_out_of_the_script() {
    let cast = "{\"version\":2,\"width\":80,\"height\":24}\n[1.0,\"o\",\"</script><script>alert(1)\"]\n";
    let page = build_page(cast, &PageMeta::default(), "x.cast");
    let payload_zone = &page[page.find("const CAST_DATA").unwrap()..];
    assert!(!payload_zone.contains("</script><script>alert"), "the recording's text is neutralized");
    assert!(payload_zone.contains("\\u003c/script>"), "escaped as \\u003c");
  }

  #[test]
  fn hostile_meta_titles_are_neutralized_too() {
    let meta = PageMeta { title: Some("<script>x"), summary: None, chapters: &[(0.0, "</script>y")] };
    let page = build_page("{\"version\":2,\"width\":80,\"height\":24}\n", &meta, "x.cast");
    assert!(page.contains("<title>&lt;script&gt;x</title>"));
    assert!(!page.contains("\"title\":\"</script>"), "chapter titles are script-safe in the embedded META");
  }

  #[test]
  fn render_never_rescans_substituted_values() {
    // A value containing another token must land verbatim, not get expanded.
    let out = render("[@@A@@|@@B@@]", &[("@@A@@", "value-with-@@B@@"), ("@@B@@", "bee")]);
    assert_eq!(out, "[value-with-@@B@@|bee]");
  }

  #[test]
  fn no_tokens_survive_in_the_output() {
    let page = build_page("{\"version\":2,\"width\":80,\"height\":24}\n", &demo_meta(), "demo.cast");
    assert!(!page.contains("@@BEECAST_"), "all template tokens substituted");
  }

  /// The self-containment claim also depends on the player bundle itself: it must not
  /// contain a literal `</script`, must not reference workers, and its CSS must not pull
  /// fonts or images. Asserted here so a future player change that breaks any of it fails
  /// loudly. And the bundle must be the first-party clean-room player — no third-party
  /// code, and therefore no third-party license, rides in any generated page.
  #[test]
  fn player_bundle_is_inline_safe_and_first_party() {
    assert!(!PLAYER_JS.contains("</script"), "bundle would terminate the inline <script>");
    assert!(!PLAYER_JS.contains("<!--"), "bundle would enter the script double-escaped state");
    assert!(!PLAYER_JS.to_lowercase().contains("worker"), "bundle must not load a worker sidecar");
    assert!(!PLAYER_CSS.contains("url("), "player CSS must not fetch fonts/images");
    assert!(PLAYER_JS.contains("ScshCastPlayer") && PLAYER_JS.contains("Clean-room implementation"));
    for banned in ["asciinema-player", "AsciinemaPlayer", "@license", "Apache"] {
      assert!(!PLAYER_JS.contains(banned) && !PLAYER_CSS.contains(banned), "third-party marker '{banned}'");
    }
  }

  /// The same guarantee end to end: a BUILT page carries no third-party license marker
  /// either (the bundle check above could miss template regressions).
  #[test]
  fn generated_page_carries_no_third_party_marker() {
    let page =
      build_page("{\"version\":2,\"width\":80,\"height\":24}\n[1.0,\"o\",\"hi\"]\n", &demo_meta(), "demo.cast");
    for banned in ["asciinema-player", "AsciinemaPlayer", "@license", "Apache"] {
      assert!(!page.contains(banned), "third-party marker '{banned}' in the generated page");
    }
  }

  /// Behavior tests for the player's DOM-free VT core, run under Node (the same suite that
  /// gates the canonical copy in scsh). Skips silently when `node` is not installed — the
  /// structural assertions above still gate the bundle itself.
  #[test]
  fn vt_core_node_selftest() {
    let dir = std::env::temp_dir().join(format!("beecast-vt-selftest-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bundle = dir.join("player.js");
    std::fs::write(&bundle, PLAYER_JS).unwrap();
    let script = r#"
const assert = require('assert');
require(process.argv[2]);
const VT = globalThis.ScshVT;

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
