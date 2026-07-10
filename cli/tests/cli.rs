//! End-to-end CLI tests: run the real binary the way a script would (stdout captured, so
//! `beecast` is in machine mode) and assert the §2 contract — JSON envelopes on stdout,
//! diagnostics on stderr, documented exit codes — plus the shape of the generated page.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn beecast(args: &[&str], cwd: &Path) -> Output {
  Command::new(env!("CARGO_BIN_EXE_beecast")).args(args).current_dir(cwd).output().expect("binary runs")
}

fn fixture(name: &str) -> PathBuf {
  Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

/// A per-test scratch directory that cleans itself up on drop, so a panicking test
/// (a failed assertion) does not leak directories under the system temp dir.
struct TempDir(PathBuf);

impl std::ops::Deref for TempDir {
  type Target = Path;
  fn deref(&self) -> &Path {
    &self.0
  }
}

impl Drop for TempDir {
  fn drop(&mut self) {
    let _ = std::fs::remove_dir_all(&self.0);
  }
}

fn tempdir(tag: &str) -> TempDir {
  let dir = std::env::temp_dir().join(format!("beecast-cli-{tag}-{}", std::process::id()));
  let _ = std::fs::remove_dir_all(&dir); // a stale dir from an older, crashed run
  std::fs::create_dir_all(&dir).unwrap();
  TempDir(dir)
}

#[test]
fn build_discovers_the_sidecar_and_reports_json_ok() {
  let dir = tempdir("ok");
  std::fs::copy(fixture("sample.cast"), dir.join("sample.cast")).unwrap();
  std::fs::copy(fixture("sample.meta.json"), dir.join("sample.meta.json")).unwrap();

  let out = beecast(&["build", "sample.cast"], &dir);
  assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));

  // Machine mode (stdout is a pipe here): a single-key, request-specific `Built` document.
  let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("clean JSON on stdout");
  assert_eq!(v["Built"]["cast_version"], 3);
  assert_eq!(v["Built"]["chapters"], 2);
  // The implicit sidecar discovery rides inside the document; machine-mode stderr is quiet.
  assert!(v["Built"]["meta"].as_str().unwrap().contains("sample.meta.json"));
  assert_eq!(v["Built"]["warnings"], serde_json::json!([]));
  assert!(out.stderr.is_empty(), "machine-mode stderr stays quiet, got: {}", String::from_utf8_lossy(&out.stderr));

  let html = std::fs::read_to_string(dir.join("sample.html")).unwrap();
  assert!(html.contains("<title>Sample session</title>"));
  assert!(html.contains("Echoes a greeting"));
  assert!(html.contains("\"title\":\"The build\""));
}

#[test]
fn build_streams_html_to_stdout_with_dash_output() {
  let dir = tempdir("stdout");
  std::fs::copy(fixture("sample.cast"), dir.join("s.cast")).unwrap();
  let out = beecast(&["build", "s.cast", "-o", "-"], &dir);
  assert!(out.status.success());
  let html = String::from_utf8_lossy(&out.stdout);
  assert!(html.starts_with("<!DOCTYPE html>"), "data (the page itself) goes to stdout");
  assert!(html.contains("<title>s.cast</title>"), "no sidecar: the filename is the title");
}

