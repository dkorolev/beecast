# Vendored web assets

- `asciinema-player.min.js`, `asciinema-player.css` — [asciinema-player](https://github.com/asciinema/asciinema-player) v3.17.0, Apache License 2.0, © the asciinema-player authors. Fetched from the npm `asciinema-player@3.17.0` package (`dist/bundle/`). Vendored — and inlined verbatim into every generated page — so the output `.html` is fully self-contained: no CDN, no network, no worker sidecar (the main bundle references none of them, which is asserted by a unit test in `page.rs`).
