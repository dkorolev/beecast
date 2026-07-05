# Vendored web assets

- `asciinema-player.min.js`, `asciinema-player.css` — [asciinema-player](https://github.com/asciinema/asciinema-player) v3.17.0, Apache License 2.0, © the asciinema-player authors. Fetched from the npm `asciinema-player@3.17.0` package (`dist/bundle/`). Vendored — and inlined verbatim into every generated page — so the output `.html` is fully self-contained: no CDN, no network, no worker sidecar (the main bundle references none of them, which is asserted by a unit test in `page.rs`).

## License

The player is redistributed **unmodified** under the Apache License 2.0. Its full text is `LICENSE-APACHE-2.0` in this directory (Apache-2.0 §4(a)); the minified bundle also carries its own inline `@license` header, so the notice survives even when the code is inlined into a generated page. The upstream package ships no `NOTICE` file, so §4(d) adds nothing.

BeeCast's own code is MIT (repo-root `LICENSE`). The published crate therefore contains both licenses — its `Cargo.toml` declares `license = "MIT AND Apache-2.0"` accordingly.