#[test]
fn invalid_metadata_fails_with_a_json_error_and_exit_1() {
  let dir = tempdir("badmeta");
  std::fs::copy(fixture("sample.cast"), dir.join("s.cast")).unwrap();
  std::fs::write(dir.join("s.meta.json"), r#"{ "chapters": [{ "t": 5, "title": "starts late" }] }"#).unwrap();
  let out = beecast(&["build", "s.cast"], &dir);
  assert_eq!(out.status.code(), Some(1));
  let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("error is clean JSON on stdout");
  assert!(v["Error"]["message"].as_str().unwrap().contains("first chapter must start at t = 0"));
  assert_eq!(v["Error"]["stage"], "request", "run failures are stage=request");
  assert!(!dir.join("s.html").exists(), "no output written on failure");

  // A serde-level failure (duplicate key) must state the problem exactly once — a
  // Display prefix plus an error-chain source would print the serde message twice.
  std::fs::write(dir.join("s.meta.json"), r#"{ "title": "a", "title": "b" }"#).unwrap();
  let out = beecast(&["build", "s.cast"], &dir);
  assert_eq!(out.status.code(), Some(1));
  let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
  let msg = v["Error"]["message"].as_str().unwrap();
  assert_eq!(msg.matches("duplicate field").count(), 1, "serde message printed once, got: {msg}");
}

#[test]
fn chapter_past_the_end_warns_in_both_channels() {
  let dir = tempdir("warn");
  std::fs::copy(fixture("sample.cast"), dir.join("s.cast")).unwrap();
  std::fs::write(
    dir.join("s.meta.json"),
    r#"{ "chapters": [{ "t": 0, "title": "Start" }, { "t": 9999, "title": "Way past" }] }"#,
  )
  .unwrap();
  let out = beecast(&["build", "s.cast"], &dir);
  assert!(out.status.success(), "a stale sidecar warns, it does not fail");
  let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
  let warning = v["Built"]["warnings"][0].as_str().expect("warning folded into the JSON document");
  assert!(warning.contains("past the end"), "got: {warning}");
  // …and on stderr too — a warning only in the JSON is invisible to humans.
  assert!(String::from_utf8_lossy(&out.stderr).contains("past the end"));
}

#[test]
fn junk_input_fails_and_usage_errors_exit_2() {
  let dir = tempdir("usage");
  std::fs::write(dir.join("junk.cast"), "not a cast at all").unwrap();
  assert_eq!(beecast(&["build", "junk.cast"], &dir).status.code(), Some(1));
  assert_eq!(beecast(&["frobnicate"], &dir).status.code(), Some(2));
  assert_eq!(beecast(&["build"], &dir).status.code(), Some(2));
  assert_eq!(beecast(&["build", "a.cast", "--wat"], &dir).status.code(), Some(2));
}

#[test]
fn explicit_meta_flag_is_used_and_must_exist() {
  let dir = tempdir("meta");
  std::fs::copy(fixture("sample.cast"), dir.join("s.cast")).unwrap();
  std::fs::copy(fixture("sample.meta.json"), dir.join("custom.json")).unwrap();
  let out = beecast(&["build", "s.cast", "--meta", "custom.json"], &dir);
  assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
  let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
  assert_eq!(v["Built"]["chapters"], 2);
  assert!(v["Built"]["meta"].as_str().unwrap().contains("custom.json"));
  // Unlike the optional implicit `<recording>.meta.json`, an explicit `--meta` that does
  // not exist is a hard error — the caller named a file and must not silently lose it.
  let missing = beecast(&["build", "s.cast", "--meta", "nope.json"], &dir);
  assert_eq!(missing.status.code(), Some(1));
  let e: serde_json::Value = serde_json::from_slice(&missing.stdout).unwrap();
  assert!(e["Error"]["message"].as_str().unwrap().contains("nope.json"));
}

#[test]
fn machine_invocations_must_use_the_canonical_command() {
  let dir = tempdir("canonical");
  std::fs::copy(fixture("sample.cast"), dir.join("s.cast")).unwrap();
  // `beecast s.cast` is accepted at a TTY; here (a pipe) it must refuse and teach.
  let out = beecast(&["s.cast"], &dir);
  assert_eq!(out.status.code(), Some(2));
  let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
  assert!(v["Error"]["message"].as_str().unwrap().contains("beecast build s.cast"));
  assert_eq!(v["Error"]["stage"], "usage", "wrong invocations are stage=usage");
  assert!(!dir.join("s.html").exists());
}

/// Piping into `head` must end the program quietly (§2): a broken pipe is a clean exit,
/// never a panic. The read end of the child's stdout is closed before it writes, so the
/// 6 MB HTML stream hits `BrokenPipe`; `emit` exits 0 with no traceback (no `libc`, no
/// signal death — pure std). This asserts the clean-exit contract without depending on
/// how the OS would otherwise deliver SIGPIPE.
#[cfg(unix)]
#[test]
fn broken_pipe_dies_quietly_without_a_panic() {
  use std::process::Stdio;
  let dir = tempdir("sigpipe");
  std::fs::copy(fixture("sample.cast"), dir.join("s.cast")).unwrap();
  let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_beecast"))
    .args(["build", "s.cast", "-o", "-"])
    .current_dir(&*dir)
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("binary spawns");
  drop(child.stdout.take()); // close the pipe's read end before the child writes
  let out = child.wait_with_output().unwrap();
  assert_eq!(out.status.code(), Some(0), "a broken pipe is a clean exit, status: {:?}", out.status);
  let stderr = String::from_utf8_lossy(&out.stderr);
  assert!(!stderr.contains("panicked"), "no traceback on a broken pipe, got: {stderr}");
}

#[test]
fn schema_prints_the_shipped_json_schema() {
  let dir = tempdir("schema");
  let out = beecast(&["schema"], &dir);
  assert!(out.status.success());
  let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
  assert_eq!(v["properties"]["chapters"]["items"]["required"], serde_json::json!(["t", "title"]));
}

#[test]
fn help_and_version_work() {
  let dir = tempdir("help");
  // Explicitly asking for help prints help in every mode — the text IS the data.
  let help = beecast(&["help"], &dir);
  assert!(help.status.success());
  assert!(String::from_utf8_lossy(&help.stdout).contains("build <recording.cast>"));
  let ver = beecast(&["version"], &dir);
  let v: serde_json::Value = serde_json::from_slice(&ver.stdout).unwrap();
  assert_eq!(v["Version"]["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn bare_machine_invocation_is_a_usage_document_not_prose_help() {
  let dir = tempdir("bare");
  // A script running `beecast` with no command is malfunctioning; it gets one parseable
  // JSON error and exit 2, never the prose help banner with exit 0 on the data stream.
  let out = beecast(&[], &dir);
  assert_eq!(out.status.code(), Some(2));
  let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("usage error is clean JSON");
  assert!(v["Error"]["message"].as_str().unwrap().contains("a command is required"));
  assert_eq!(v["Error"]["stage"], "usage");
}

#[test]
fn global_json_and_color_flags_are_accepted_anywhere() {
  let dir = tempdir("global");
  // `--json` forces machine mode (redundant on a pipe, but must parse in any position);
  // `--color=never` parses; a bogus mode is a usage error with the documented remedy.
  let out = beecast(&["--json", "version", "--color=never"], &dir);
  assert!(out.status.success());
  let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
  assert_eq!(v["Version"]["version"], env!("CARGO_PKG_VERSION"));
  let bad = beecast(&["--color=rainbow", "version"], &dir);
  assert_eq!(bad.status.code(), Some(2));
  let e: serde_json::Value = serde_json::from_slice(&bad.stdout).unwrap();
  assert!(e["Error"]["message"].as_str().unwrap().contains("supported: auto, never, no"));
}

/// FNV-1a 64: a tiny, dependency-free fingerprint for pinning the generated page's exact bytes.
fn fnv1a(bytes: &[u8]) -> u64 {
  bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3))
}

/// The page must be byte-identical to what the serde-era renderer produced: these fingerprints
/// were captured from the CLI at the commit before the page pipeline moved into the
/// zero-dependency `beecast-page` crate, and they pin the fixture pages whole — template, player,
/// escaping, float formatting, and the embedded metadata document. Re-pins since that
/// capture: the 0.1.0 → 0.2.0 version bump of the footer, then 0.2.1 together with the favicon
/// swap (the bee emoji became scsh's prompt chevron on a dark tile, recolored red), then the
/// vendored asciinema-player gave way to the first-party clean-room player (the page
/// shrank ~168KB and now carries a single MIT license), then the 0.3.0 footer stamp of that
/// change's version bump, then 0.3.1 with the inline-block run fix (multi-row background
/// panels render as one solid block, no gaps between lines), then the player graduated into
/// its own `beecast-player` crate and learned live-follow (`append`), then `layout()` gained
/// horizontal centering of the terminal (ported from the scsh session browser, unifying the
/// two embeddings), then in-band "m" markers started becoming chapter ticks, then the 0.4.0
/// footer stamp of that minor bump (a new crate and a new API), then the 0.4.1 stamp of the
/// release-automation patch. When the
/// template, the player, or the workspace version changes *intentionally*, re-pin using the
/// lengths and fingerprints this assertion prints.
#[test]
fn generated_page_is_byte_identical_to_the_serde_era_renderer() {
  let dir = tempdir("pin");
  std::fs::copy(fixture("sample.cast"), dir.join("sample.cast")).unwrap();
  std::fs::copy(fixture("sample.meta.json"), dir.join("sample.meta.json")).unwrap();
  let with_meta = beecast(&["build", "sample.cast", "-o", "-"], &dir).stdout;
  // The same recording under another name and without the sidecar: the fallback-title path.
  std::fs::copy(fixture("sample.cast"), dir.join("bare.cast")).unwrap();
  let bare = beecast(&["build", "bare.cast", "-o", "-"], &dir).stdout;
  let got = (with_meta.len(), fnv1a(&with_meta), bare.len(), fnv1a(&bare));
  assert_eq!(got, (51222, 0x3df6c10502a87406, 51012, 0xb537b0bff26a3f0b), "the generated page's bytes moved");
}

/// `beecast schema` is the codegen script (§1): its output must be exactly the schema file
/// shipped in the `beecast-dto` crate, which a unit test there pins to the generated document.
#[test]
fn schema_command_matches_the_shipped_file() {
  let dir = tempdir("schemagen");
  let out = beecast(&["schema"], &dir);
  assert!(out.status.success());
  let shipped =
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../dto/schema/beecast-meta.schema.json")).unwrap();
  assert_eq!(out.stdout, shipped, "run `cargo run -p beecast -q -- schema > dto/schema/beecast-meta.schema.json`");
}
