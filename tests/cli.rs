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

fn tempdir(tag: &str) -> PathBuf {
  let dir = std::env::temp_dir().join(format!("beecast-cli-{tag}-{}", std::process::id()));
  let _ = std::fs::remove_dir_all(&dir);
  std::fs::create_dir_all(&dir).unwrap();
  dir
}

#[test]
fn build_discovers_the_sidecar_and_reports_json_ok() {
  let dir = tempdir("ok");
  std::fs::copy(fixture("sample.cast"), dir.join("sample.cast")).unwrap();
  std::fs::copy(fixture("sample.meta.json"), dir.join("sample.meta.json")).unwrap();

  let out = beecast(&["build", "sample.cast"], &dir);
  assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));

  // Machine mode (stdout is a pipe here): a single-key `Ok` envelope on stdout.
  let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("clean JSON on stdout");
  assert_eq!(v["Ok"]["cast_version"], 3);
  assert_eq!(v["Ok"]["chapters"], 2);
  // The implicit `sample.meta.json` discovery is narrated on stderr, not stdout.
  assert!(String::from_utf8_lossy(&out.stderr).contains("sample.meta.json"));

  let html = std::fs::read_to_string(dir.join("sample.html")).unwrap();
  assert!(html.contains("<title>Sample session</title>"));
  assert!(html.contains("Echoes a greeting"));
  assert!(html.contains("\"title\":\"The build\""));
  let _ = std::fs::remove_dir_all(&dir);
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
  let _ = std::fs::remove_dir_all(&dir);
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
  assert!(!dir.join("s.html").exists(), "no output written on failure");
  let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn junk_input_fails_and_usage_errors_exit_2() {
  let dir = tempdir("usage");
  std::fs::write(dir.join("junk.cast"), "not a cast at all").unwrap();
  assert_eq!(beecast(&["build", "junk.cast"], &dir).status.code(), Some(1));
  assert_eq!(beecast(&["frobnicate"], &dir).status.code(), Some(2));
  assert_eq!(beecast(&["build"], &dir).status.code(), Some(2));
  assert_eq!(beecast(&["build", "a.cast", "--wat"], &dir).status.code(), Some(2));
  let _ = std::fs::remove_dir_all(&dir);
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
  assert!(!dir.join("s.html").exists());
  let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn schema_prints_the_shipped_json_schema() {
  let dir = tempdir("schema");
  let out = beecast(&["schema"], &dir);
  assert!(out.status.success());
  let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
  assert_eq!(v["properties"]["chapters"]["items"]["required"], serde_json::json!(["t", "title"]));
  let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn help_and_version_work() {
  let dir = tempdir("help");
  let help = beecast(&["help"], &dir);
  assert!(help.status.success());
  assert!(String::from_utf8_lossy(&help.stdout).contains("build <recording.cast>"));
  let ver = beecast(&["version"], &dir);
  let v: serde_json::Value = serde_json::from_slice(&ver.stdout).unwrap();
  assert_eq!(v["Ok"]["version"], env!("CARGO_PKG_VERSION"));
  let _ = std::fs::remove_dir_all(&dir);
}
