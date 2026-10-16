# Phase 0 soundness review for adversarial review

Date: 2026-10-16  
Reviewer: Codex  
Repository: `dhanna11/rs-fsmgen`  
Reviewed branch: `claude/phase0-spike`  
Reviewed commit: `644918c5df999d7dd1fc8debe6cbae4615d10579`

This document records an initial review for a second, independent LLM review. It is not an
accepted design decision or a proof. Findings below distinguish an executed counterexample from
design obligations and limitations already acknowledged by the project.

## Assessment

The architecture is worth pursuing: a bounded control-flow DSL, readable runtime code, shared
variable expressions, and separate checks for specification errors and implementation drift.
The assurance ledger is useful because it distinguishes tested behavior from assumptions.

The current spike does not establish end-to-end soundness. There is no generator yet, and a
terminal-state mismatch makes the stated runtime/model refinement claim too broad. The composed
system also needs a contract for abstract messages, outputs, timers, and opaque hooks before its
results can be connected to runtime behavior.

## Evidence collected

At the reviewed commit, with Rust/Cargo 1.99.1:

- `cargo test --locked -p toy-handshake --features model`: 10 tests passed.
- `cargo test --locked -p toy-handshake --features model,planted-bug`: 12 tests passed,
  including tests expecting the planted defect.
- `cargo test --locked -p toy-handshake --features model,planted-drift`: 10 tests passed,
  including the expected differential-test panic.
- `cargo fmt --all --check`: passed.
- `cargo clippy --locked --all-targets --features toy-handshake/model -- -D warnings`: passed.

These establish that the existing examples and detectors behave as intended. They do not prove
refinement or generator correctness. No Kani run, IKE verification, or composed-network check was
performed. An additional standalone Rust reproduction confirmed R1 below. Repository source files
were checked against the GitHub snapshot; the reproduction did not modify them.

### Handoff verification

On 2026-10-16, the publication session verified the package checksums, confirmed that the
remote default branch still pointed to the reviewed commit above, and inspected the relevant
runtime, model, differential-test, and theory definitions. The supplied temporary integration
test was rerun against that commit: one test executed and failed at the final runtime assertion,
as expected for R1. The temporary test was then removed. The baseline checks listed above
were also run successfully earlier in this cloud session; no implementation fixes are included.
R2–R4 remain design obligations or conditional limitations, rather than demonstrated current
implementation defects. R5 is an evidence improvement.

## R1 — Confirmed: terminal history breaks the stated refinement mapping

References:

- [Runtime terminal handling](../examples/toy-handshake/src/initiator.rs), terminal match arms.
- [Model action enumeration](../examples/toy-handshake/src/model.rs), `Model::actions`.
- [User property](../examples/toy-handshake/src/props.rs), "Done only via resp_valid".
- [Differential test](../examples/toy-handshake/tests/differential.rs), terminal-state branch.
- [Refinement claim](theory.md), sections 3.3 and 5.

The runtime accepts events after reaching `Done`. Such an event preserves control state and
variables but writes `Last::NoRow(event_kind)`. The explored model offers no actions in terminal
states, so its `Last` remains the row that entered `Done`.

Executed trace:

| Input | Runtime control / Last | Explored model control / Last |
|---|---|---|
| Start | Waiting / IdleStart | Waiting / IdleStart |
| Valid Resp | Done / WaitingRespAccept | Done / WaitingRespAccept |
| Timeout | Done / NoRow(Timeout) | No terminal action; retains Done / WaitingRespAccept |

The actual property predicate is:

```rust
s.control != Control::Done || s.last == Last::Row(Row::WaitingRespAccept)
```

It holds in the explored model and is false on the runtime after the third input.

To reproduce, temporarily save the following as
`examples/toy-handshake/tests/terminal_history_review.rs` and run
`cargo test --locked -p toy-handshake --features model --test terminal_history_review`.
The final assertion fails. The temporary test is not part of this documentation change.

```rust
use toy_handshake::hooks::Client;
use toy_handshake::initiator::{Control, Event, Last, Machine, Row};
use toy_handshake::model::{Action, InitiatorModel};
use toy_handshake::types::Resp;

#[test]
fn terminal_history_review() {
    let mut runtime = Machine::new();
    let mut hooks = Client::new(7);
    let mut model = InitiatorModel::INIT;

    runtime.step(&mut hooks, Event::Start).unwrap();
    model = InitiatorModel::step(&model, Action::Start);

    runtime
        .step(&mut hooks, Event::Resp(Resp { nonce: 7, cookie: 99 }))
        .unwrap();
    model = InitiatorModel::step(&model, Action::Resp { resp_valid: true });

    assert_eq!(runtime.state().control(), Some(Control::Done));
    assert_eq!(runtime.last(), Last::Row(Row::WaitingRespAccept));
    assert_eq!(runtime.last(), model.last);

    runtime.step(&mut hooks, Event::Timeout).unwrap();

    // The model property is still true.
    assert!(
        model.control != Control::Done
            || model.last == Last::Row(Row::WaitingRespAccept)
    );

    // This is false on the runtime.
    assert!(
        runtime.state().control() != Some(Control::Done)
            || runtime.last() == Last::Row(Row::WaitingRespAccept)
    );
}
```

**Scope.** This does not show acceptance of an invalid response. The intended fact about *entry*
into Done can still be true. It shows that the chosen history representation and the broad
claim that the model simulates every runtime execution do not agree after termination.
The differential test explicitly special-cases this suffix instead of checking its usual
`Last` equality.

