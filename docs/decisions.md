# Decisions

Status values: **proposed** (needs sign-off), **accepted**, **superseded**.

---

## D-001: Own parser and IR instead of forking smlang

**Status:** proposed, needs lead sign-off.

Write `fsmgen-core` (IR, parser, validation) as a plain library on `syn` 2, and copy smlang's
guard-expression grammar with attribution. Reasoning is in `docs/smlang-review.md`. In short: a fork
would keep under a quarter of smlang. Its IR sits inside the proc-macro crate, has
nondeterministic `HashMap` ordering, and has no vars, row identity, or totality.

## D-002: stateright as the model checker

**Status:** proposed, needs lead sign-off.

`stateright = "=0.31.0"`, pinned exactly in the workspace and behind the `model` feature.
Not vendored yet. We'll vendor it once the dependency is approved.

Pros: pure Rust, so a generated model calls the same `Vars` functions as the runtime (see D-005).
It runs BFS with counterexample paths and is fast enough for these models: the toy has 20 states.

Spike findings to know before committing:
- **Heavy dependency tree.** stateright has no cargo features to turn off its web explorer, so
  `tiny_http`, `serde_json`, and more come in. Behind our `model` feature they never reach the
  runtime build.
- **`Property::eventually` is unsound on cycles.** Its docs say so. Any machine with a
  retry or discard self-loop has cycles, so we do not generate `eventually` properties. See D-006.
- **`always` counterexamples are not minimal.** The BFS overwrites a property's discovery each time
  it meets another violating state, so the stored path can run past the first violation. Our
  reporter cuts the path at the first violating state (`examples/check.rs`). `fsmgen-model` must
  do the same.
- `Property` takes `fn` pointers, not closures, and closure parameters need explicit types
  (`|_, s: &ModelState|`). The generator must always write the types.

## D-003: Runtime shape changes found by the spike

**Status:** proposed. These refine the plan's "Generated Rust" section.

1. **`step()` returns `Result<Vec<Out>, Poisoned>`**, not `Vec<Out>`. After a hook panics the
   state is `Poisoned`, and every later call must say so without panicking, so the Kani "step never
   panics" harness holds. `Poisoned` is the only error.
2. **Var updates are staged.** `step()` copies `vars`, applies updates to the copy, and commits the
   copy only after the action returns. A panicking hook leaves `vars` untouched.
3. **No row matched means stay put.** When every guard in a (state, event) group is false, the
   machine stays in its state with no output and records `Last::NoRow(kind)`. The generator can't
   prove var guards exhaustive, so it always emits this `else` branch. The model flags it if it is
   ever reached (built-in property "every event matches a row").
4. **Shared, data-free types.** `Control`, `EventKind`, `Row` (one variant per table row), `Last`, and
   `Vars` are generated once and used by both the runtime and the model. `Machine::last()` is also
   useful for tracing.
5. **Unit types are left out of hook signatures.** `send_req(&mut self)`, not
   `send_req(&mut self, s: (), e: ())`.
6. **Rows with no action** (`Idle + Resp = _`) are allowed and generate no hook. Without them,
   an `ignore` action reused across rows would have conflicting signatures.

## D-004: DSL additions found by the spike

**Status:** proposed.

- `states: { *Idle, Waiting(Waiting), ... }` and `events: { Start, Resp(Resp), ... }` declare states
  and events up front, instead of collecting them from rows the way smlang does. This makes
  totality and reachability well defined and gives each name one span for errors.
- `out: InitiatorOut` names the output type.
- `terminal: [Done, Failed]`. Terminal states ignore every event at runtime and have no actions in
  the model. That's what built-in "no deadlock" and termination need. It replaces a lot of
  `Done + X = _` rows under `total: true`. The generated arm lists events explicitly
  (`Event::Start | Event::Resp(_) | ...`) rather than using `_`, so a new event is still a compile
  error.

Open question: should hooks get read-only `&Vars`? It would be useful for logging `tries` in `resend`.
I left it out of the spike.

## D-005: One implementation of var expressions

**Status:** proposed. This implements the plan's "Key risk" mitigation.

Each var guard and update becomes a method on `Vars` (`r6_guard`, `r6_update`, ...). `Machine::step`
and the model's `step` both call them. There is no second copy of any expression to drift. The
spike's planted bug shows this: one line in `Vars::r6_guard` changed both runtime and model.

Opaque guards are fields of the model's `Action` (`Resp { resp_valid: bool }`). A guard that the
current (state, event) group never calls is fixed at `false`. The differential test
(`tests/differential.rs`) records what the runtime's guards returned, turns that into the
model `Action`, and checks that control state, vars, and `Last` agree after every step. It also
checks that every (state, action) pair the model offers was exercised.

## D-006: Termination is `AG EF terminal`, checked by our own pass

**Status:** proposed.

Because stateright's `eventually` misses counterexamples that end in a cycle (D-002), `fsmgen-model`
will provide a small explicit-state pass (`examples/toy-handshake/src/liveness.rs`, about 100 lines).
The pass explores the model's graph and checks that a terminal state is reachable from every
reachable state. On a finite graph this means termination under strong fairness. When it fails it
returns the shortest path to a stuck state.

Trade-off: the model state includes `Last`, so states can multiply by up to the number of rows.
That's fine at toy and Main Mode sizes. If composition blows up, we can replace `Last` with
user-declared ghost vars.
