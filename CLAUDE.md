# fsmgen

One state-machine table generates Rust runtime code, a stateright model, Kani harnesses, and a DOT
diagram. The full plan is in `fsmgen-plan.md`. Decisions are in `docs/decisions.md`.
What each result does and doesn't establish is tracked in `docs/assurance.md`. Update it whenever a
phase lands or the evidence for a link changes.

## Status

Phase 0, the spike. There is no generator yet. `examples/toy-handshake/` holds **hand-written target
output**. Treat it as the spec for what the generator must emit.

## Commands

```sh
cargo test -p toy-handshake --features model                 # unit + differential + model check
cargo test -p toy-handshake --features model,planted-bug     # shows the planted bug is caught
cargo run  -p toy-handshake --example check --features model[,planted-bug]   # readable report
cargo clippy --all-targets --features toy-handshake/model -- -D warnings
cargo fmt --all --check
```

## Conventions

- No `unsafe` in generated code or core crates (`#![forbid(unsafe_code)]`).
- `cargo clippy -- -D warnings` and `cargo fmt --check` must pass.
- Generated code must be readable under `cargo expand`; prefer plain `match` over clever abstractions.
- Every new DSL feature needs: parser test, snapshot test of generated code, `trybuild` error test (if it can fail), and a model-check test.
- External dependencies are pinned and vendored; no new dependency without noting it in `docs/decisions.md`.
- Never add a catch-all `_` arm to generated `step()` when `total: true`.
- Keep `step()` synchronous and I/O-free.
- Runtime and model share var guards and updates through `Vars` methods. Never write a var expression twice (D-005).
- Do not generate stateright `eventually` properties; use the termination pass (D-006).
