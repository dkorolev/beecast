#!/usr/bin/env python3
"""seecast — annotate an asciinema .cast recording with { title, summary, chapters }.

Renders the recording into a compact timestamped transcript, hands it to `cursor-agent -p`
(headless, Composer Fast by default), validates the reply against the cast-metadata schema
(see dto/SCHEMA.md; strictly ascending fractional-second timekeys, first chapter pinned to 0),
and writes the sidecar next to the recording: demo.cast -> demo.meta.json.

Stdlib-only and single-file on purpose: this exact file is what the scsh skill bundles,
so it must run anywhere python3 exists. Tested by seecast/tests/test_seecast.py
(transcript, v1/v2/v3 timing, ANSI stripping, validation, the annotate flow with a stubbed
model, and the CLI contract); the cursor-agent path is exercised for real by running it
on an actual recording.

CLI contract (ENG-PRINCIPLES §2): data -> stdout, diagnostics -> stderr. In machine
mode (`--json`, or stdout is not a TTY) the result is a two-space-indented single-key JSON
document with a request-specific variant -- `{ "Annotated": ... }`, `{ "Valid": ... }`,
`{ "Version": ... }`, or `{ "Error": { message, stage } }` where `stage` is `usage` or
`request` -- except in the explicit stream modes (`--transcript`, `-o -`), where the
document itself is the data. Color obeys `--color=never|no` and `NO_COLOR`.
Exit codes: 0 ok, 1 failure, 2 usage, 130 interrupted (Ctrl+C); a broken pipe ends the
program quietly. `seecast help exitcodes` prints the full table. External-call discipline
(§9): a liveness tick on stderr every ~10 seconds while cursor-agent runs, and a
hard watchdog (default 180 s) kills it.
"""

import argparse
import json
import math
import os
import signal
import subprocess
import sys
import tempfile
import time

VERSION = "0.2.0"  # what build is this? -- answerable offline, always
DEFAULT_MODEL = "composer-2.5-fast"  # Composer Fast: quick and cheap, plenty for titling.
DEFAULT_TIMEOUT = 180  # seconds; annotation is a known-fast job, well under the 5-min default cap
LIVENESS_PERIOD = 10  # seconds between "still waiting" ticks on stderr
TRANSCRIPT_MAX_LINES = 120  # keeps the prompt small; plenty of signal for chaptering


def strip_ansi(s):
    """Strip ANSI/VT control sequences (CSI, OSC, lone escapes) from `s`, leaving text.
    Carriage returns rewrite the current line in a terminal; treat them as newlines."""
    out = []
    i, n = 0, len(s)
    while i < n:
        ch = s[i]
        if ch == "\x1b":
            nxt = s[i + 1] if i + 1 < n else ""
            if nxt == "[":  # CSI: ESC [ ... <final 0x40..0x7e>
                i += 2
                while i < n and not ("\x40" <= s[i] <= "\x7e"):
                    i += 1
                i += 1
            elif nxt == "]":  # OSC: ESC ] ... (BEL | ESC \)
                i += 2
                while i < n and s[i] != "\x07" and not (s[i] == "\x1b" and i + 1 < n and s[i + 1] == "\\"):
                    i += 1
                i += 2 if i < n and s[i] == "\x1b" else 1
            else:  # other two-byte escape (charset selection, etc.)
                i += 2
        elif ch == "\r":
            out.append("\n")
            i += 1
        elif ch < "\x20" and ch not in "\n\t":
            i += 1  # drop other control bytes
        else:
            out.append(ch)
            i += 1
    return "".join(out)


