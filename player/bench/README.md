# Player microbenchmarks

Build or otherwise produce a JavaScript bundle containing `vt.js` and
`controller.js`, then run:

```sh
node --expose-gc bench/bench.js src 10k
node --expose-gc bench/bench.js path/to/player.js all
```

The optional size is `10k`, `100k`, `1m`, a numeric event count, or `all` (the
default). Each size runs scrolling, SGR-heavy redraw, alternate-screen churn,
and long sparse recordings. The table reports parsing time and approximate heap
growth, a backwards seek near the end, append throughput across deliberately
hostile chunk boundaries, and `getState()` cost during fake-clock playback.

This is intentionally a local benchmark: shared-runner timing is too noisy for
a useful CI threshold. Run it on the same machine before and after performance
changes and record the machine, Node version, command, and resulting table in
the change description.

For development convenience, the first argument may also be the `src` directory;
the harness loads `vt.js` and `controller.js` in bundle order.
