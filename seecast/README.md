# SeeCast

**SeeCast** watches an [asciinema](https://asciinema.org) `.cast` recording so you don't have to: it generates the `{ title, summary, chapters }` metadata sidecar that [BeeCast](../README.md) renders — a one-line title, a short summary, and timekeyed chapters, YouTube-style. Annotation runs on `cursor-agent` with the **Composer Fast** model (`composer-2.5-fast`): fast and cheap, and chapter titling is squarely within its reach.

```
./seecast demo.cast        # → demo.meta.json, next to the recording
```

## What it produces

A sidecar in the shape specified by [`SCHEMA.md`](../SCHEMA.md) (formally: [`schema/beecast-meta.schema.json`](../schema/beecast-meta.schema.json)):

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
```

- Reads asciicast **v2 and v3** (v3 event times are relative and get accumulated).
- Renders the recording into a compact, timestamped, ANSI-stripped transcript, deduplicated and downsampled — TUI redraws don't flood the prompt.
- Calls `cursor-agent -p` headless; a hard watchdog kills a hung call (default 180 s), and liveness ticks go to stderr every ~10 s so callers can tell it's alive.
- Writes the validated sidecar next to the recording (`demo.cast` → `demo.meta.json`) and echoes the JSON to stdout. Diagnostics go to stderr; errors are `{ "Error": { … } }` JSON when stdout is not a TTY.

## The `scsh` skill

The same annotator ships as a scoped skill for [scsh](https://github.com/dkorolev/scsh), where the harness itself is cursor/Composer — the model reads the transcript and writes the sidecar directly, no nested agent call:

```
scsh installskills <this-repo>       # into your repo's .scsh.yml
CAST=recordings/demo.cast scsh run --profile seecast
```

The skill requires `CAST` (the repo-relative path of the recording) and writes the sidecar JSON to its declared result path, `tmp/seecast.json`.

## Testing

`python3 -m unittest discover -s tests` — transcript rendering, v2/v3 time handling, ANSI stripping, and reply validation are covered with a stubbed model; the cursor-agent path is exercised for real by running the script on an actual recording.

## License

MIT.
