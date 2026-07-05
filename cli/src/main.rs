//! `beecast` — Browser Cast. Turns an asciinema `.cast` recording (v1/v2/v3) plus an
//! optional metadata sidecar into one fully self-contained `.html` player page.
//!
//! CLI behavior follows ENG-PRINCIPLES §2: data on stdout and everything else on stderr;
//! human-friendly output at a TTY and two-space-indented single-key JSON otherwise, with
//! request-specific variant names (`{ "Built": … }`, `{ "Version": … }`) and errors as
//! `{ "Error": { message, stage } }`. In machine mode stderr stays quiet — diagnostics
//! ride inside the JSON document, and warnings land in BOTH channels. Canonical syntax is
//! what `help` shows; abbreviated invocations are resolved for humans but bounced back
//! with the canonical spelling for machines. Exit codes: 0 ok, 1 failure, 2 usage,
//! 130 interrupted (SIGINT); a broken pipe ends the program quietly. All stdout data goes
//! through [`emit`], which treats a broken pipe (`beecast schema | head`) as a clean exit
//! rather than the panic the `print!` macros would raise — so no `libc`, no `unsafe`.

mod cast;
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
     Global flags (any command):\n\
     \x20   --json                    Machine output (single-key JSON); the default when stdout is not a TTY.\n\
     \x20   --color=<auto|never|no>   Color for human output; never/no disable it, as does NO_COLOR.\n\
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
       Prints the metadata sidecar's formal JSON Schema to stdout, generated from the Rust\n\
       types in the beecast-dto crate (the source of truth); dto/SCHEMA.md is the\n\
       human-readable rendering.\n"
        .into(),
    ),
    "exitcodes" => Some(
      "Exit codes:\n\
       \x20 0    success\n\
       \x20 1    failure (unreadable input, invalid cast or metadata, write error)\n\
       \x20 2    usage (unknown command or flag, missing argument, non-canonical machine invocation)\n\
       \x20 130  interrupted (Ctrl+C / SIGINT)\n\
       A broken pipe (e.g. `beecast schema | head`) ends the program quietly, per SIGPIPE convention.\n\
       Machine-mode errors also carry a `stage` field (`usage` or `request`) for scripts to branch on.\n"
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
  let mut args: Vec<String> = std::env::args().skip(1).collect();
  let global = match parse_global(&mut args) {
    Ok(g) => g,
    Err(Usage(msg)) => {
      report_error(&msg, !std::io::stdout().is_terminal(), "usage", false);
      return ExitCode::from(2);
    }
  };
  match dispatch(&args, global.machine, global.color) {
    Ok(()) => ExitCode::SUCCESS,
    Err(Fail::Usage(msg)) => {
      report_error(&msg, global.machine, "usage", global.color);
      ExitCode::from(2)
    }
    Err(Fail::Run(e)) => {
      report_error(&format!("{e:#}"), global.machine, "request", global.color);
      ExitCode::FAILURE
    }
  }
}

/// The resolved global flags: what mode output is in, and whether stderr gets color.
struct Global {
  /// Machine output: `--json` given, or stdout is not a TTY (§2).
  machine: bool,
  /// Color human diagnostics: at a TTY, unless `NO_COLOR` or `--color=never|no` said not to.
  color: bool,
}

