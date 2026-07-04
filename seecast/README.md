# SeeCast

**SeeCast** watches an [asciinema](https://asciinema.org) `.cast` recording so you don't have to: it generates the `{ title, summary, chapters }` metadata sidecar that [BeeCast](../README.md) — this repo's Rust renderer — turns into a player page: a one-line title, a short summary, and timekeyed chapters, YouTube-style. Annotation runs on `cursor-agent` with the **Composer Fast** model (`composer-2.5-fast`): fast and cheap, and chapter titling is squarely within its reach.

```
./seecast demo.cast        # → demo.meta.json, next to the recording
```

## What it produces

A sidecar in the shape specified by [`SCHEMA.md`](../SCHEMA.md) (formally: [`schema/beecast-meta.schema.json`](../schema/beecast-meta.schema.json) — one copy, kept in sync with the Rust types in [`src/meta.rs`](../src/meta.rs), the source of truth):

```json
{
  "title": "Rebuilding the parser",
  "summary": "Replaces the ad-hoc tokenizer with a table-driven one and gets the suite green.",
  "chapters": [
    { "t": 0,     "title": "Setup" },
    { "t": 42.5,  "title": "First failing test" },
    { "t": 187.0, "title": "Green build" }
  ]
}
```

The timekey `t` is fractional seconds; chapters are strictly ascending and the first one is pinned to `t: 0` (whatever the model guessed for it — the opening segment always gets a marker). The reply is validated before anything is written: a malformed or schema-violating reply produces no sidecar, never a broken one.

## The standalone script

`./seecast` is a single-file, stdlib-only Python 3 script:

```
./seecast <recording.cast> [-o <meta.json>] [--model composer-2.5-fast] [--timeout 180]
./seecast --transcript <recording.cast>     # print the compact transcript and stop
./seecast --validate <meta.json>            # validate a sidecar against the schema and stop
./seecast --version                         # works offline
```

Install it as a command — single-file and stdlib-only means a symlink (or a copy) is the whole story:

```
ln -s "$(pwd)/seecast/seecast" ~/.local/bin/seecast     # from the repo root
```

- Reads asciicast **v2 and v3** (v3 event times are relative and get accumulated).
- Renders the recording into a compact, timestamped, ANSI-stripped transcript, deduplicated and downsampled — TUI redraws don't flood the prompt.
- Calls `cursor-agent -p` headless; a hard watchdog kills a hung call (default 180 s) and retries it once — announced on stderr, since cursor-agent occasionally stalls on startup — and liveness ticks go to stderr every ~10 s so callers can tell it's alive.
- Writes the validated sidecar next to the recording (`demo.cast` → `demo.meta.json`).
- Human at a TTY gets human-readable output; `--json` (or a piped/captured stdout) gets a two-space-indented single-key JSON document with a request-specific variant: `{ "Annotated": { output, chapters, meta } }`, `{ "Valid": … }`, `{ "Version": … }`, and on failure `{ "Error": { message, stage } }` where `stage` is `usage` or `request`. The explicit stream modes (`--transcript`, `-o -`) emit the bare document — it *is* the data.
- Exit codes: `0` success, `1` failure, `2` usage, `130` interrupted; a broken pipe ends the program quietly.
- Color at a TTY by default; `--color=never`, `--color=no`, or `NO_COLOR` turn it off.

## Not this tool's job

| Absent on purpose               | Where that job IS done                                 |
| ------------------------------- | ------------------------------------------------------ |
| Recording a terminal session    | `asciinema rec` (or any asciicast v2/v3 producer)      |
| Rendering the player page       | [BeeCast](../README.md), this repo's Rust renderer     |
| Hand-editing a sidecar          | Any editor — then `./seecast --validate` checks it     |
| Hosting the annotation model    | `cursor-agent` (Cursor CLI) with its own credentials   |

## The `scsh` skill

The same annotator ships as a scoped skill for [scsh](https://github.com/dkorolev/scsh) (needs an scsh with the `cursor` harness, ≥ 1.10), where the harness itself is cursor/Composer — the model reads the transcript and writes the sidecar directly, no nested agent call. This is the canonical path for casts that live in a repo; the standalone script above is for loose files:

```
scsh installskills <this-repo>       # into your repo's .scsh.yml
CAST=recordings/demo.cast scsh run --profile seecast
```

The skill requires `CAST` (the repo-relative path of the recording) and writes the sidecar JSON to its declared result path, `tmp/seecast.json`.

## Testing

`python3 -m unittest discover -s seecast/tests` (from the repo root) — transcript rendering, v2/v3 time handling, ANSI stripping, reply validation, and the CLI contract are covered with a stubbed model; the cursor-agent path is exercised for real by running the script on an actual recording.

## License

MIT, per the repo's root [LICENSE](../LICENSE).
