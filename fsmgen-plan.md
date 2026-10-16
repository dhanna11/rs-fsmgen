# fsmgen — Plan

A single state-machine definition that generates:
1. Rust runtime code (enums, `step()`, a `Hooks` trait for real logic)
2. A model-checkable model (stateright)
3. Kani proof harnesses for the generated code
4. A Graphviz diagram
5. (Optional) a TLA+ export

First real target: IKEv1 Main Mode (MM1–MM6) initiator and responder, Phase 1.

---

## Goals

- **One source of truth.** The transition table is the spec. Code, model, harnesses, and diagrams are all generated from it and cannot drift.
- **Thin generated layer, ordinary hand-written logic.** Generated code handles control flow only. Crypto, parsing, and packet building live in hand-written `Hooks` impls that are normal, testable Rust.
- **Verification of control logic.** Model-check properties such as bounded retries, no deadlock, totality, and agreement between initiator and responder.
- **Auditable output.** Generated code should be readable, `cargo expand`-friendly, and contain no `unsafe`.

## Non-goals

- Generating cryptographic protocol models (Tamarin/ProVerif). Those depend on crypto semantics that are not in a transition table. Handle separately and by hand if needed.
- Hierarchical statecharts (superstates) in v1.
- Async inside the generated core. `step()` is synchronous and pure; async lives in the driver/actor loop.

## Open decision (resolve in Phase 0)

- **Fork smlang vs. own parser.** smlang (MIT/Apache-2.0) already parses a Boost-SML-style table with guards, actions, and state/event data. Options:
  - A. Fork smlang and add backends (model, Kani, DOT) to its existing IR.
  - B. Write our own small parser with `syn` and own the whole stack.
- Decide based on: how cleanly smlang's IR can be reused, how much of its codegen we'd keep, and team preference for owning vs. vendoring.
- **Get lead/team sign-off** before depending on an outside crate or starting a new internal tool. Vendor and pin any external dependency.

---

## DSL (v1 sketch)

```text
statemachine! {
    name: MainModeInitiator,

    // Model-visible variables: small, bounded, with simple updates.
    // These are the ONLY data the model checker sees precisely.
    vars: {
        tries: u8 in 0..=MAX_TRIES = 0,
    },

    transitions: {
        *Idle    + Start                      / send_mm1  { tries = 0 }  = Mm1Sent,

        Mm1Sent  + Mm2     [mm2_valid]        / build_mm3 { tries = 0 }  = Mm3Sent,
        Mm1Sent  + Mm2     [!mm2_valid]       / drop                     = _,
        Mm1Sent  + Timeout [tries < MAX_TRIES] / resend   { tries += 1 } = _,
        Mm1Sent  + Timeout [tries >= MAX_TRIES] / abort                  = Failed,

        // ... Mm3Sent, Mm5Sent rows follow the same pattern ...

        Mm5Sent  + Mm6     [mm6_valid]        / complete                 = Established,
    },

    // Optional: require every (state, event) pair to be covered.
    total: true,
}
```

Row anatomy: `Source + Event [guard] / action { var updates } = Target`.

- `= _` means stay in the current state (internal transition).
- **Guards** come in two kinds:
  - *Opaque* (e.g. `mm2_valid`): implemented in Rust via `Hooks`; the model treats them as nondeterministic (explores both true and false).
  - *Var guards* (e.g. `tries < MAX_TRIES`): expressions over `vars`; evaluated precisely in both code and model.
- **Actions** are opaque (implemented via `Hooks`), and may be followed by var updates in `{ }`, which are applied identically in code and model.
- Guard expressions support `!`, `&&`, `||`.

## Generated Rust (runtime)

For each machine:

- `enum State { Idle, Mm1Sent(Mm1Sent), ... }`. State data structs are user-defined.
- `enum Event { Start, Mm2(Mm2), Timeout, ... }`
- `trait Hooks` with one method per unique guard/action:
  - Guards: `fn g(&mut self, s: &Src, e: &Ev) -> bool` (borrow, because the next arm may still need them).
  - Actions: `fn a(&mut self, s: Src, e: Ev) -> (Target, Vec<Out>)` (consume, because the transition is happening).
  - Signatures are derived from each row's source, event, and target types.
