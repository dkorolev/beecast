//! `beecast-page` — the BeeCast page pipeline as a library: turn asciinema `.cast` text plus
//! plain-strings metadata into one fully self-contained `.html` player page, with the vendored
//! player, styles, recording, and metadata all inlined so a saved copy works fully offline.
//!
//! Deliberately **zero-dependency**: consumers with tiny dependency trees (`scsh` hand-rolls its
//! JSON on purpose) embed the pipeline without pulling serde or anyhow. The few JSON needs are met
//! by the std-only [`json`] module, whose behavior — and output bytes — match the serde-backed
//! renderer this crate was extracted from; the `beecast` CLI test suite pins that equivalence.

mod cast;
mod json;

pub use cast::{inspect, CastError, CastInfo};
