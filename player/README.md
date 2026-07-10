# `beecast-player` — the first-party beecast player

A self-contained, dependency-free player for asciicast recordings (v1, v2, and v3): a
DOM-free asciicast parser and VT100/xterm-subset terminal emulator, a headless playback
controller, and a thin DOM half with the default controls and `<beecast-player>` Web
Component. The crate exposes the component as two string constants to inline —
`PLAYER_JS` (one `<script>`) and `PLAYER_CSS` (one `<style>`); nothing is fetched at
runtime, no workers, no fonts, no images.

This crate is the component's canonical home. The component was born in scsh's session
browser and graduated here; scsh is now one downstream consumer among any others.
[beecast](https://github.com/dkorolev/beecast) pages embed it through `beecast-page`; any
other page or app that plays asciicast recordings consumes it from crates.io the same way.

**Clean-room statement.** Written from scratch against public format and protocol
documentation only — the asciicast v1/v2/v3 format descriptions and the standard ECMA-48 /
xterm control-sequence references. No asciinema-player source code was consulted, copied,
or translated. **MIT**, like the rest of beecast, so every embedding page carries a single
license.

## Layout

| File | Role |
| --- | --- |
| `src/vt.js` | **The portable core.** Asciicast parsing (v1/v2/v3) with live-follow appends, the VT100/xterm-subset terminal emulator, and the pacing map. Pure state machines: bytes in, screen snapshot out. No DOM, no timers, no globals — runs in a browser or Node unchanged. |
| `src/controller.js` | **Headless playback controller** (`BeeCastController`). Owns cast state, terminal, pacing, clock, markers, and subscribers. Injectable scheduling for deterministic tests. No DOM. |
| `src/player.js` | DOM view over the controller, default controls, `<beecast-player>` custom element, and the legacy `BeeCastPlayer.create` factory. |
| `src/player.css` | Terminal palette + player chrome. Semantic `--beecast-*` tokens are the stable theming surface; `--sp-*` is the terminal palette. |

The three JS files are concatenated at compile time into the one `PLAYER_JS` constant.

```rust
// Inline both constants whole; the page stays fully self-contained.
let js = beecast_player::PLAYER_JS;
let css = beecast_player::PLAYER_CSS;
```

## Integration levels

1. **Zero-config factory** (legacy, still fully supported):

```js
const player = BeeCastPlayer.create({ data: castText }, mountElement, {
  fit: 'both',
  controls: true,
  idleTimeLimit: 2,
  markers: [[t, 'label'], /* or */ { time: t, label: 'label', type: 'chapter' }],
  startAt: 12.5,
  speed: 1.5,
  autoPlay: true,
  fullscreenEl: el,
});
player.play();
player.pause();
player.seek(t);
player.setSpeed(2);       // in place — does not remount
player.getCurrentTime();  // recording seconds, synchronous
player.getState();        // snapshot: status, time, markers, terminal, …
player.subscribe(fn);     // immediate + changes; returns unsubscribe
player.append(text);      // live-follow
player.dispose();
```

2. **Web Component** (preferred for new browser embeddings):

```html
<beecast-player fit="both" idle-time-limit="2"></beecast-player>
<script>
  const el = document.querySelector('beecast-player');
  el.load({ cast: castText, markers: [[0, 'Start']] });
  el.play();
  el.addEventListener('beecast-timeupdate', (e) => { /* e.detail.currentTime */ });
</script>
```

3. **Headless controller** (custom UI, tests, non-DOM hosts):

```js
const controller = BeeCastController.create({
  data: castText,
  idleTimeLimit: 2,
  speed: 1,
  markers: [],
  clock: optionalClock, // { now, requestAnimationFrame, cancelAnimationFrame }
});
const stop = controller.subscribe((state, meta) => { /* render your UI */ });
controller.play();
controller.setSpeed(1.5);
controller.seek(42.5);
controller.append(chunk);
controller.dispose();
```

## Compatibility notes (public vs internal)

**Public today**

- `BeeCastVT`: `parseCast`, `appendCast`, `buildPacing`, `extendPacing`, `mapTime`, `Term`, attribute bit constants, `color256`.
- `BeeCastController.create(...)` and its command/state/subscribe contract.
- `BeeCastPlayer.create(...)` methods: `play`, `pause`, `toggle`, `seek`, `getCurrentTime`, `setSpeed`, `getState`, `subscribe`, `append`, `dispose`.
- `<beecast-player>` element: properties/methods/events listed below.
- CSS variables listed in `BeeCastPlayer.supportedCssVariables` (semantic `--beecast-*` plus terminal `--sp-*`).

**Readable but not public** (migration window only — do not depend on these):

- `player.playing`, `player.pacedPos`, `player.eventIdx`, and other internals listed in `BeeCastPlayer.nonPublicFields`. Prefer `getState().status === 'playing'` or `subscribe`.

`getCurrentTime()` and `seek()` are **synchronous**. If a future source needs async seeking, that will be a separately named method — these contracts will not change silently.

## Playback state

```ts
interface PlaybackState {
  status: 'idle' | 'playing' | 'paused' | 'ended';
  currentTime: number;      // recording seconds
  duration: number;
  speed: number;
  atLiveEdge: boolean;
  canAppend: boolean;
  markers: TimelineMarker[];
  terminal: TerminalSnapshot;
  dimensions: { columns: number; rows: number };
}
```

`getState()` returns a defensive snapshot. `subscribe(listener)` invokes the listener immediately with the current state, then on meaningful changes. High-frequency `timeupdate` notifications are coalesced to animation-frame rate; discrete events (`seek`, `speedchange`, `durationchange`, …) always deliver. The unsubscribe function is idempotent.

## Events (`CustomEvent` on the player root / `<beecast-player>`)

| Event | Typical `detail` |
| --- | --- |
| `beecast-ready` | `{ state }` |
| `beecast-play` | `{ origin, currentTime }` |
| `beecast-pause` | `{ origin, currentTime }` |
| `beecast-timeupdate` | `{ currentTime, duration, atLiveEdge }` |
| `beecast-seek` | `{ origin, currentTime, duration }` |
| `beecast-durationchange` | `{ duration }` |
| `beecast-speedchange` | `{ speed, origin }` |
| `beecast-markerchange` | `{ markers }` |
| `beecast-markerselect` | `{ marker }` (cancelable) |
| `beecast-ended` | `{ currentTime, duration }` |
| `beecast-liveedgechange` | `{ atLiveEdge }` |

Times are always **recording time**. Origins include `'api'`, `'keyboard'`, `'pointer'`, `'marker'`, `'source'`.

## Markers

Tuples `[time, label]` still work and are normalized. Preferred form:

```ts
interface TimelineMarker {
  id: string;
  time: number;
  type: 'chapter' | 'annotation' | 'event' | string;
  label: string;
  description?: string;
  color?: string;
  source?: 'cast' | 'sidecar' | 'integration';
  data?: unknown;
}
```

In-band `m` events get `source: 'cast'` and stable ids. Sidecar chapters use `source: 'sidecar'`.

## Sources

```ts
type CastSource =
  | { type: 'text'; data: string }
  | { type: 'custom'; subscribe: (sink) => unsubscribe };
```

The base player never fetches because a string looks like a URL. Network-backed adapters are caller-supplied; generated pages use only inline text and perform zero network requests.

## Theming

Stable semantic tokens:

```css
--beecast-color-surface
--beecast-color-surface-raised
--beecast-color-text
--beecast-color-text-muted
--beecast-color-accent
--beecast-color-focus
--beecast-color-marker
--beecast-color-error
--beecast-control-height
--beecast-radius
--beecast-font-ui
--beecast-font-terminal
```

Built-in themes via `data-theme="dark" | "light" | "system"` on the player root (or the `theme` attribute on `<beecast-player>`). Terminal ANSI colors (`--sp-c0`…`--sp-c15`) remain independently themeable. Internal `.sp-*` classes are not a public API.

## Controls configuration

```js
controls: true | false | {
  play: true,
  seek: true,
  time: true,
  speed: true,
  chapters: true,
  fullscreen: true,
}
```

## Accessibility

- Icon-only controls expose accessible names (`aria-label`) independent of `title`.
- Play/pause uses `aria-pressed`; speed options use `aria-checked`.
- Seek supports Arrow keys, Home, End, Page Up/Down.
- `:focus-visible` styles use `--beecast-color-focus` / accent.
- `accessibility: 'snapshot' | 'off'` — snapshot mode exposes the current terminal as off-screen preformatted text (not a live region, so playback does not flood assistive tech).
- `prefers-reduced-motion` disables overlay motion.

Keyboard when the player has focus: **space** play/pause · **←/→** seek ±5s ·
**< / >** speed down/up · **[ / ]** previous/next marker · **c** chapters ·
**f** fullscreen · **Escape** closes menus.

## Time axis, layout, live-follow

**The time axis is always recording time.** Idle-time compression only affects pacing
(long silences play back at most `idleTimeLimit` seconds long); `seek`, `getCurrentTime`,
markers, and `?t=` deep links all use the recording's own clock.

**Layout.** With `fit` set, the fixed-metric terminal scales *down* (never up) to the
containing box's width — and, for `fit: 'both'`, also to the mount's height when the
embedding page gives it one. Whenever the terminal (scaled or not) ends up narrower than
its pane, it is centered horizontally.

**The big play button.** While the recording sits at its very start, a large center play
glyph — block characters shaped as a triangle — dims the screen behind it; one click
starts playback. It never appears mid-recording.

**Live-follow.** Feed each new chunk of v2/v3 NDJSON to `append(text)`. Chunk boundaries
are free; partial trailing lines buffer until complete. A playhead at the live edge stays
pinned (`tail -f` policy); a viewer who seeked back is never yanked forward. v1 `append`
is a no-op.

**Declared-live mode.** `player.setLive(true)` (also on the controller) is for the
embedder that KNOWS the recording is still being produced: the playhead parks at the
growing edge — every append renders immediately, pinned unconditionally — and the seek
bar renders full-width in the live color (`--beecast-color-live`), reading as "now"
rather than a position that jitters as the duration grows. Any explicit rewind — a seek
before the edge, or `play()` (which would replay from the top) — drops live mode with a
`livechange` event (`beecast-livechange` on the element); `getState().live` reports it.

The core half is exposed on `BeeCastVT` for embedders that need it without a mounted player.

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

The DOM-free core and headless controller self-test under Node from `cargo test`
(`vt_core_node_selftest` shells out to `node` and skips silently when Node is not
installed): parsing, the emulator subset, live-follow appends, the pacing map, controller
state transitions, subscribe/unsubscribe, setSpeed, seek, and marker normalization.
Structural tests gate the bundle properties every self-contained embedding depends on —
no `</script`, no workers, no CSS fetches, no third-party license marker, and a stable
public API surface.

## License

MIT (text in [`LICENSE`](LICENSE), shipped with the crate) — the component, like the rest
of beecast, is all first-party code.