- `struct Machine { state: State, vars: Vars }`
- `fn step<H: Hooks>(&mut self, h: &mut H, ev: Event) -> Vec<Out>`
  - One `match` arm per row, in table order.
  - Uses `mem::replace` with a `Poisoned` placeholder so a panic mid-transition never leaves half-updated state.
  - A `!guard` row that directly follows its `guard` row is emitted as a fallthrough (no double evaluation).
- With `total: true`, generation fails if any (state, event) pair is uncovered. No catch-all `_` arm.

## Generated model (stateright)

- `impl stateright::Model` for each machine, behind a `model` feature.
- Model state = (control state name, `vars`). State data structs are **not** modeled.
- Opaque guards → nondeterministic choice. Opaque actions → no-op on data, but var updates apply.
- **Composition:** a combined model of initiator + responder over an abstract network with bounded in-flight messages and configurable **loss, duplication, and reordering**. Timeouts are modeled as events that can fire at any time while waiting.
- **Built-in properties** (auto-generated):
  - No deadlock: every reachable non-terminal state has an enabled transition.
  - Reachability: every declared state is reachable (warn otherwise).
  - Bounded vars: `vars` stay within declared ranges.
  - Termination: every run eventually reaches a terminal state (`Established` or `Failed`) under fair scheduling.
- **User properties**: written in Rust next to the model, e.g.
  - Never `Established` unless the last transition was via `mm6_valid`.
  - Retries bounded: never more than `MAX_TRIES` resends per step.
  - Agreement: if the initiator reaches `Established`, the responder is not in `Failed` for the same exchange.

## Generated Kani harnesses

Behind `cfg(kani)`:
- A `Hooks` impl whose guards return `kani::any()` and whose actions return arbitrary target data.
- Harness: from every state, for any event, `step()` does not panic.
- Harness: `vars` invariants hold after any single `step()`.
- Hand-written harnesses for critical hooks (e.g. "failed MM6 validation never mutates crypto/IV state").

## Generated diagram

- Graphviz DOT per machine: states as nodes, rows as edges labeled `Event [guard] / action`.
- Emitted at build time (feature-gated) or via a small CLI. Diagrams can be checked into docs and regenerated in CI.

## (Optional) TLA+ export

- Emit a TLA+ module mirroring the stateright model semantics. Only if someone wants to inspect or check the model in TLC.

---

## Workspace layout

```text
fsmgen/
  crates/
    fsmgen-core/      # IR, parser, validation (totality, reachability, determinism lint)
    fsmgen-macros/    # proc macro: statemachine! -> Rust runtime code
    fsmgen-model/     # stateright backend + network composition + built-in properties
    fsmgen-kani/      # harness generation helpers
    fsmgen-dot/       # Graphviz export
    fsmgen-mutate/    # mutation testing tool
    fsmgen-tla/       # optional
  examples/
    toy-handshake/    # 3-state request/response, used to develop every backend
    ike-mainmode/     # initiator + responder, abstracted (no real crypto)
  redteam/
    corpus/           # buggy tables + expected property violation per file
  docs/
  CLAUDE.md
```

---

## Phases

### Phase 0: Decide and spike (small)
- [x] Read smlang's parser and IR; write a 1-page note: reuse vs. own parser. (`docs/smlang-review.md`)
- [x] Spike: hand-write the Rust that the generator *would* produce for the toy handshake, plus a hand-written stateright model for it. Confirm the shapes compile and the model finds a planted bug. (`examples/toy-handshake/`)
- [ ] Get lead/team sign-off on approach and dependencies. (D-001…D-006 in `docs/decisions.md` are *proposed*)

**Done when:** decision recorded in `docs/decisions.md`, and the hand-written spike shows the target output.

