# Crustacean

A small chess engine written in Rust. The name is Rust's crab meeting Stockfish;
this is an independent engine, not a Stockfish port.

**[Play in your browser](https://jasonshaw0.github.io/crustacean/)** — no account or install.

Drag pieces or click to move. Play either color, watch the engine play itself,
pause and step through moves, undo, load FEN positions, or export a PGN.
The board shows legal moves, check, material balance and the last engine search.
Desktop play fits the viewport; on phones, pause sits beside the board.
Wins, losses and draws appear over the board, with an option to review the position.
Browser games live in the current tab and reset on reload, so export before leaving.

## Run locally

Install stable Rust, then:

```sh
cargo run --release -- gui
```

Open the printed address. The native app embeds the UI and keeps your game until
the server closes. Use `gui --port 8788` to choose another port.

For terminal play, use `cargo run --release -- play`. Other commands:

```sh
cargo run --release -- analyze --movetime 1000
cargo run --release -- perft 4
cargo run --release -- uci
```

The engine uses alpha-beta and quiescence search, handles standard chess rules,
and automatically adjudicates threefold repetition and the fifty-move rule.
UCI supports timed/depth/node searches and stop; Chess960 and pondering aren't supported.

## Build the website

With Rust and Node installed:

```sh
rustup target add wasm32-unknown-unknown
node scripts/build-web.mjs
node --test tests/*.mjs
cargo test --release --locked
```

Serve `dist/` with any static HTTP server. GitHub Actions builds, tests and publishes
it on pushes to `main`. The website runs the same Rust engine through WebAssembly;
search happens in a worker so the board stays responsive. No runtime dependencies.
