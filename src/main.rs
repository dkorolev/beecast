//! `beecast` — Browser Cast. Turns an asciinema `.cast` recording (v1/v2/v3) plus an
//! optional metadata sidecar into one fully self-contained `.html` player page.
//!
//! CLI behavior follows ENG-PRINCIPLES §2: data on stdout and everything else on stderr;
//! human-friendly output at a TTY and two-space-indented single-key JSON otherwise
//! (successes as `{ "Ok": … }`, errors as `{ "Error": … }`); canonical syntax is what
//! `help` shows, and abbreviated invocations are resolved for humans but bounced back
//! with the canonical spelling for machines. Exit codes: 0 ok, 1 failure, 2 usage.

mod cast;
mod meta;
mod page;

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Context;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn help() -> String {
  format!(
    "beecast {VERSION} — Browser Cast\n\
     Turn an asciinema .cast recording into a single self-contained .html player page:\n\
     zero network requests, so a saved copy keeps working fully offline.\n\
     \n\
     Usage: beecast <command> [options] — a bare `beecast` prints this help\n\
     \n\
     Commands:\n\
     \x20 • build <recording.cast>    Generate the player page next to the recording.\n\
     \x20     --meta <file.json>      Metadata sidecar; default: <recording>.meta.json when it exists.\n\
     \x20     -o, --output <file>     Output path; default: <recording>.html. `-o -` writes to stdout.\n\
     \x20 • schema                    Print the metadata sidecar's JSON Schema to stdout.\n\
     \x20 • version                   Print the version.\n\
     \x20 • help [topic]              This help, or one of: build, schema, exitcodes.\n\
     \n\
     The sidecar carries {{ title, summary, chapters }}; see SCHEMA.md. Chapters are keyed\n\
     by fractional seconds and the first one must start at 0. The generated page supports\n\
     chapter navigation, 0.5×–3× speed, and ?t=<seconds>&note=<comment> deep links.\n"
  )
}

fn help_topic(topic: &str) -> Option<String> {
  match topic {
    "build" => Some(
      "beecast build <recording.cast> [--meta <file.json>] [-o <output.html>]\n\
       \n\
       Reads the recording (asciicast v1, v2, or v3) and writes one self-contained HTML\n\
       page embedding the player, the styles, the recording, and the metadata. --meta\n\
       defaults to <recording>.meta.json when that file exists; the page falls back to\n\
       the recording's filename when the metadata has no title. `-o -` streams the HTML\n\
       to stdout (diagnostics stay on stderr).\n"
        .into(),
    ),
    "schema" => Some(
      "beecast schema\n\
       \n\
       Prints the metadata sidecar's formal JSON Schema to stdout. The human-readable\n\
       rendering is SCHEMA.md; the Rust types in src/meta.rs are the source of truth.\n"
        .into(),
    ),
    "exitcodes" => Some(
      "Exit codes:\n\
       \x20 0  success\n\
       \x20 1  failure (unreadable input, invalid cast or metadata, write error)\n\
       \x20 2  usage (unknown command or flag, missing argument, non-canonical machine invocation)\n"
        .into(),
    ),
    _ => None,
  }
}

/// One parsed `build` invocation.
struct BuildArgs {
  cast: PathBuf,
  /// Explicit `--meta`; `None` means "use `<recording>.meta.json` when present."
  meta: Option<PathBuf>,
  /// Explicit `-o`; `None` means `<recording>.html`. `Some("-")` streams to stdout.
  output: Option<PathBuf>,
}

/// A usage-level error: wrong invocation, not a failed run. Reported with exit code 2.
struct Usage(String);

fn main() -> ExitCode {
  let args: Vec<String> = std::env::args().skip(1).collect();
  let machine = !std::io::stdout().is_terminal();
  match dispatch(&args, machine) {
    Ok(()) => ExitCode::SUCCESS,
    Err(Fail::Usage(msg)) => {
      report_error(&msg, machine, true);
      ExitCode::from(2)
    }
    Err(Fail::Run(e)) => {
      report_error(&format!("{e:#}"), machine, false);
      ExitCode::FAILURE
    }
  }
}

/// The two failure planes, kept apart so `main` can map them to exit codes 2 and 1.
enum Fail {
  Usage(String),
  Run(anyhow::Error),
}

impl From<Usage> for Fail {
  fn from(u: Usage) -> Self {
    Fail::Usage(u.0)
  }
}
impl From<anyhow::Error> for Fail {
  fn from(e: anyhow::Error) -> Self {
    Fail::Run(e)
  }
}

fn dispatch(args: &[String], machine: bool) -> Result<(), Fail> {
  let first = args.first().map(String::as_str);
  match first {
    None | Some("help") | Some("--help") | Some("-h") => {
      match args.get(1) {
        Some(topic) => match help_topic(topic) {
          Some(text) => print!("{text}"),
          None => return Err(Usage(format!("unknown help topic `{topic}` (topics: build, schema, exitcodes)")).into()),
        },
        None => print!("{}", help()),
      }
      Ok(())
    }
    Some("version") | Some("--version") | Some("-V") => {
      if machine {
        println!("{}", json_ok(&serde_json::json!({ "version": VERSION })));
      } else {
        println!("beecast {VERSION}");
      }
      Ok(())
    }
    Some("schema") => {
      // Data → stdout, verbatim: the schema file itself is the machine-readable output.
      print!("{}", meta::JSON_SCHEMA);
      Ok(())
    }
    Some("build") => Ok(run_build(parse_build_args(&args[1..])?, machine)?),
    // Liberal acceptance (§2): `beecast demo.cast` unambiguously means `build` — resolve
    // it for a human (teaching the canonical spelling on stderr), refuse it for a machine
    // (abbreviations rot in scripts).
    Some(castish) if castish.ends_with(".cast") => {
      if machine {
        return Err(Usage(format!("canonical syntax required for machines: beecast build {castish} [...]")).into());
      }
      eprintln!("note: resolved to the canonical `beecast build {castish}`");
      Ok(run_build(parse_build_args(args)?, machine)?)
    }
    Some(other) => Err(Usage(format!("unknown command `{other}` — run `beecast help`")).into()),
  }
}

