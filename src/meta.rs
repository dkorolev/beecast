//! The cast metadata sidecar: `{ title, summary, chapters }`.
//!
//! These Rust types are the source of truth for the sidecar's shape (ENG-PRINCIPLES §1).
//! `schema/beecast-meta.schema.json` is the formal JSON Schema rendering and `SCHEMA.md`
//! the human-readable one; a unit test below keeps the schema file in sync with what this
//! module actually accepts. Parsing is strict — an unknown key is a hard, loud error.

use serde::{Deserialize, Serialize};

/// The formal JSON Schema for [`CastMeta`], embedded so `beecast schema` works offline
/// and so tests can assert the shipped schema matches the Rust validator.
pub const JSON_SCHEMA: &str = include_str!("../schema/beecast-meta.schema.json");

/// Sidecar metadata for one `.cast` recording, conventionally stored next to it as
/// `<name>.meta.json`. Every field is optional: a bare recording plays fine without any.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CastMeta {
  /// Short human title for the recording; becomes the page's `<title>` and header.
  /// Absent → the page falls back to the recording's filename.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub title: Option<String>,
  /// One- or two-sentence description of what the recording shows, rendered under the title.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub summary: Option<String>,
  /// Chapter markers, strictly ascending by `t`. When non-empty, the first chapter MUST
  /// start at `t: 0` (YouTube-style: the opening segment always has a marker).
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub chapters: Vec<Chapter>,
}

/// One chapter marker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chapter {
  /// The timekey: seconds into the recording. Fractional values are allowed (`12.5`);
  /// the first chapter's `t` must be exactly `0`.
  pub t: f64,
  /// Short chapter title; 3–6 words works best.
  pub title: String,
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

/// Parse and validate a sidecar. serde reports syntax and unknown-field errors;
/// [`CastMeta::validate`] reports the semantic ones. Both come back as one `anyhow`
/// error with the file's story attached by the caller.
pub fn parse(json: &str) -> anyhow::Result<CastMeta> {
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

  /// The shipped JSON Schema is the *rendering* of these Rust types (§1) — keep the two
  /// from drifting: it must parse, describe the same three properties, stay strict, and
  /// state the fractional-seconds / starts-at-zero timekey contract.
  #[test]
  fn shipped_schema_matches_the_types() {
    let s: serde_json::Value = serde_json::from_str(JSON_SCHEMA).unwrap();
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
    assert!(t_desc.contains("MUST be exactly 0"), "got: {t_desc}");
  }
}
