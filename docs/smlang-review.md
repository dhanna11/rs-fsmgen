# smlang review: reuse or write our own?

Source read: `smlang` 0.8.0 / `smlang-macros` 0.8.0 from crates.io (MIT OR Apache-2.0).
The macro crate is about 2,200 lines: parser 900, codegen 710, validation 190, diagram 85.

## How it is built

**Parse layer** (`parser/state_machine.rs`, `transition.rs`, `input_state.rs`, `output_state.rs`, `event.rs`).
A `syn::parse::Parse` impl reads the top-level keys `name`, `transitions`, `temporary_context`,
`custom_error`, `derive_states`, and `derive_events`. Each row becomes a `StateTransitions`:

```text
in_states: Vec<InputState>        // `A | B`, `_` wildcard, `*` start marker, optional `(Type)`
event:     Event                  // ident + optional `(Type)`
guard:     Option<GuardExpression> // Guard(ident) | Not | Group | And | Or, optional `async`
action:    Option<AsyncIdent>     // ident, optional `async`
out_state: OutputState            // ident or `_` (internal), optional `(Type)`
```

`A | B + E` is expanded into one `StateTransition` per input state. The guard grammar is a small,
clean recursive-descent parser (`parse_or` → `parse_and` → `parse_not` → `parse_primary`). It accepts
**identifiers only**, with no comparisons or literals.

**IR** (`ParsedStateMachine`, `parser/mod.rs`):

```text
states:                HashMap<String, Ident>
events:                HashMap<String, Ident>
state_data/event_data: DataDefinitions { HashMap<String, syn::Type>, lifetimes }
starting_state:        Ident
states_events_mapping: HashMap<String /*state*/, HashMap<String /*event*/, EventMapping>>
EventMapping:          { in_state, event, transitions: Vec<Transition { guard, action, out_state: Ident }> }
```

States and events are collected implicitly from the rows (there is no declaration list). Wildcards
are expanded against the collected states. Within one (state, event) group the rows stay in table
order. The IR keeps no global row order and no row index.

**Validation** (`validation.rs`): a guard or action used on several rows must have the same data
signature on each. A guarded row after an unguarded one is an error, and two unguarded rows for
the same pair are an error. There is no totality check and no reachability check. Most errors use
`Span::call_site()`, so they point at the macro invocation rather than the offending row.

**Codegen** (`codegen.rs`): emits a `{Name}StateMachineContext` trait with
`fn guard(&self, &StateData, &EventData) -> Result<bool, E>` and
`fn action(&mut self, &StateData, EventData) -> Result<OutData, E>`, plus `on_entry_*`/`on_exit_*`,
logging hooks, and `process_event`. `process_event` is
`match self.state { S(ref d) => match event { E(e) => { if guard { action; self.state = ...; return } ... } _ => Err(InvalidEvent) } }`.
It also emits `PartialEq` impls that compare only the variant and ignore the data.

**Diagram** (`diagramgen.rs`, `graphviz` feature): builds DOT text and **runs the `dot` binary from
inside the proc macro**, writing an SVG into the current directory at compile time.

## How well it fits our plan

| Need | smlang | Gap |
|---|---|---|
| `vars` with ranges, var guards (`tries < MAX`), var updates `{ tries += 1 }` | none | Guard grammar, IR, and codegen all need new node types. |
| Shared IR for several backends (model, Kani, DOT, mutate) | IR lives inside the proc-macro crate and holds `syn` types | Would need to move into a normal crate and be rewritten around our own types. |
| Deterministic output (snapshot tests, `cargo expand`) | codegen iterates `HashMap`s, so arm order changes between builds | Every map would have to change to an ordered one. |
| Table order and row identity (row ids for counterexamples and the differential test) | lost when rows are grouped | IR redesign. |
| `total: true`, no catch-all arm | always emits `_ => Err(InvalidEvent)` | New feature, and the existing default must be removed. |
| Actions consume state data (`s: Src`) and return `(Target, Vec<Out>)` | actions take `&StateData` and return target data; no outputs | Signature model differs. |
| Panic safety (`Poisoned`) | assigns `self.state` after the action returns, so a panic leaves the old state in place, but nothing marks it | We'd change this. |
| `!g` right after `g` evaluated once | evaluated twice | Small change. |
| Errors with row spans | mostly `call_site()` | Every error site needs a span. |
| DOT | shells out at compile time; event nodes keyed by event name collide when an event has several rows | We'd rewrite this as a pure function in a separate crate. |
| `syn` version | `syn` 1, edition 2018 | Port to `syn` 2. |
| async, lifetimes, `temporary_context`, entry/exit hooks | yes | Not in our v1 scope. That's code to carry or delete. |

## What we would keep from a fork

- The guard-expression parser: about 80 lines, worth copying with attribution (MIT/Apache permits this).
- The `A | B` input-pattern and `_` wildcard ideas.
- The signature-consistency validation idea.

The rest of the IR and all of the codegen would be rewritten.

## Recommendation: **B, write our own parser and IR** (borrowing the guard grammar)

A fork would keep under a quarter of the code. It would also leave us maintaining async, lifetime,
and context features we don't want, while we redesign the IR underneath them. Our own `fsmgen-core`
is about 1,000 lines we fully understand. It is a plain library with no proc-macro dependency, so
the model, Kani, DOT, and mutation backends can share it. That's the main point of the plan's
"one IR" mitigation. The only new dependencies would be `syn` 2, `quote`, and `proc-macro2`.

Status: **proposed, needs lead sign-off.** See `docs/decisions.md` D-001.