fn parse_build_args(args: &[String]) -> Result<BuildArgs, Usage> {
  let mut cast: Option<PathBuf> = None;
  let mut meta: Option<PathBuf> = None;
  let mut output: Option<PathBuf> = None;
  let mut it = args.iter();
  while let Some(a) = it.next() {
    match a.as_str() {
      "--meta" => meta = Some(PathBuf::from(it.next().ok_or(Usage("--meta needs a file argument".into()))?)),
      "-o" | "--output" => {
        output = Some(PathBuf::from(it.next().ok_or(Usage("-o/--output needs a file argument".into()))?))
      }
      flag if flag.starts_with('-') && flag != "-" => return Err(Usage(format!("unknown flag `{flag}`"))),
      positional => {
        if cast.replace(PathBuf::from(positional)).is_some() {
          return Err(Usage("build takes exactly one <recording.cast>".into()));
        }
      }
    }
  }
  Ok(BuildArgs {
    cast: cast.ok_or(Usage("usage: beecast build <recording.cast> [--meta <f>] [-o <f>]".into()))?,
    meta,
    output,
  })
}

fn run_build(args: BuildArgs, machine: bool) -> anyhow::Result<()> {
  let cast_path = &args.cast;
  let ndjson =
    std::fs::read_to_string(cast_path).with_context(|| format!("cannot read recording `{}`", cast_path.display()))?;
  let info =
    cast::inspect(&ndjson).with_context(|| format!("`{}` is not a playable recording", cast_path.display()))?;

  let (meta, meta_source) = load_meta(&args)?;
  if let Some(src) = &meta_source {
    eprintln!("using metadata from `{}`", src.display());
  }
  // A chapter past the end still renders (the player clamps the seek), but it almost
  // certainly means the sidecar belongs to a different recording — warn, don't fail.
  if let Some(duration) = info.duration {
    for c in meta.chapters.iter().filter(|c| c.t > duration) {
      eprintln!("warning: chapter `{}` at t={} is past the end of the recording ({duration:.1}s)", c.title, c.t);
    }
  }

  let fallback_title = cast_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or("cast".into());
  let html = page::build_page(&ndjson, &meta, &fallback_title);

  let to_stdout = args.output.as_deref() == Some(Path::new("-"));
  if to_stdout {
    std::io::stdout().write_all(html.as_bytes()).context("writing HTML to stdout")?;
    eprintln!("asciicast v{}, {} bytes of HTML to stdout", info.version, html.len());
    return Ok(());
  }
  let out_path = args.output.unwrap_or_else(|| args.cast.with_extension("html"));
  std::fs::write(&out_path, &html).with_context(|| format!("cannot write `{}`", out_path.display()))?;
  if machine {
    println!(
      "{}",
      json_ok(&serde_json::json!({
        "output": out_path.to_string_lossy(),
        "bytes": html.len(),
        "cast_version": info.version,
        "chapters": meta.chapters.len(),
      }))
    );
  } else {
    println!(
      "wrote {} ({} KB, asciicast v{}, {} chapters)",
      out_path.display(),
      html.len() / 1024,
      info.version,
      meta.chapters.len()
    );
  }
  Ok(())
}

/// Resolve which sidecar to use: an explicit `--meta` must exist and parse; the implicit
/// `<recording>.meta.json` is used only when present. Returns the metadata and where it
/// came from (`None` when the recording is played bare).
fn load_meta(args: &BuildArgs) -> anyhow::Result<(meta::CastMeta, Option<PathBuf>)> {
  let implicit = args.cast.with_extension("meta.json");
  let path = match &args.meta {
    Some(explicit) => explicit.clone(),
    None if implicit.is_file() => implicit,
    None => return Ok((meta::CastMeta::default(), None)),
  };
  let json = std::fs::read_to_string(&path).with_context(|| format!("cannot read metadata `{}`", path.display()))?;
  let parsed = meta::parse(&json).with_context(|| format!("invalid metadata in `{}`", path.display()))?;
  Ok((parsed, Some(path)))
}

/// Two-space-indented single-key success envelope, per the house JSON conventions.
fn json_ok(payload: &serde_json::Value) -> String {
  serde_json::to_string_pretty(&serde_json::json!({ "Ok": payload })).expect("json_ok serializes")
}

/// Errors mirror regular output: plain text (colored at a TTY unless NO_COLOR) on stderr
/// for humans, `{ "Error": { … } }` on stdout for machines — a harness always sees clean
/// JSON on the data stream.
fn report_error(message: &str, machine: bool, usage: bool) {
  if machine {
    let e = serde_json::json!({ "Error": { "message": message, "usage": usage } });
    println!("{}", serde_json::to_string_pretty(&e).expect("error envelope serializes"));
  } else {
    let color = std::env::var_os("NO_COLOR").is_none() && std::io::stderr().is_terminal();
    if color {
      eprintln!("\x1b[1;31merror:\x1b[0m {message}");
    } else {
      eprintln!("error: {message}");
    }
  }
}
