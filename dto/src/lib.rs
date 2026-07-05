//! `beecast-dto` — the cast-metadata sidecar DTO: `{ title, summary, chapters }`.
//!
//! These Rust types are the source of truth for the sidecar's shape (ENG-PRINCIPLES §1).
//! `schema/beecast-meta.schema.json` is the formal JSON Schema rendering — *generated*
//! from these types by [`generated_schema`] (doc comments become descriptions) and pinned
//! byte-for-byte by a unit test below; regenerate with
//! `cargo run -p beecast -q -- schema > dto/schema/beecast-meta.schema.json`. `SCHEMA.md`
//! is the human-readable rendering. Parsing is strict — an unknown key is a hard, loud error.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The shipped JSON Schema file, embedded only to pin it byte-for-byte to
/// [`generated_schema`] (the `shipped_schema_is_the_generated_one` test). The CLI itself
/// prints the generated document, so the binary never depends on this file at runtime.
#[cfg(test)]
pub const JSON_SCHEMA: &str = include_str!("../schema/beecast-meta.schema.json");

/// Sidecar metadata for one `.cast` recording, conventionally stored next to it as
/// `<name>.meta.json`. Every field is optional: a bare recording plays fine without any.
/// Two invariants live beyond JSON Schema, in the beecast/seecast validators: chapters
/// are STRICTLY ascending by `t`, and a non-empty chapter list MUST start at `t` = 0.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Cast metadata sidecar")]
pub struct CastMeta {
  /// Short human title for the recording; becomes the page's `<title>` and header.
  /// Absent → the page falls back to the recording's filename.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  #[schemars(length(min = 1))]
  pub title: Option<String>,
  /// One- or two-sentence description of what the recording shows, rendered under the title.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  #[schemars(length(min = 1))]
  pub summary: Option<String>,
  /// Chapter markers, strictly ascending by `t`. When non-empty, the first chapter MUST
  /// start at `t: 0` (YouTube-style: the opening segment always has a marker).
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub chapters: Vec<Chapter>,
}

/// One chapter marker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Chapter {
  /// The timekey: seconds into the recording. Fractional values are allowed (`12.5`);
  /// the first chapter's `t` must be exactly `0`.
  #[schemars(range(min = 0.0))]
  pub t: f64,
  /// Short chapter title; 3–6 words works best.
  #[schemars(length(min = 1))]
  pub title: String,
}

/// Render the JSON Schema from the Rust types themselves — the §1 codegen path. Chapter
/// is inlined (no `$defs`) so the document reads top-down, like the hand of a human.
pub fn generated_schema() -> String {
  let generator = schemars::generate::SchemaSettings::default().with(|s| s.inline_subschemas = true).into_generator();
  let schema = generator.into_root_schema_for::<CastMeta>();
  let mut out = serde_json::to_string_pretty(&schema).expect("the schema serializes: it is a plain JSON value");
  out.push('\n');
  out
}

/// Everything that can be wrong with a structurally-valid sidecar. JSON syntax and
/// unknown-field errors surface earlier, from serde itself (see [`parse`]).
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum MetaError {
  #[error("`title` must not be empty (omit the field instead)")]
  EmptyTitle,
  #[error("`summary` must not be empty (omit the field instead)")]
  EmptySummary,
  #[error("chapter {index}: `title` must not be empty")]
  EmptyChapterTitle { index: usize },
  #[error("chapter {index}: `t` = {t} is not a finite number of seconds >= 0")]
  BadTimekey { index: usize, t: f64 },
  #[error("the first chapter must start at t = 0, got t = {t}")]
  FirstChapterNotZero { t: f64 },
  #[error("chapters must be strictly ascending by `t`: chapter {index} has t = {t} after t = {prev}")]
  NotAscending { index: usize, t: f64, prev: f64 },
}

impl CastMeta {
  /// Enforce the invariants JSON Schema cannot express: non-empty strings, finite
  /// timekeys, strict ascending order, and the first chapter pinned at `t = 0`.
  pub fn validate(&self) -> Result<(), MetaError> {
    if self.title.as_deref().is_some_and(|s| s.trim().is_empty()) {
      return Err(MetaError::EmptyTitle);
    }
    if self.summary.as_deref().is_some_and(|s| s.trim().is_empty()) {
      return Err(MetaError::EmptySummary);
    }
    for (index, c) in self.chapters.iter().enumerate() {
      if c.title.trim().is_empty() {
        return Err(MetaError::EmptyChapterTitle { index });
      }
      if !c.t.is_finite() || c.t < 0.0 {
        return Err(MetaError::BadTimekey { index, t: c.t });
      }
      if index == 0 && c.t != 0.0 {
        return Err(MetaError::FirstChapterNotZero { t: c.t });
      }
      if index > 0 {
        let prev = self.chapters[index - 1].t;
        if c.t <= prev {
          return Err(MetaError::NotAscending { index, t: c.t, prev });
        }
      }
    }
    Ok(())
  }
}

