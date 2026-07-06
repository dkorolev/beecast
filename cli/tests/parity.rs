//! Extraction parity: the zero-dependency `beecast-page` crate reimplements the two serializers
//! the page pipeline used to get from serde_json — string escaping and float rendering — and its
//! output must match serde byte-for-byte. The fixture-level fingerprint in `cli.rs` pins one
//! recording end to end; this suite hammers the reimplementations differentially, comparing the
//! metadata document embedded by `beecast_page::build_page` against `serde_json::to_string` of the
//! very same `CastMeta`, over hostile strings and thousands of pseudo-random `f64` bit patterns.
//! It lives in the CLI's suite (not in `beecast-page`) so the library keeps zero dependencies,
//! dev-dependencies included.

use beecast_dto::{CastMeta, Chapter};
use beecast_page::{build_page, PageMeta};

const CAST: &str = "{\"version\":2,\"width\":80,\"height\":24}\n[1.0,\"o\",\"hi\"]\n";

/// The metadata document `build_page` embedded, extracted back out of the rendered page.
fn embedded_meta(page: &str) -> &str {
  let start = page.find("const META = ").expect("the page embeds META") + "const META = ".len();
  let end = start + page[start..].find(";\n").expect("the document ends its line");
  &page[start..end]
}

/// Render `meta` through the plain-strings `beecast-page` path, exactly as the CLI does.
fn page_rendering(meta: &CastMeta) -> String {
  let chapters: Vec<(f64, &str)> = meta.chapters.iter().map(|c| (c.t, c.title.as_str())).collect();
  let page_meta = PageMeta { title: meta.title.as_deref(), summary: meta.summary.as_deref(), chapters: &chapters };
  embedded_meta(&build_page(CAST, &page_meta, "x.cast")).to_string()
}

/// What the serde-era renderer embedded: `serde_json::to_string` plus the `<` neutralization.
fn serde_rendering(meta: &CastMeta) -> String {
  serde_json::to_string(meta).expect("CastMeta serializes").replace('<', "\\u003c")
}

#[test]
fn hostile_strings_serialize_exactly_like_serde() {
  let strings = [
    "plain",
    "with \"quotes\" and \\back\\slashes\\ and /slashes/",
    "tabs\tnewlines\nreturns\rbackspace\u{8}formfeed\u{c}",
    "every C0: \u{0}\u{1}\u{2}\u{3}\u{b}\u{e}\u{f}\u{10}\u{1f}",
    "</script><script>alert(1)</script><!--",
    "unicode: héllo \u{1f41d} \u{2028}\u{2029} \u{7f} \u{fffd}",
    "",
    " ",
  ];
  for s in strings {
    let meta = CastMeta {
      title: Some(s.into()),
      summary: Some(format!("summary {s}")),
      chapters: vec![Chapter { t: 0.0, title: s.into() }],
    };
    assert_eq!(page_rendering(&meta), serde_rendering(&meta), "for {s:?}");
  }
  // The all-absent shape serializes to the empty object.
  assert_eq!(page_rendering(&CastMeta::default()), "{}");
  assert_eq!(serde_rendering(&CastMeta::default()), "{}");
}

fn xorshift(state: &mut u64) -> u64 {
  *state ^= *state << 13;
  *state ^= *state >> 7;
  *state ^= *state << 17;
  *state
}

/// Float rendering is the riskiest reimplementation (serde goes through ryu; `beecast-page`
/// reassembles ryu's notation from `{:e}`), so it gets the widest net: boundary values around
/// every notation switch, plus thousands of finite `f64`s drawn from random bit patterns.
#[test]
fn floats_serialize_exactly_like_serde() {
  let mut timekeys: Vec<f64> = vec![
    0.0,
    -0.0,
    1.0,
    -1.0,
    0.1,
    12.5,
    1e-4,
    1e-5,
    1e-6,
    1e15,
    9007199254740992.0,
    1e16,
    1.23e17,
    1e21,
    5e-324,
    f64::MIN_POSITIVE,
    f64::MAX,
    -f64::MAX,
  ];
  let mut state = 0x9e37_79b9_7f4a_7c15u64;
  while timekeys.len() < 5000 {
    let value = f64::from_bits(xorshift(&mut state));
    if value.is_finite() {
      timekeys.push(value);
    }
  }
  // `build_page` does not validate (the CLI validates before it renders), so even the negative
  // and denormal timekeys above flow through — exactly what a differential test wants.
  let chapters: Vec<Chapter> = timekeys.iter().map(|t| Chapter { t: *t, title: "x".into() }).collect();
  let meta = CastMeta { title: None, summary: None, chapters };
  assert_eq!(page_rendering(&meta), serde_rendering(&meta));
}