### Phase 1: Core IR + Rust codegen + DOT
- [ ] `fsmgen-core`: IR types, parser, validation errors with good spans.
- [ ] Validation: totality (when `total: true`), unreachable states, duplicate unguarded rows, var range checks on literal updates.
- [ ] `fsmgen-macros`: generate `State`, `Event`, `Vars`, `Hooks`, `Machine::step`.
- [ ] `fsmgen-dot`: DOT export.
- [ ] Toy handshake example builds and has unit tests driven by a fake `Hooks`.
- [ ] Snapshot tests (e.g. `insta`) for generated code; `trybuild` tests for compile errors.

**Done when:** the toy handshake compiles from the DSL, unit tests pass, the DOT renders, and invalid tables produce clear compile errors.

### Phase 2: stateright backend (single machine)
- [ ] Generate `impl Model` with nondeterministic opaque guards and precise var guards/updates.
- [ ] Built-in properties: no deadlock, reachability, bounded vars, termination.
- [ ] Support for user-defined properties.
- [ ] Toy handshake model-checks clean.

**Done when:** the model checker passes on the correct toy machine and fails, with a readable counterexample, on at least 3 hand-planted bugs.

### Phase 3: Composition (initiator + responder)
- [ ] Abstract network: bounded in-flight messages; loss, duplication, reordering toggles.
- [ ] Timeouts as always-enabled events while waiting.
- [ ] Agreement properties across the two machines.
- [ ] `ike-mainmode` example: initiator + responder tables for MM1–MM6 with drop/resend/abort behavior.

**Done when:** Main Mode initiator + responder check clean under loss/dup/reorder, and planted bugs (e.g. missing resend, off-by-one `MAX_TRIES`, responder not resending cached reply on duplicate) are caught.

### Phase 4: Kani harnesses
- [ ] Generated "step never panics" and "vars invariant" harnesses.
- [ ] Example hand-written harness for one critical hook.
- [ ] CI job running Kani on examples.

**Done when:** Kani passes on the examples and catches a planted panic in a generated or hook path.

### Phase 5: Red team + mutation testing
- [ ] `redteam/corpus/`: buggy tables, each with the property expected to fail and a short description. Seed with bugs from Claude, ChatGPT, and Grok (prompt them with the DSL spec + property list; ask for *subtle* violations).
- [ ] `fsmgen-mutate`: automatically mutate a known-good table (delete row, swap target, negate guard, reorder rows, change var bound) and run the model checker on each mutant.
- [ ] Report: mutants killed vs. survived. Every surviving mutant is either equivalent (document why) or reveals a missing property (add it).
- [ ] Wire the corpus + mutation run into CI as regression tests.

**Done when:** every corpus bug is caught, and surviving mutants are triaged and explained.

### Phase 6 (optional): TLA+ export
- [ ] Emit TLA+ matching the stateright semantics; check the toy and Main Mode models in TLC.

---

## Key risk: model/code mismatch

The biggest danger is the generated model meaning something different from the generated code, which gives false confidence. Mitigations:
- Both backends are generated from the **same IR**; no backend re-parses the DSL.
- Var guards and updates use one shared expression evaluator / codegen path.
- **Differential test:** run random event sequences through the generated Rust (with a `Hooks` impl that records which opaque guard values it returned) and through the model with the same guard choices; assert identical control states and vars at every step.
- The red-team corpus and mutation testing (Phase 5) catch blind spots in the properties themselves.

---

## Conventions (for CLAUDE.md)

- No `unsafe` in generated code or core crates (`#![forbid(unsafe_code)]`).
- `cargo clippy -- -D warnings` and `cargo fmt --check` must pass.
- Generated code must be readable under `cargo expand`; prefer plain `match` over clever abstractions.
- Every new DSL feature needs: parser test, snapshot test of generated code, `trybuild` error test (if it can fail), and a model-check test.
- External dependencies are pinned and vendored; no new dependency without noting it in `docs/decisions.md`.
- Never add a catch-all `_` arm to generated `step()` when `total: true`.
- Keep `step()` synchronous and I/O-free.

## Starting prompt for Claude Code

> Read `fsmgen-plan.md`. We're starting Phase 0. First, summarize smlang's parser and IR structure (I'll provide or point you to the source), then hand-write the target generated code and a stateright model for the toy 3-state handshake in `examples/toy-handshake/`. Plant one bug and show the model checker catching it. Don't build the generator yet.