def iter_output_events(cast_ndjson):
    """Yield (absolute_seconds, text) for each output event of an asciicast v1, v2, or v3
    recording. A v1 recording is one JSON document whose `stdout` pairs carry intervals;
    v2 stamps are absolute; v3 stamps are intervals since the previous event. Intervals
    get accumulated. Unparseable lines (e.g. a truncated live tail) are skipped."""
    lines = cast_ndjson.splitlines()
    # The header is the first non-empty line, exactly like the Rust parser (cast.rs) — a
    # recording beecast can play must never be one seecast refuses to annotate. An empty
    # file is not an asciicast, not an empty transcript.
    header_index = next((i for i, line in enumerate(lines) if line.strip()), None)
    if header_index is None:
        raise ValueError("not an asciicast: the file is empty")
    try:
        header = json.loads(lines[header_index])
    except ValueError:
        # A v1 recording may be one pretty-printed JSON document spanning many lines: when
        # the first line does not parse on its own, the whole file gets one more chance —
        # exactly like the Rust parser (cast.rs).
        try:
            header = json.loads(cast_ndjson)
        except ValueError:
            raise ValueError("not an asciicast: the first line is not a JSON header")
    try:
        version = int(header.get("version", 0))
    except (ValueError, TypeError, AttributeError):
        raise ValueError("not an asciicast: the first line is not a JSON header")
    if version == 1:
        # v1 carries its events inside the header document itself: `stdout` pairs of
        # [interval_seconds, text], mirroring the player's own parseCast (vt.js).
        clock = 0.0
        for pair in header.get("stdout") or []:
            if not (isinstance(pair, list) and len(pair) >= 2 and isinstance(pair[0], (int, float))):
                continue
            clock += float(pair[0])
            if isinstance(pair[1], str):
                yield clock, pair[1]
        return
    if version not in (2, 3):
        raise ValueError("asciicast v%s is not supported (v1, v2, and v3 are)" % version)
    clock = 0.0
    for line in lines[header_index + 1 :]:
        line = line.strip()
        if not line:
            continue
        try:
            ev = json.loads(line)
        except ValueError:
            continue
        if not (isinstance(ev, list) and len(ev) >= 3 and isinstance(ev[0], (int, float))):
            continue
        t = float(ev[0])
        clock = clock + t if version == 3 else t  # v3: relative intervals; v2: absolute
        if ev[1] == "o" and isinstance(ev[2], str):
            yield clock, ev[2]


def transcript(cast_ndjson, max_lines=TRANSCRIPT_MAX_LINES):
    """Render a recording into `[<secs>s] visible text` lines: ANSI-stripped, whitespace-
    normalized, consecutive duplicates collapsed (TUI redraws are repetitive), then evenly
    downsampled to at most `max_lines` in chronological order. Timestamps stay fractional
    so the model can place chapters on sub-second boundaries — the timekey is a float."""
    events = []
    last = ""
    for t, data in iter_output_events(cast_ndjson):
        for raw in strip_ansi(data).split("\n"):
            text = " ".join(raw.split())
            if not text or text == last:
                continue
            # Spinner frames and redraw slivers ("✶", "i…", "✻3") are pure noise that would
            # crowd real lines out of the downsample budget; three alphanumerics is the
            # cheapest reliable tell of actual content.
            if sum(c.isalnum() for c in text) < 3:
                continue
            last = text
            events.append((t, text[:200]))
    step = math.ceil(len(events) / max_lines) if len(events) > max_lines else 1
    return "\n".join("[%.1fs] %s" % (t, text) for t, text in events[::step])


def build_prompt(transcript_text):
    """The prompt handed to cursor-agent, embedding the transcript."""
    return (
        "Below is a timestamped transcript of a terminal-session screen recording. "
        "Produce a JSON object describing it.\n\n"
        "Output ONLY the JSON - no prose, no markdown code fence. Schema:\n"
        '{"title": "<3-8 word human title for the recording>", '
        '"summary": "<one or two sentences: what the session did>", '
        '"chapters": [{"t": <seconds into the recording, may be fractional e.g. 12.5>, '
        '"title": "<3-6 word phase name>"}]}\n\n'
        "Use between 3 and 8 chapters, in ascending time order. The FIRST chapter MUST "
        "start at t=0 (the beginning). Each chapter marks a distinct phase; keep titles "
        "terse. No keys other than the three above.\n\n"
        "TRANSCRIPT:\n" + transcript_text
    )


