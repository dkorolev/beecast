# `beecast-player` — the first-party BeeCast player

A self-contained, dependency-free player for asciicast recordings (v1, v2, and v3): a
DOM-free asciicast parser and VT100/xterm-subset terminal emulator, plus a thin DOM half
with the renderer, playback clock, and controls. The crate exposes the component as two
string constants to inline — `PLAYER_JS` (one `<script>`) and `PLAYER_CSS` (one
`<style>`); nothing is fetched at runtime, no workers, no fonts, no images.

This crate is the component's canonical home. The component was born in scsh's session
browser and graduated here; scsh is now one downstream consumer among any others.
[BeeCast](https://github.com/dkorolev/beecast) pages embed it through `beecast-page`; any
other page or app that plays asciicast recordings consumes it from crates.io the same way.

**Clean-room statement.** Written from scratch against public format and protocol
documentation only — the asciicast v1/v2/v3 format descriptions and the standard ECMA-48 /
xterm control-sequence references. No asciinema-player source code was consulted, copied,
or translated. **MIT**, like the rest of BeeCast, so every embedding page carries a single
license.

## Layout

| File | Role |
| --- | --- |
| `src/vt.js` | **The portable core.** Asciicast parsing (v1/v2/v3) with live-follow appends, the VT100/xterm-subset terminal emulator, and the pacing map. Pure state machines: bytes in, screen snapshot out. No DOM, no timers, no globals — runs in a browser or Node unchanged. |
| `src/player.js` | The thin DOM half: renderer (snapshot → HTML runs), playback clock (idle-time compression, speed), controls (play/pause, seek bar with chapter markers, keyboard shortcuts), the live-follow policy, and the public API. |
| `src/player.css` | Terminal palette + player chrome. All colors are CSS variables, themeable by the embedding page. |

The two JS files are concatenated at compile time into the one `PLAYER_JS` constant.

```rust
// Inline both constants whole; the page stays fully self-contained.
let js = beecast_player::PLAYER_JS;
let css = beecast_player::PLAYER_CSS;
```

## Public API

```js
const player = BeeCastPlayer.create({ data: castText }, mountElement, {
  fit: 'both',        // scale the terminal to the container (width, or width + height)
  controls: true,     // render the control bar (default true)
  idleTimeLimit: 2,   // cap silent gaps at N seconds of playback time
  markers: [[t, 'label'], …],  // chapter ticks on the seek bar
  startAt: 12.5,      // seconds, or a 'mm:ss' string
  speed: 1.5,         // initial speed (one of 0.5, 1, 1.5, 2, 3, 5)
  autoPlay: true,     // start playing immediately
});
player.play();
player.pause();
player.seek(t);            // seconds, or 'mm:ss' — always in RECORDING time
player.getCurrentTime();   // seconds, in RECORDING time
player.append(text);       // live-follow: newly produced cast lines (see below)
player.dispose();
```

**The time axis is always recording time.** Idle-time compression only affects pacing
(long silences play back at most `idleTimeLimit` seconds long); `seek`, `getCurrentTime`,
markers, and `?t=` deep links all use the recording's own clock, so chapter sidecars and
share-links stay aligned no matter the compression.

**Layout.** With `fit` set, the fixed-metric terminal scales *down* (never up) to the
containing box's width — and, for `fit: 'both'`, also to the mount's height when the
embedding page gives it one. Whenever the terminal (scaled or not) ends up narrower than
its pane, it is centered horizontally in it.

Keyboard, when the player has focus: **space** play/pause · **←/→** seek ±5s ·
**< / >** speed down/up · **[ / ]** previous/next marker.

## Live-follow

A recording that is still being produced can be followed as it grows: feed each new chunk
of v2/v3 NDJSON lines to `player.append(text)`. How the data arrives — WebSocket, polling,
a tailed file — is the caller's business; the player owns everything after that:

- **Chunk boundaries are free.** Only complete (newline-terminated) lines are consumed; a
  partial trailing line — including one already present when the player loaded a file cut
  mid-write — is buffered until its remainder arrives. Stray header replays and `#`
  comment lines are skipped.
- **The follow policy is positional, like `tail -f`.** A playhead resting at the live edge
  stays pinned to it and renders each append immediately. A viewer who paused earlier or
  seeked back is never yanked forward — they just watch the duration grow. A *playing*
  player keeps its own clock; the longer recording simply no longer auto-pauses it at the
  old end, and once playback catches the edge and parks there, subsequent appends pick it
  up and follow.
- A player mounted on an empty live cast (header only, `duration` 0) follows from the
  first byte with no extra configuration.

v1 recordings are a single JSON document with no line to append to; `append` on them is a
no-op by design.

The core half is exposed on `BeeCastVT` for embedders that need it without a mounted player:
`parseCast(text)`, `appendCast(cast, text)`, `buildPacing` / `extendPacing` / `mapTime`,
and `new Term(cols, rows)` with `.write(text)`, `.resize(c, r)`, `.snapshot()`.

## Terminal emulation scope

The subset a tmux-hosted TUI actually exercises: cursor addressing (CUP/CUU/CUD/CUF/CUB/
CHA/VPA/CNL/CPL), erase (ED/EL/ECH), insert/delete (ICH/DCH/IL/DL), scroll (SU/SD, DECSTBM
scroll regions, IND/RI/NEL), SGR (16/256/true color, bold, dim, italic, underline, inverse,
strikethrough), alternate screen (`?1049`, `?47`), cursor visibility (`?25`), autowrap with
deferred wrap (`?7`), save/restore cursor (DECSC/DECRC, CSI s/u), DEC special graphics
(`ESC ( 0` line drawing, SO/SI), tab stops, OSC consumption (titles are parsed and ignored),
and v3 in-band resize events. Unrecognized sequences are consumed and ignored — never
rendered as text.

## Testing

The DOM-free core self-tests under Node from `cargo test` (`vt_core_node_selftest` shells
out to `node` and skips silently when Node is not installed): parsing, the emulator
subset, live-follow appends across hostile chunk boundaries, and the pacing map. A
structural test (`player_bundle_is_inline_safe_and_first_party`) gates the properties
every self-contained embedding depends on — no `</script`, no workers, no CSS fetches, and
no third-party license marker anywhere in the bundle.

## License

MIT (text in [`LICENSE`](LICENSE), shipped with the crate) — the component, like the rest
of BeeCast, is all first-party code.
