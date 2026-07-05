---
name: seecast
description: "Generate the { title, summary, chapters } cast-metadata sidecar for an asciinema .cast recording. The recording path arrives in $CAST (repo-relative); the JSON result goes to $SCSH_RESULT. Use when asked to annotate, chapter, or summarize a .cast recording."
---

# seecast

Produce the cast metadata JSON for the recording at `$CAST`. You are the annotator: read the transcript, then write the metadata yourself — there is no nested model call.

## Steps

1. Render the recording into a compact timestamped transcript (do not read the raw cast; it is full of escape sequences):

   ```
   python3 .skills/seecast/scripts/seecast.py --transcript "$CAST"
   ```

2. Study the transcript and compose exactly this object — these three keys, no others:

   ```json
   {
     "title": "<3-8 word human title for the recording>",
     "summary": "<one or two sentences: what the session did>",
     "chapters": [ { "t": <seconds>, "title": "<3-6 word phase name>" } ]
   }
   ```

   - Use between 3 and 8 chapters, in ascending time order.
   - `t` is seconds into the recording; fractional values like `12.5` are welcome.
   - The FIRST chapter MUST have `t: 0` — the opening segment always gets a marker.
   - Each chapter marks a distinct phase; keep titles terse.

3. Write the object to `$SCSH_RESULT` as a real file on disk (two-space indented JSON) — do not only print it.

4. Verify, and fix + re-verify if it fails (`--generated` requires all three keys and at least one chapter, so an empty or partial result cannot pass):

   ```
   python3 .skills/seecast/scripts/seecast.py --validate "$SCSH_RESULT" --generated
   ```

5. **Stop.** Do not commit, push, build, or edit anything except the result file.

## Pass criteria

The result file exists and step 4 exits 0. Anything else is a failure.
