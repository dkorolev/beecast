# `scsh-cast-player` — the first-party asciicast player

The clean-room, dependency-free player these pages embed: `vt.js` is a DOM-free asciicast
(v1/v2/v3) parser plus a VT100/xterm-subset terminal emulator; `player.js`/`player.css` are
the DOM renderer, playback clock, and controls. Written from scratch against public format
and protocol documentation only (the asciicast format descriptions and ECMA-48 / xterm
references) — no asciinema-player code was consulted, copied, or translated. **MIT**, like
the rest of BeeCast, so generated pages carry a single license.

The canonical copy currently lives in `scsh` (`src/daemon/html/player/`, with its full API
docs and a Node-based test suite); this directory is a verbatim copy of `vt.js`,
`player.js`, and `player.css`. Keep the two in sync until the component graduates into its
own crate — the DOM-free `vt.js` core is the part destined to become a Rust module.
