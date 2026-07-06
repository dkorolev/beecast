//! Light validation of the input recording: enough to reject a non-asciicast early with
//! a clear error, tell v1/v2/v3 apart, and know the total duration (used to warn about
//! chapters that point past the end). Full playback parsing is the embedded player's job.

use crate::json::{self, Value};

/// What `beecast` needs to know about a recording before embedding it.
#[derive(Debug, Clone, PartialEq)]
pub struct CastInfo {
  /// The asciicast format version: 1 (single JSON document), 2, or 3 (NDJSON; v3 events
  /// carry *relative* intervals where v2 carries absolute times).
  pub version: u8,
  /// Total length in seconds, when it can be computed cheaply. `None` for a v1 recording
  /// without a `duration` header field.
  pub duration: Option<f64>,
}

/// Everything that disqualifies an input file. The `Display` impl is written by hand rather than
/// derived by `thiserror` — this crate is zero-dependency on purpose.
#[derive(Debug, PartialEq)]
pub enum CastError {
  /// The file starts with neither an asciicast header nor a v1 JSON document.
  NotAsciicast,
  /// The header names an asciicast version this crate does not know how to embed.
  UnsupportedVersion(u64),
}

impl std::fmt::Display for CastError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      CastError::NotAsciicast => {
        write!(f, "not an asciicast: the file starts with neither an asciicast header nor a v1 JSON document")
      }
      CastError::UnsupportedVersion(v) => write!(f, "asciicast v{v} is not supported (v1, v2, and v3 are)"),
    }
  }
}

impl std::error::Error for CastError {}

/// Sniff the recording. v2/v3 are NDJSON with a one-line header; v1 is one (possibly
/// pretty-printed) JSON document, so when the first line does not parse on its own the
/// whole file gets one more chance.
pub fn inspect(content: &str) -> Result<CastInfo, CastError> {
  let first_line = content.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
  let header = json::parse(first_line).or_else(|| json::parse(content)).ok_or(CastError::NotAsciicast)?;
  let version = header.get("version").and_then(Value::as_u64).ok_or(CastError::NotAsciicast)?;
  match version {
    1 => Ok(CastInfo { version: 1, duration: header.get("duration").and_then(Value::as_f64) }),
    2 | 3 => Ok(CastInfo { version: version as u8, duration: ndjson_duration(content, version == 3) }),
    other => Err(CastError::UnsupportedVersion(other)),
  }
}

/// Duration of an NDJSON recording: the last event time for v2 (absolute stamps), the sum
/// of intervals for v3 (relative stamps). Unparseable event lines are skipped — a live,
/// still-growing recording may end mid-line and must still embed fine.
fn ndjson_duration(content: &str, relative: bool) -> Option<f64> {
  let mut last = 0.0f64;
  let mut sum = 0.0f64;
  for line in content.lines().skip(1) {
    let Some(Value::Arr(items)) = json::parse(line.trim()) else { continue };
    let Some(t) = items.first().and_then(Value::as_f64) else { continue };
    last = t;
    sum += t;
  }
  Some(if relative { sum } else { last })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn inspects_v2_with_absolute_times() {
    let cast = "{\"version\":2,\"width\":80,\"height\":24}\n[0.5,\"o\",\"a\"]\n[2.0,\"o\",\"b\"]\n";
    assert_eq!(inspect(cast).unwrap(), CastInfo { version: 2, duration: Some(2.0) });
  }

  #[test]
  fn inspects_v3_with_relative_intervals() {
    let cast =
      "{\"version\":3,\"term\":{\"cols\":80,\"rows\":24}}\n[0.5,\"o\",\"a\"]\n[1.0,\"o\",\"b\"]\n[1.5,\"o\",\"c\"]\n";
    assert_eq!(inspect(cast).unwrap(), CastInfo { version: 3, duration: Some(3.0) });
  }

  #[test]
  fn inspects_v1_whole_document() {
    let cast = "{\n  \"version\": 1,\n  \"duration\": 7.5,\n  \"width\": 80,\n  \"height\": 24,\n  \"stdout\": []\n}";
    assert_eq!(inspect(cast).unwrap(), CastInfo { version: 1, duration: Some(7.5) });
  }

  #[test]
  fn tolerates_a_truncated_trailing_line() {
    let cast = "{\"version\":3,\"term\":{\"cols\":80,\"rows\":24}}\n[0.5,\"o\",\"a\"]\n[1.0,\"o\",\"tru";
    assert_eq!(inspect(cast).unwrap(), CastInfo { version: 3, duration: Some(0.5) });
  }

  #[test]
  fn rejects_junk_and_unknown_versions() {
    assert_eq!(inspect("hello world"), Err(CastError::NotAsciicast));
    assert_eq!(inspect("{\"no_version\":true}"), Err(CastError::NotAsciicast));
    assert_eq!(inspect("{\"version\":9}"), Err(CastError::UnsupportedVersion(9)));
    // The serde-era reading is preserved: a fractional literal is not an integer version.
    assert_eq!(inspect("{\"version\":2.0}"), Err(CastError::NotAsciicast));
  }
}