def reject_duplicate_keys(pairs):
    """`object_pairs_hook` that refuses duplicated JSON keys. Python's json module keeps
    the last duplicate silently; serde (the Rust side) rejects them — so without this,
    seecast would certify sidecars that beecast then refuses to build."""
    obj = {}
    for key, value in pairs:
        if key in obj:
            raise ValueError("duplicate field `%s`" % key)
        obj[key] = value
    return obj


def extract_json(reply):
    """Take the first `{` .. last `}` slice of a model reply (which may wrap the JSON in
    prose or a code fence despite instructions) and parse it. Raises ValueError."""
    start, end = reply.find("{"), reply.rfind("}")
    if start < 0 or end < start:
        raise ValueError("the reply contains no JSON object")
    return json.loads(reply[start : end + 1], object_pairs_hook=reject_duplicate_keys)


def atomic_write_text(path, text):
    """Replace `path` only after a complete, flushed sibling temporary is ready."""
    parent = os.path.dirname(os.path.abspath(path))
    fd, tmp_path = tempfile.mkstemp(prefix=".seecast-", suffix=".tmp", dir=parent, text=True)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            f.write(text)
            f.flush()
            os.fsync(f.fileno())
        os.replace(tmp_path, path)
    except BaseException:
        try:
            os.unlink(tmp_path)
        except FileNotFoundError:
            pass
        raise


def validate_meta(obj, generated=False, require_all=False):
    """Validate `obj` against the cast-metadata schema and return it normalized.

    Strict, mirroring beecast's parser: unknown keys anywhere, wrong types, empty strings,
    non-finite or negative timekeys, ties, or descending chapters are all hard errors.
    With `generated=True` (a model reply about to become a sidecar) two normalizations
    are applied first — chapters are sorted by `t` and the first one is pinned to 0,
    YouTube-style — and title/summary/chapters are all required, not optional.
    With `require_all=True` the three fields are required but NOTHING is normalized: the
    shape a finished generated sidecar must already have, used to gate files on disk
    (a gate that silently re-sorted or re-pinned would pass files beecast then rejects).
    """
    if not isinstance(obj, dict):
        raise ValueError("the metadata must be a JSON object")
    unknown = sorted(set(obj) - {"title", "summary", "chapters"})
    if unknown:
        raise ValueError("unknown fields: %s" % ", ".join(unknown))
    meta = {}
    for field in ("title", "summary"):
        value = obj.get(field)
        if value is None:
            # JSON `null` and an absent key are the same thing to Rust's Option<String>;
            # mirror that here so a sidecar beecast accepts never fails seecast validation.
            if generated or require_all:
                raise ValueError("`%s` is required" % field)
            continue
        if not isinstance(value, str) or not value.strip():
            raise ValueError("`%s` must be a non-empty string" % field)
        meta[field] = value.strip()
    # An explicit `null` for chapters stays an error: Rust's Vec<Chapter> rejects it too.
    chapters = obj.get("chapters", [])
    if not isinstance(chapters, list):
        raise ValueError("`chapters` must be an array")
    if (generated or require_all) and not chapters:
        raise ValueError("at least one chapter is required")
    normalized = []
    for i, ch in enumerate(chapters):
        if not isinstance(ch, dict):
            raise ValueError("chapter %d: not an object" % i)
        extra = sorted(set(ch) - {"t", "title"})
        if extra:
            raise ValueError("chapter %d: unknown fields: %s" % (i, ", ".join(extra)))
        if "t" not in ch or "title" not in ch:
            raise ValueError("chapter %d: `t` and `title` are both required" % i)
        t, title = ch["t"], ch["title"]
        if isinstance(t, bool) or not isinstance(t, (int, float)):
            raise ValueError("chapter %d: `t` must be a finite number of seconds >= 0" % i)
        try:
            # An int too large for a float would make math.isfinite raise OverflowError,
            # escaping the ValueError contract; convert first and catch the overflow.
            t = float(t)
        except OverflowError:
            raise ValueError("chapter %d: `t` must be a finite number of seconds >= 0" % i)
        if not math.isfinite(t) or t < 0:
            raise ValueError("chapter %d: `t` must be a finite number of seconds >= 0" % i)
        if not isinstance(title, str) or not title.strip():
            raise ValueError("chapter %d: `title` must be a non-empty string" % i)
        normalized.append({"t": t, "title": title.strip()})
    if generated and normalized:
        normalized.sort(key=lambda c: c["t"])
        normalized[0]["t"] = 0.0  # the opening segment always gets a marker
        # The pin (or a sloppy model) can leave tied timekeys, which the strict ascending
        # check below would reject; collapse a tie to its first chapter — normalization of
        # generated content, exactly like the pin itself.
        deduped = [normalized[0]]
        for ch in normalized[1:]:
            if ch["t"] > deduped[-1]["t"]:
                deduped.append(ch)
        normalized = deduped
    for i in range(len(normalized)):
        if i == 0 and normalized and normalized[0]["t"] != 0.0:
            raise ValueError("the first chapter must start at t = 0, got t = %g" % normalized[0]["t"])
        if i > 0 and normalized[i]["t"] <= normalized[i - 1]["t"]:
            raise ValueError(
                "chapters must be strictly ascending by `t`: chapter %d has t = %g after t = %g"
                % (i, normalized[i]["t"], normalized[i - 1]["t"])
            )
    if normalized or "chapters" in obj:
        meta["chapters"] = normalized
    return meta