/// Strip the global flags (`--json`, `--color=<mode>`) out of the argument list, leaving
/// the command and its own flags for `dispatch`.
fn parse_global(args: &mut Vec<String>) -> Result<Global, Usage> {
  let mut json = false;
  let mut color_off = false;
  let mut rest = Vec::with_capacity(args.len());
  for a in args.drain(..) {
    if a == "--json" {
      json = true;
    } else if let Some(mode) = a.strip_prefix("--color=") {
      match mode {
        "auto" => color_off = false,
        "never" | "no" => color_off = true,
        other => return Err(Usage(format!("unknown --color mode `{other}` (supported: auto, never, no)"))),
      }
    } else if a == "--color" {
      return Err(Usage("--color needs a mode: --color=<auto|never|no>".into()));
    } else {
      rest.push(a);
    }
  }
  *args = rest;
  let machine = json || !std::io::stdout().is_terminal();
  let color = !color_off && std::env::var_os("NO_COLOR").is_none() && std::io::stderr().is_terminal();
  Ok(Global { machine, color })
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

fn dispatch(args: &[String], machine: bool, color: bool) -> Result<(), Fail> {
  let first = args.first().map(String::as_str);
  match first {
    None | Some("help") | Some("--help") | Some("-h") => {
      match args.get(1) {
        Some(topic) => match help_topic(topic) {
          Some(text) => emit(&text),
          None => return Err(Usage(format!("unknown help topic `{topic}` (topics: build, schema, exitcodes)")).into()),
        },
        None => emit(&help()),
      }
      Ok(())
    }
    Some("version") | Some("--version") | Some("-V") => {
      if machine {
        emit(&format!("{}\n", json_doc("Version", serde_json::json!({ "version": VERSION }))));
      } else {
        emit(&format!("beecast {VERSION}\n"));
      }
      Ok(())
    }
    Some("schema") => {
      // Data → stdout, generated live from the Rust types (§1): this command IS the
      // codegen script — `beecast schema > schema/beecast-meta.schema.json` regenerates
      // the shipped file, and a unit test pins the two byte-for-byte.
      emit(&beecast_dto::generated_schema());
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
      // A nudge, not a warning: dim/grey when color is on (§2), so it reads as a hint.
      if color {
        eprintln!("\x1b[2mnote: resolved to the canonical `beecast build {castish}`\x1b[0m");
      } else {
        eprintln!("note: resolved to the canonical `beecast build {castish}`");
      }
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
  // The sidecar-discovery narration is a diagnostic: stderr for a human, a field in the
  // JSON document for a machine (§2: machine-mode stderr stays quiet).
  if !machine {
    if let Some(src) = &meta_source {
      eprintln!("using metadata from `{}`", src.display());
    }
  }
  // A chapter past the end still renders (the player clamps the seek), but it almost
  // certainly means the sidecar belongs to a different recording — warn, don't fail.
  // Warnings land in BOTH channels (§2): stderr now, and the JSON document below.
  let mut warnings: Vec<String> = Vec::new();
  if let Some(duration) = info.duration {
    for c in meta.chapters.iter().filter(|c| c.t > duration) {
      warnings.push(format!("chapter `{}` at t={} is past the end of the recording ({duration:.1}s)", c.title, c.t));
    }
  }
  for w in &warnings {
    eprintln!("warning: {w}");
  }

  let fallback_title = cast_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or("cast".into());
  let html = page::build_page(&ndjson, &meta, &fallback_title);

  let to_stdout = args.output.as_deref() == Some(Path::new("-"));
  if to_stdout {
    // `-o -` is the explicit "stream me the document" mode: the page itself is the data,
    // so there is no JSON envelope — the exit code is the machine's success signal.
    emit(&html);
    if !machine {
      eprintln!("asciicast v{}, {} bytes of HTML to stdout", info.version, html.len());
    }
    return Ok(());
  }
  let out_path = args.output.unwrap_or_else(|| args.cast.with_extension("html"));
  std::fs::write(&out_path, &html).with_context(|| format!("cannot write `{}`", out_path.display()))?;
  if machine {
    emit(&format!(
      "{}\n",
      json_doc(
        "Built",
        serde_json::json!({
          "output": out_path.to_string_lossy(),
          "bytes": html.len(),
          "cast_version": info.version,
          "chapters": meta.chapters.len(),
          "meta": meta_source.as_ref().map(|p| p.to_string_lossy()),
          "warnings": warnings,
        })
      )
    ));
  } else {
    emit(&format!(
      "wrote {} ({} KB, asciicast v{}, {} chapters)\n",
      out_path.display(),
      html.len() / 1024,
      info.version,
      meta.chapters.len()
    ));
  }
  Ok(())
}

/// Resolve which sidecar to use: an explicit `--meta` must exist and parse; the implicit
/// `<recording>.meta.json` is used only when present. Returns the metadata and where it
/// came from (`None` when the recording is played bare).
fn load_meta(args: &BuildArgs) -> anyhow::Result<(beecast_dto::CastMeta, Option<PathBuf>)> {
  let implicit = args.cast.with_extension("meta.json");
  let path = match &args.meta {
    Some(explicit) => explicit.clone(),
    None if implicit.is_file() => implicit,
    None => return Ok((beecast_dto::CastMeta::default(), None)),
  };
  let json = std::fs::read_to_string(&path).with_context(|| format!("cannot read metadata `{}`", path.display()))?;
  let parsed = beecast_dto::parse(&json).with_context(|| format!("invalid metadata in `{}`", path.display()))?;
  Ok((parsed, Some(path)))
}

/// Write `s` to stdout, treating a broken pipe as a clean exit rather than a panic (§2:
/// `beecast schema | head` is not an error). Rust leaves SIGPIPE ignored, so a reader that
/// hangs up surfaces here as a `BrokenPipe` error instead of killing the process — we honor
/// it by exiting 0 with no traceback, which is why the CLI needs neither `libc` nor
/// `unsafe`. Any other write failure (e.g. a full disk on a redirect) is a genuine error:
/// reported on stderr, exit 1.
fn emit(s: &str) {
  let mut out = std::io::stdout();
  if let Err(e) = out.write_all(s.as_bytes()).and_then(|()| out.flush()) {
    if e.kind() == std::io::ErrorKind::BrokenPipe {
      std::process::exit(0);
    }
    eprintln!("error: writing to stdout: {e}");
    std::process::exit(1);
  }
}

/// Two-space-indented single-key union document (§2): the variant name is request-specific
/// (`Built`, `Version`, `Error`), so scripts branch on the top-level key.
fn json_doc(variant: &str, payload: serde_json::Value) -> String {
  serde_json::to_string_pretty(&serde_json::json!({ variant: payload })).expect("json_doc serializes")
}

/// Errors mirror regular output: plain text (colored per the resolved `--color`/`NO_COLOR`
/// mode) on stderr for humans, `{ "Error": { message, stage } }` on stdout for machines —
/// a harness always sees clean JSON on the data stream. `stage` (`"usage"` | `"request"`)
/// mirrors the exit code so scripts branch without decoding prose.
fn report_error(message: &str, machine: bool, stage: &str, color: bool) {
  if machine {
    emit(&format!("{}\n", json_doc("Error", serde_json::json!({ "message": message, "stage": stage }))));
  } else {
    if color {
      eprintln!("\x1b[1;31merror:\x1b[0m {message}");
    } else {
      eprintln!("error: {message}");
    }
  }
}