**Possible resolutions to assess.** Make terminal calls preserve the abstract history, introduce
a separate persistent entry-history marker, or model terminal inputs and express the property
about entry transitions. Explicitly restricting verification to execution prefixes ending at
termination is another possible contract, but it must narrow the stated transfer claim.
Merely adding terminal self-loops that update Last will expose the current property failure;
it does not repair the property's meaning.

## R2 — Open design obligation: composition needs hook/message contracts

References: `fsmgen-plan.md`, "Generated model" and Phase 3; D-007; theory section 9.5.

The single-machine model erases payloads and treats action data/output effects as opaque.
That is reasonable for properties over its control state and bounded variables. It is not yet
a definition of what another endpoint receives.

A composed backend needs an explicit abstraction describing:

- Which abstract outputs each action can emit, and how concrete outputs map to them.
- Which message and endpoint-state distinctions matter to guards.
- Timer arm/cancel behavior and the driver's handling of returned outputs.
- How those modeled effects are related to actual Hooks and driver implementations.

D-007 retains arbitrary Rust hooks. Section 9.5 says guard results become determined by the
delivered message, but modeling a channel alone cannot recover semantics erased from payloads,
endpoint data, and hooks. The toy's response guard depends on both the received nonce and the
stored request nonce.

**Requested resolution.** Define the abstract message/output interface and its refinement
obligations before claiming composed agreement or delivery properties. This may involve
user-supplied model adapters, contracts, or conservative nondeterminism. It does not require
putting cryptographic semantics into the DSL.

This is an architectural gap in the future backend, not a demonstrated failure of an existing
composed implementation; that implementation does not exist yet.

## R3 — Open design obligation: one guard value versus repeated mutable calls

References: `Hooks::resp_valid`; D-005; theory section 3.2; planned Boolean guard grammar.

The model gives each opaque guard name one Boolean value per step. Runtime guards take
`&mut self`, so a hook can change its answer between calls.

For example, a future row might be guarded by `g && !g`. A naive code generator could call
g twice. A stateful g returning true and then false would make that runtime expression true,
while the model's single Boolean valuation makes `g && !g` false for every valuation. The
runtime could then take a row that is unreachable in the model.

The toy does not exhibit this bug: its adjacent `g` / `!g` pair evaluates g once.

**Requested resolution.** Specify and implement lazy per-step caching of each named guard,
or model each evaluation separately with matching runtime semantics. Merely changing the Rust
receiver to `&self` would not prove purity because interior mutability remains possible.

## R4 — Conditional result: terminal reachability is not runtime inevitability

References: D-006; theory section 4.3; assurance assumptions A3, A4, A8.

The current pass checks `AG EF terminal`: every reachable state has a path to a terminal.
The finite-graph argument connecting this to termination under strong fairness over individual
edges appears valid under its stated hypotheses.

That fairness is stronger than realistic scheduling in general. An abstract accepting guard
outcome may remain possible forever while every concrete response fails validation. The docs
already acknowledge that the universal-safety transfer argument does not transfer this
branching-time result. The toy's bounded timeout path provides a separate escape route, assuming
timeouts actually occur.

For Phase 3, the fairness classes and delivery assumptions must be explicit. A channel that
repeatedly drops messages can keep taking actions and satisfy coarse component fairness without
delivering anything. Fair-loss delivery is an additional condition; it does not follow merely
from scheduling a channel fairly. Finite retries also mean eventual delivery assumptions alone
do not guarantee successful establishment before retry exhaustion.

**Requested resolution.** Report the current result as terminal reachability under the stated
assumptions. Check composed liveness against precisely defined fairness conditions, keeping
termination in failure separate from successful establishment.

This is primarily a limitation and a future proof obligation, not a newly discovered error in
the current backward-reachability algorithm.

## R5 — Evidence improvement: systematic abstract-edge replay

Reference: differential test; theory section 8, item 2; assurance L2.

The current test samples 5,000 runs of 12 steps. Its coverage set records
`(Control, Action)`, not every reachable `(Control, Vars, Last, Action)`.
Thus its final coverage assertion is not exhaustive state/edge coverage.

For this finite toy, replay a path to every reachable abstract state and check every outgoing
edge with forced guard outcomes. Cover the agreed terminal semantics too. This would strengthen
the abstract control-flow comparison and should precede substantial generator expansion.

Even exhaustive replay with representative dummy data would not prove arbitrary payload,
hook, output, or driver correctness. Keep those obligations explicit.

## Questions for the second reviewer

Review the implementation and claims independently; do not treat this document as ground truth.

1. Reproduce R1. Is the proposed refinement scope wrong, or is there an existing restriction
   that makes the apparent mismatch intentional and sound? Cite its exact definition.
2. Check R2 and R3 against the proposed DSL semantics. Identify a sufficient contract or give
   a concrete counterexample; distinguish current bugs from future implementation hazards.
3. Check the section 4.3 proof and the planned weak-fairness SCC criterion separately.
   State exactly which fairness classes and terminal assumptions each argument requires.
4. Challenge the recommendations: which are necessary for soundness, which only improve evidence,
   and which are unnecessary under an explicitly narrower claim?
5. Look for shared generator/model mistakes that differential agreement would miss.

For each finding, return **agree**, **disagree**, or **unresolved**, with source references,
an executable counterexample or proof argument where possible, and the smallest adequate fix.
Record limitations of your own review. Do not silently resolve a disagreement by assuming the
other reviewer is correct.