/// Everything that can go wrong parsing a sidecar: a JSON syntax or unknown-field error
/// from serde, or a semantic invariant from [`CastMeta::validate`]. Typed so a library
/// caller can branch on it (§5); the CLI adds `anyhow` context on top.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
  #[error("invalid JSON: {0}")]
  Json(#[from] serde_json::Error),
  #[error(transparent)]
  Invalid(#[from] MetaError),
}

/// Parse and validate a sidecar: serde reports syntax and unknown-field errors,
/// [`CastMeta::validate`] the semantic ones, both funneled into one typed [`ParseError`].
pub fn parse(json: &str) -> Result<CastMeta, ParseError> {
  let meta: CastMeta = serde_json::from_str(json)?;
  meta.validate()?;
  Ok(meta)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_full_and_empty_sidecars() {
    let m = parse(
      r#"{ "title": "T", "summary": "S.", "chapters": [{ "t": 0, "title": "A" }, { "t": 12.5, "title": "B" }] }"#,
    )
    .unwrap();
    assert_eq!(m.title.as_deref(), Some("T"));
    assert_eq!(m.chapters[1].t, 12.5); // fractional timekey preserved
    assert_eq!(parse("{}").unwrap(), CastMeta::default()); // every field optional
    assert_eq!(parse(r#"{ "chapters": [] }"#).unwrap().chapters.len(), 0); // empty == absent
  }

  #[test]
  fn rejects_unknown_fields_loudly() {
    assert!(parse(r#"{ "titel": "typo" }"#).is_err());
    assert!(parse(r#"{ "chapters": [{ "t": 0, "title": "A", "note": "nope" }] }"#).is_err());
  }

  #[test]
  fn rejects_semantic_violations() {
    let first_not_zero = CastMeta { chapters: vec![Chapter { t: 1.0, title: "A".into() }], ..Default::default() };
    assert_eq!(first_not_zero.validate(), Err(MetaError::FirstChapterNotZero { t: 1.0 }));

    let unsorted = CastMeta {
      chapters: vec![
        Chapter { t: 0.0, title: "A".into() },
        Chapter { t: 9.0, title: "B".into() },
        Chapter { t: 9.0, title: "C".into() }, // ties are not ascending either
      ],
      ..Default::default()
    };
    assert_eq!(unsorted.validate(), Err(MetaError::NotAscending { index: 2, t: 9.0, prev: 9.0 }));

    let negative = CastMeta { chapters: vec![Chapter { t: -0.5, title: "A".into() }], ..Default::default() };
    assert_eq!(negative.validate(), Err(MetaError::BadTimekey { index: 0, t: -0.5 }));

    assert_eq!(CastMeta { title: Some("  ".into()), ..Default::default() }.validate(), Err(MetaError::EmptyTitle));
    let blank_chapter = CastMeta { chapters: vec![Chapter { t: 0.0, title: " ".into() }], ..Default::default() };
    assert_eq!(blank_chapter.validate(), Err(MetaError::EmptyChapterTitle { index: 0 }));
  }

  /// The shipped schema file IS the codegen output (§1): byte-for-byte. When this fails, the
  /// types changed — regenerate with `cargo run -p beecast -q -- schema > dto/schema/beecast-meta.schema.json`.
  #[test]
  fn shipped_schema_is_the_generated_one() {
    assert_eq!(
      JSON_SCHEMA,
      generated_schema(),
      "dto/schema/beecast-meta.schema.json is stale; regenerate it from the Rust types"
    );
  }

  /// Sanity on the generated document itself: strict, complete, and self-describing.
  #[test]
  fn generated_schema_is_strict_and_documented() {
    let s: serde_json::Value = serde_json::from_str(&generated_schema()).unwrap();
    assert_eq!(s["additionalProperties"], serde_json::Value::Bool(false));
    let props = s["properties"].as_object().unwrap();
    let mut keys: Vec<_> = props.keys().collect();
    keys.sort();
    assert_eq!(keys, vec!["chapters", "summary", "title"]);
    let chapter = &s["properties"]["chapters"]["items"];
    assert_eq!(chapter["additionalProperties"], serde_json::Value::Bool(false));
    assert_eq!(chapter["required"], serde_json::json!(["t", "title"]));
    let t_desc = chapter["properties"]["t"]["description"].as_str().unwrap();
    assert!(t_desc.contains("seconds") && t_desc.contains("Fractional"), "got: {t_desc}");
    for (name, prop) in props {
      assert!(prop["description"].is_string(), "field `{name}` lost its doc comment");
    }
  }
}