def run_cursor_agent(model, prompt, timeout=DEFAULT_TIMEOUT):
    """Run cursor-agent headless with `prompt`, returning its stdout. Runs in an empty
    temp dir (the prompt is self-contained). Emits a liveness tick on stderr every
    ~10 s and kills the child past `timeout` — a hung annotation never stalls the caller.
    The child never outlives this function: whatever exits the loop (a result, the
    watchdog, Ctrl+C), a still-running cursor-agent is killed and reaped on the way out."""
    with tempfile.TemporaryDirectory(prefix="seecast-") as tmp:
        proc = subprocess.Popen(
            ["cursor-agent", "-p", "--force", "--output-format", "text", "--model", model, prompt],
            cwd=tmp,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
        try:
            start = time.monotonic()
            next_tick = start + LIVENESS_PERIOD
            while True:
                try:
                    stdout, _ = proc.communicate(timeout=min(1.0, max(0.0, next_tick - time.monotonic())) + 0.05)
                    return stdout.decode("utf-8", "replace")
                except subprocess.TimeoutExpired:
                    elapsed = time.monotonic() - start
                    if elapsed >= timeout:
                        raise RuntimeError("cursor-agent produced no result within %d s and was killed" % timeout)
                    if time.monotonic() >= next_tick:
                        print("seecast: waiting for cursor-agent (%ds elapsed)" % int(elapsed), file=sys.stderr)
                        next_tick += LIVENESS_PERIOD
        finally:
            if proc.poll() is None:
                proc.kill()
                proc.communicate()


def annotate(cast_path, model=DEFAULT_MODEL, timeout=DEFAULT_TIMEOUT, run=run_cursor_agent, warnings=None):
    """The full flow: read -> transcript -> model -> validate. Returns the metadata dict.
    `run` is injectable so tests can stub the model call. A watchdog-killed call is
    retried ONCE: cursor-agent occasionally stalls on startup and then answers a fresh
    call within seconds, so one retry turns a flaky external into a reliable command
    without hiding that anything happened (§9). The retry is a warning, and warnings land
    in BOTH channels (§2): on stderr immediately, and appended to the caller's `warnings`
    list so machine mode can fold them into the `Annotated` document."""
    with open(cast_path, "r", encoding="utf-8", errors="replace") as f:
        text = transcript(f.read())
    if not text.strip():
        raise ValueError("the recording has no visible output to annotate")
    prompt = build_prompt(text)
    try:
        reply = run(model, prompt, timeout)
    except RuntimeError as e:
        message = "%s; retrying once" % e
        if warnings is not None:
            warnings.append(message)
        print("seecast: %s" % message, file=sys.stderr)
        reply = run(model, prompt, timeout)
    return validate_meta(extract_json(reply), generated=True)


def fail(message, code=1, stage="request", machine=None, color="auto"):
    """Report an error per the house CLI rules and exit: human text on stderr in human
    mode, a single-key `{ "Error": { message, stage } }` JSON document on stdout in
    machine mode (`--json`, or stdout is not a TTY — the default of None re-derives the
    latter for callers that fail before flags are parsed). `stage` (`usage` | `request`)
    mirrors the exit code so scripts branch without decoding prose."""
    if machine is None:
        machine = not sys.stdout.isatty()
    if machine:
        print(json.dumps({"Error": {"message": message, "stage": stage}}, indent=2, ensure_ascii=False))
    else:
        colored = color not in ("never", "no") and os.environ.get("NO_COLOR") is None and sys.stderr.isatty()
        prefix = "\x1b[1;31merror:\x1b[0m" if colored else "error:"
        print("%s %s" % (prefix, message), file=sys.stderr)
    sys.exit(code)


def emit(variant, payload):
    """Print a two-space-indented single-key union document: the machine-mode result shape
    for every command, success and failure alike."""
    print(json.dumps({variant: payload}, indent=2, ensure_ascii=False))


# The exit-code table (ENG-PRINCIPLES §2). Single source: `seecast help exitcodes` prints
# it, and the argparse epilog points here rather than restating it.
EXITCODES = (
    "Exit codes:\n"
    "  0    success\n"
    "  1    failure (unreadable recording, invalid metadata, model/validation error)\n"
    "  2    usage (unknown flag, missing argument, bad --color mode)\n"
    "  130  interrupted (Ctrl+C / SIGINT)\n"
    "A broken pipe (e.g. `seecast --transcript rec.cast | head`) ends the program quietly, "
    "per SIGPIPE convention.\n"
    "Machine-mode errors also carry a `stage` field (`usage` or `request`) for scripts to branch on."
)


class Parser(argparse.ArgumentParser):
    """argparse whose usage errors follow the same contract as every other failure: exit 2,
    human prose on stderr at a TTY, an `{ "Error": ... }` JSON document on stdout otherwise
    (stock argparse prints bare prose to stderr in both modes). `main` stores the argv it
    was actually given in `raw_argv`, so a library caller's `--json` is honored too — not
    just the process's own sys.argv."""

    raw_argv = None

    def error(self, message):
        # Flags are not parsed yet when argparse errors, so `--json` is sniffed raw.
        raw = self.raw_argv if self.raw_argv is not None else sys.argv[1:]
        machine = "--json" in raw or not sys.stdout.isatty()
        if not machine:
            self.print_usage(sys.stderr)
        fail(message, code=2, stage="usage", machine=machine)


def main(argv=None):
    parser = Parser(
        prog="seecast",
        description="Annotate an asciinema .cast recording with { title, summary, chapters } "
        "metadata (see dto/SCHEMA.md), via cursor-agent on Composer Fast.",
        epilog="`seecast help exitcodes` prints the exit-code table. When stdout is not a "
        "TTY, results are single-key JSON documents (see dto/SCHEMA.md and the module docstring).",
    )
    parser.add_argument("cast", nargs="?", help="the .cast recording (asciicast v1, v2, or v3)")
    parser.add_argument("--version", action="store_true", help="print the version and exit (works offline)")
    parser.add_argument(
        "--json", action="store_true", help="machine output (single-key JSON); the default when stdout is not a TTY"
    )
    parser.add_argument(
        "--color",
        choices=("auto", "never", "no"),
        default="auto",
        help="color for human output; never/no disable it, as does NO_COLOR (default: %(default)s)",
    )
    parser.add_argument("-o", "--output", help="sidecar path; default: <recording>.meta.json ('-' = stdout only)")
    parser.add_argument("--model", default=DEFAULT_MODEL, help="cursor-agent model (default: %(default)s)")
    parser.add_argument("--timeout", type=int, default=DEFAULT_TIMEOUT, help="watchdog seconds (default: %(default)s)")
    parser.add_argument(
        "--transcript", action="store_true", help="print the compact transcript and stop (no model call)"
    )
    parser.add_argument("--validate", metavar="META_JSON", help="validate a sidecar file against the schema and stop")
    parser.add_argument(
        "--generated",
        action="store_true",
        help="with --validate: also require title, summary, and at least one chapter — "
        "the shape a generated sidecar must have (used as the scsh skill's pass gate)",
    )

    raw = sys.argv[1:] if argv is None else list(argv)
    parser.raw_argv = raw

    # `help [topic]` dispatch (matches beecast): `help` prints full help, `help exitcodes`
    # prints the table. Intercepted before parse_args, since `help` is not an argparse arg.
    # Global flags may precede the command — `seecast --json help exitcodes` is the same
    # invocation — so skip them when looking for it.
    rest = list(raw)
    while rest:
        if rest[0] == "--json" or rest[0].startswith("--color="):
            rest.pop(0)
        elif rest[0] == "--color" and len(rest) > 1:
            del rest[:2]
        else:
            break
    if rest and rest[0] == "help":
        topic = rest[1] if len(rest) > 1 else None
        if topic is None:
            parser.print_help()
        elif topic == "exitcodes":
            print(EXITCODES)
        else:
            parser.error("unknown help topic `%s` (topics: exitcodes)" % topic)
        return

    args = parser.parse_args(argv)
    machine = args.json or not sys.stdout.isatty()

    if args.version:
        if machine:
            emit("Version", {"version": VERSION})
        else:
            print("seecast %s" % VERSION)
        return

    if args.validate:
        try:
            with open(args.validate, "r", encoding="utf-8") as f:
                # The duplicate-key hook mirrors serde's strictness (see reject_duplicate_keys).
                validate_meta(json.load(f, object_pairs_hook=reject_duplicate_keys), require_all=args.generated)
        except (OSError, ValueError) as e:
            fail("invalid metadata in `%s`: %s" % (args.validate, e), machine=machine, color=args.color)
        if machine:
            emit("Valid", {"path": args.validate})
        else:
            print("`%s` conforms to the cast metadata schema" % args.validate)
        return

    if not args.cast:
        parser.error("the .cast recording argument is required")
    warnings = []
    try:
        if args.transcript:
            # Explicit stream mode: the transcript itself is the data, no envelope.
            with open(args.cast, "r", encoding="utf-8", errors="replace") as f:
                print(transcript(f.read()))
            return
        meta = annotate(args.cast, model=args.model, timeout=args.timeout, warnings=warnings)
    except (OSError, ValueError, RuntimeError) as e:
        fail(str(e), machine=machine, color=args.color)
    sidecar_json = json.dumps(meta, indent=2, ensure_ascii=False) + "\n"
    if args.output == "-":
        # Explicit stream mode: the sidecar itself is the data, no envelope; the exit
        # code is the machine's success signal.
        sys.stdout.write(sidecar_json)
        return
    out_path = args.output or os.path.splitext(args.cast)[0] + ".meta.json"
    try:
        atomic_write_text(out_path, sidecar_json)
    except OSError as e:
        # An unwritable path must be the contract error, not a traceback — especially
        # here, after the paid annotation has already succeeded.
        fail("cannot write `%s`: %s" % (out_path, e), machine=machine, color=args.color)
    if machine:
        # The write narration rides inside the document (machine-mode stderr stays quiet);
        # warnings appear here too, per §2 — a warning only on stderr is invisible to scripts.
        emit(
            "Annotated",
            {"output": out_path, "chapters": len(meta.get("chapters", [])), "warnings": warnings, "meta": meta},
        )
    else:
        # Human mode: the result line IS the output — raw JSON stays in the file and the
        # explicit stream modes, per the module docstring.
        print("wrote %s (%d chapters)" % (out_path, len(meta.get("chapters", []))))


def entry():
    """Process-level signal discipline, applied only when running as a program (importers,
    i.e. the tests, must not have their dispositions changed): a broken pipe ends the
    program quietly (the SIGPIPE default, instead of Python's BrokenPipeError traceback),
    and Ctrl+C exits 130 -- never a stack trace."""
    if hasattr(signal, "SIGPIPE"):
        signal.signal(signal.SIGPIPE, signal.SIG_DFL)
    try:
        main()
    except KeyboardInterrupt:
        sys.exit(130)


if __name__ == "__main__":
    entry()
