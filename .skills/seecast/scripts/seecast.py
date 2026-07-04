#!/usr/bin/env python3
"""seecast — annotate an asciinema .cast recording with { title, summary, chapters }.

Renders the recording into a compact timestamped transcript, hands it to `cursor-agent -p`
(headless, Composer Fast by default), validates the reply against the cast-metadata schema
(see SCHEMA.md; strictly ascending fractional-second timekeys, first chapter pinned to 0),
and writes the sidecar next to the recording: demo.cast -> demo.meta.json.

Stdlib-only and single-file on purpose: this exact file is what the scsh skill bundles,
so it must run anywhere python3 exists. Tested by seecast/tests/test_seecast.py
(transcript, v2/v3 timing, ANSI stripping, validation, the annotate flow with a stubbed
model, and the CLI contract); the cursor-agent path is exercised for real by running it
on an actual recording.

CLI contract (ENG-PRINCIPLES paragraph 2): data -> stdout, diagnostics -> stderr. When stdout
is not a TTY the result is a two-space-indented single-key JSON document with a
request-specific variant -- `{ "Annotated": ... }`, `{ "Valid": ... }`, `{ "Version": ... }`,
or `{ "Error": { message, stage } }` where `stage` is `usage` or `request` -- except in the
explicit stream modes (`--transcript`, `-o -`), where the document itself is the data.
Exit codes: 0 ok, 1 failure, 2 usage, 130 interrupted (Ctrl+C); a broken pipe ends the
program quietly. External-call discipline (paragraph 9): a liveness tick on stderr every
~10 seconds while cursor-agent runs, and a hard watchdog (default 180 s) kills it.
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

VERSION = "0.1.0"  # what build is this? -- answerable offline, always
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
    """Yield (absolute_seconds, text) for each `o` event of an asciicast v2 or v3 NDJSON
    recording. v2 stamps are absolute; v3 stamps are intervals since the previous event
    and get accumulated. Unparseable lines (e.g. a truncated live tail) are skipped."""
    lines = cast_ndjson.splitlines()
    if not lines:
        return
    try:
        header = json.loads(lines[0])
        version = int(header.get("version", 0))
    except (ValueError, TypeError, AttributeError):
        raise ValueError("not an asciicast: the first line is not a JSON header")
    if version not in (2, 3):
        raise ValueError("asciicast v%s is not supported (v2 and v3 are)" % version)
    clock = 0.0
    for line in lines[1:]:
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


def extract_json(reply):
    """Take the first `{` .. last `}` slice of a model reply (which may wrap the JSON in
    prose or a code fence despite instructions) and parse it. Raises ValueError."""
    start, end = reply.find("{"), reply.rfind("}")
    if start < 0 or end < start:
        raise ValueError("the reply contains no JSON object")
    return json.loads(reply[start : end + 1])


def validate_meta(obj, generated=False):
    """Validate `obj` against the cast-metadata schema and return it normalized.

    Strict, mirroring beecast's parser: unknown keys anywhere, wrong types, empty strings,
    non-finite or negative timekeys, ties, or descending chapters are all hard errors.
    With `generated=True` (a model reply about to become a sidecar) two normalizations
    are applied first — chapters are sorted by `t` and the first one is pinned to 0,
    YouTube-style — and title/summary/chapters are all required, not optional.
    """
    if not isinstance(obj, dict):
        raise ValueError("the metadata must be a JSON object")
    unknown = sorted(set(obj) - {"title", "summary", "chapters"})
    if unknown:
        raise ValueError("unknown fields: %s" % ", ".join(unknown))
    meta = {}
    for field in ("title", "summary"):
        if field in obj:
            value = obj[field]
            if not isinstance(value, str) or not value.strip():
                raise ValueError("`%s` must be a non-empty string" % field)
            meta[field] = value.strip()
        elif generated:
            raise ValueError("`%s` is required" % field)
    chapters = obj.get("chapters", [])
    if not isinstance(chapters, list):
        raise ValueError("`chapters` must be an array")
    if generated and not chapters:
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
        if isinstance(t, bool) or not isinstance(t, (int, float)) or not math.isfinite(t) or t < 0:
            raise ValueError("chapter %d: `t` must be a finite number of seconds >= 0" % i)
        if not isinstance(title, str) or not title.strip():
            raise ValueError("chapter %d: `title` must be a non-empty string" % i)
        normalized.append({"t": float(t), "title": title.strip()})
    if generated and normalized:
        normalized.sort(key=lambda c: c["t"])
        normalized[0]["t"] = 0.0  # the opening segment always gets a marker
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
    ~10 s and kills the child past `timeout` — a hung annotation never stalls the caller."""
    with tempfile.TemporaryDirectory(prefix="seecast-") as tmp:
        proc = subprocess.Popen(
            ["cursor-agent", "-p", "--force", "--output-format", "text", "--model", model, prompt],
            cwd=tmp,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
        start = time.monotonic()
        next_tick = start + LIVENESS_PERIOD
        while True:
            try:
                stdout, _ = proc.communicate(timeout=min(1.0, max(0.0, next_tick - time.monotonic())) + 0.05)
                return stdout.decode("utf-8", "replace")
            except subprocess.TimeoutExpired:
                elapsed = time.monotonic() - start
                if elapsed >= timeout:
                    proc.kill()
                    proc.communicate()
                    raise RuntimeError("cursor-agent produced no result within %d s and was killed" % timeout)
                if time.monotonic() >= next_tick:
                    print("seecast: waiting for cursor-agent (%ds elapsed)" % int(elapsed), file=sys.stderr)
                    next_tick += LIVENESS_PERIOD


def annotate(cast_path, model=DEFAULT_MODEL, timeout=DEFAULT_TIMEOUT, run=run_cursor_agent):
    """The full flow: read -> transcript -> model -> validate. Returns the metadata dict.
    `run` is injectable so tests can stub the model call."""
    with open(cast_path, "r", encoding="utf-8", errors="replace") as f:
        text = transcript(f.read())
    if not text.strip():
        raise ValueError("the recording has no visible output to annotate")
    reply = run(model, build_prompt(text), timeout)
    return validate_meta(extract_json(reply), generated=True)


def fail(message, code=1, stage="request"):
    """Report an error per the house CLI rules and exit: human text on stderr at a TTY, a
    single-key `{ "Error": { message, stage } }` JSON document on stdout otherwise. `stage`
    (`usage` | `request`) mirrors the exit code so scripts branch without decoding prose."""
    if sys.stdout.isatty():
        color = os.environ.get("NO_COLOR") is None and sys.stderr.isatty()
        prefix = "\x1b[1;31merror:\x1b[0m" if color else "error:"
        print("%s %s" % (prefix, message), file=sys.stderr)
    else:
        print(json.dumps({"Error": {"message": message, "stage": stage}}, indent=2))
    sys.exit(code)


def emit(variant, payload):
    """Print a two-space-indented single-key union document: the machine-mode result shape
    for every command, success and failure alike."""
    print(json.dumps({variant: payload}, indent=2, ensure_ascii=False))


class Parser(argparse.ArgumentParser):
    """argparse whose usage errors follow the same contract as every other failure: exit 2,
    human prose on stderr at a TTY, an `{ "Error": ... }` JSON document on stdout otherwise
    (stock argparse prints bare prose to stderr in both modes)."""

    def error(self, message):
        if sys.stdout.isatty():
            self.print_usage(sys.stderr)
        fail(message, code=2, stage="usage")


def main(argv=None):
    parser = Parser(
        prog="seecast",
        description="Annotate an asciinema .cast recording with { title, summary, chapters } "
        "metadata (see SCHEMA.md), via cursor-agent on Composer Fast.",
        epilog="exit codes: 0 ok, 1 failure, 2 usage, 130 interrupted (Ctrl+C); "
        "a broken pipe ends the program quietly. When stdout is not a TTY, results are "
        "single-key JSON documents (see SCHEMA.md and the module docstring).",
    )
    parser.add_argument("cast", nargs="?", help="the .cast recording (asciicast v2 or v3)")
    parser.add_argument("--version", action="store_true", help="print the version and exit (works offline)")
    parser.add_argument("-o", "--output", help="sidecar path; default: <recording>.meta.json ('-' = stdout only)")
    parser.add_argument("--model", default=DEFAULT_MODEL, help="cursor-agent model (default: %(default)s)")
    parser.add_argument("--timeout", type=int, default=DEFAULT_TIMEOUT, help="watchdog seconds (default: %(default)s)")
    parser.add_argument(
        "--transcript", action="store_true", help="print the compact transcript and stop (no model call)"
    )
    parser.add_argument("--validate", metavar="META_JSON", help="validate a sidecar file against the schema and stop")
    args = parser.parse_args(argv)
    machine = not sys.stdout.isatty()

    if args.version:
        if machine:
            emit("Version", {"version": VERSION})
        else:
            print("seecast %s" % VERSION)
        return

    if args.validate:
        try:
            with open(args.validate, "r", encoding="utf-8") as f:
                validate_meta(json.load(f))
        except (OSError, ValueError) as e:
            fail("invalid metadata in `%s`: %s" % (args.validate, e))
        if machine:
            emit("Valid", {"path": args.validate})
        else:
            print("`%s` conforms to the cast metadata schema" % args.validate)
        return

    if not args.cast:
        parser.error("the .cast recording argument is required")
    try:
        if args.transcript:
            # Explicit stream mode: the transcript itself is the data, no envelope.
            with open(args.cast, "r", encoding="utf-8", errors="replace") as f:
                print(transcript(f.read()))
            return
        meta = annotate(args.cast, model=args.model, timeout=args.timeout)
    except (OSError, ValueError, RuntimeError) as e:
        fail(str(e))
    sidecar_json = json.dumps(meta, indent=2, ensure_ascii=False) + "\n"
    if args.output == "-":
        # Explicit stream mode: the sidecar itself is the data, no envelope; the exit
        # code is the machine's success signal.
        sys.stdout.write(sidecar_json)
        return
    out_path = args.output or os.path.splitext(args.cast)[0] + ".meta.json"
    with open(out_path, "w", encoding="utf-8") as f:
        f.write(sidecar_json)
    if machine:
        # The write narration rides inside the document (machine-mode stderr stays quiet).
        emit("Annotated", {"output": out_path, "chapters": len(meta.get("chapters", [])), "meta": meta})
    else:
        print("wrote %s (%d chapters)" % (out_path, len(meta.get("chapters", []))), file=sys.stderr)
        sys.stdout.write(sidecar_json)


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
