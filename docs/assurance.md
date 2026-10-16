# Assurance case

What we actually know about fsmgen's results, and why. Every claim fsmgen produces passes through
the links below, and is only as strong as the weakest one. Update this file whenever a phase lands
or a status changes.

**Current scope:** the Phase 0 spike, `examples/toy-handshake/`. Nothing about IKE is claimed yet.

## Status legend

| Status | Meaning |
|---|---|
| **Proved** | A theorem: published and peer-reviewed, or a proof in this repo that someone other than its author has checked. |
| **Checked** | Mechanically and exhaustively verified over a finite space, e.g. every reachable state or edge. |
| **Tested** | Sampled. Evidence, not proof. |
| **Assumed** | Taken as given. Must appear in the assumptions ledger below. |
| **Open** | Not addressed yet. |

## The shape of a sound result

Never "the protocol is correct". Always:

> Under assumptions A₁…Aₙ, within bounds B, the model satisfies properties P₁…Pₘ. The model
> over-approximates the generated code, by construction plus [tested | checked] differential
> evidence. The hooks are verified separately by [Kani | unit tests | nothing].

Any clause that can't be filled in honestly makes the result weaker. Say so.

---

## Summary

| # | Link | Question | Status today |
|---|---|---|---|
| L1 | Specification | Are the table and properties what we actually want? | **Assumed** |
| L2 | Model ↔ code | Does the model over-approximate the generated code? | **Tested** (plus by construction for var expressions) |
| L3 | Opaque hooks | Is the logic inside `Hooks` correct? | **Tested** (unit tests only) |
| L4 | Environment | Does the real world stay within the modelled environment? | **Assumed** |
| L5 | Bounds | Do results within the bounds say anything beyond them? | **Assumed** (small scope hypothesis) |
| L6 | Theory | Do the theorems we rely on hold, and are their hypotheses met? | **Proved** (published) / **Open** (our §4.3 sketch) |
| L7 | Tools | Do the checker and our passes compute what they claim? | **Tested** |
| L8 | Generator | Does the generated code and model match the table? | **Open** (no generator yet) |

---

## L1. Specification

**Claim.** The transition table and the property list capture the intended behaviour.

**Why it matters most.** A model checker proves the model satisfies the properties *as written*.
It is silent about whether they're the right ones. The classic case is Needham–Schroeder. The
public-key protocol was published in 1978 and studied for years before Lowe found a
man-in-the-middle attack (Lowe 1995). He found it by model checking against an attacker the
earlier analyses hadn't considered (Lowe 1996). The mathematics wasn't wrong; the model of the
world was.

**Evidence today.**
- The toy's properties (`src/props.rs`, `src/model.rs` `builtin_properties`) are written by hand,
  not derived from any standard.
- The "reachable: X" properties guard against *vacuity*: a model so constrained that everything
  holds trivially. All four states are reachable.
- One planted spec bug (`--features planted-bug`) is caught by two properties. A second defect,
  `planted-drift`, is a code/model mismatch rather than a spec error; that's L2's job, not L1's
  (`theory.md` §7).

**Status.** Assumed.

**To strengthen.**
- For IKE, tie each property to a cited sentence of RFC 2409 and record the citation next to it.
- Phase 5: the red-team corpus and mutation testing. A mutant that survives means either an
  equivalent mutant (document why) or a missing property.
- Have the person who knows IKE best review the table and properties, separately from the code.

## L2. Model ↔ code (refinement)

**Claim.** Every execution of `Machine::step` corresponds to a path in the model. Formally, the
model simulates the code under the abstraction function α (`theory.md` §3.3, §5).

**Evidence today.**
- **By construction:** var guards and updates exist once, as `Vars` methods, and both
  `Machine::step` and the model's `step` call them (D-005).
- **Tested:** `tests/differential.rs` runs 5,000 random runs of 12 steps with random guard
  outcomes. It checks that control state, vars and `Last` agree after every step. It also checks
  that every action the runtime takes is one the model offers, and that every (control, action)
  pair the model offers was exercised at least once.
- **Demonstrated sensitive:** `--features planted-drift` removes a var update from the runtime
  only. The model checker still reports every property holding, and the differential test fails
  at the first resend (`theory.md` §7.2). This shows the test catches real drift, and that model
  checking alone can't.

**Status.** Tested.

**Gaps.**
- The opaque-guard structure, i.e. which `if`/`else` chain calls which hook, is duplicated by hand
  between `initiator.rs` and `model.rs`. The generator will emit both from one IR walk, which is
  stronger, but not proof.
- The model doesn't contain `Poisoned`: it assumes hooks never panic (A6).

**To strengthen.** Upgrade to **Checked**. The abstract model is finite, so replay a BFS path to
every reachable abstract state through the real `Machine`, using hooks that force the guard values
on that path, and check the commuting square on *every* outgoing edge (`theory.md` §8, item 2).

## L3. Opaque hooks

**Claim.** The hand-written `Hooks` implementation does what its names say: `resp_valid` accepts
exactly the valid responses, `accept` derives the right session, and so on.

**Evidence today.** Unit tests in `src/hooks.rs`.

**Status.** Tested.

**Note: this link is safe by over-approximation.** The model treats every opaque guard as an
unconstrained boolean. Any real correlation between guards is therefore already among the
behaviours the model explores, so *safety* results don't depend on the guards being independent.
The cost of that over-approximation is spurious counterexamples, never missed ones.

**To strengthen.** Phase 4: Kani harnesses for critical hooks, e.g. "failed validation never
mutates crypto or IV state".

## L4. Environment

**Claim.** The real environment behaves within what the model allows.

**Evidence.** None possible from inside the model. This link is the assumptions ledger.

**Status.** Assumed. See A1–A5.

**Note.** The single-machine model uses a *chaotic* environment: any event can arrive at any time.
That's the most general environment, so safety results there hold against *every* environment.
Phase 3 replaces it with modelled channels and peers. That's more realistic, but it's another
assumption to state (D-007).

## L5. Bounds

**Claim.** Results within the checked bounds are evidence about the system beyond them.

**Bounds today.** `MAX_TRIES = 3`; a single machine; no channel yet.

**Status.** Assumed. The justification is Jackson's *small scope hypothesis* (Jackson 2006): most
bugs show up in small instances. It's an empirical claim, not a theorem.

**To strengthen.** Check several values of each bound (e.g. `MAX_TRIES` ∈ {1, 2, 3, 5}) and look at
how state counts and results change. A result that holds at every size tried is better evidence
than one size.

## L6. Theory

| Result we rely on | Source | Status |
|---|---|---|
| Simulation preserves ∀CTL* (safety transfers from model to code) | Clarke, Grumberg & Long 1994; Grumberg & Long 1994 | Proved (published). Hypothesis = L2. |
| I/O-automaton projection and pasting (Propositions 1–3) | Lynch & Tuttle 1989 | Proved (published) |
| `EF F` as a least fixpoint | Tarski 1955; Emerson & Clarke 1980 | Proved (published) |
| `AG EF F` ⇔ every strongly fair maximal run terminates (given no deadlock, finite graph) | `theory.md` §4.3 | **Open.** A proof sketch written here, not yet independently checked |
| Fair-SCC criterion for weak fairness (Phase 3) | `theory.md` §9.5 | **Open.** Stated, not yet proved here |

**To strengthen.** Have someone other than the author check §4.3 and write out §9.5 in full. Both
are short; a mechanized proof is optional.

## L7. Tools

**Claim.** stateright and our own passes compute what they claim.

**Evidence today.**
- `src/liveness.rs` has unit tests on hand-built graphs with known answers: a livelock trap, and
  an exitable cycle.
- Two stateright limitations were found and worked around (D-002):
  - `eventually` misses counterexamples that end in a cycle, so we don't use it (D-006).
  - `always` discoveries get overwritten, so the reported path can run past the first violation;
    the reporter trims it.

**Status.** Tested.

**To strengthen.**
- A second, independent checker: the Phase 6 TLA+ export, checked with TLC. Disagreement between
  the two checkers is a finding.
- Keep a set of small models with hand-computed answers, and re-run it whenever stateright is
  upgraded.

## L8. Generator

**Claim.** The generated runtime, model, harnesses and diagram all mean what the table means.

**Status.** Open. There's no generator yet; Phase 0's target output is written by hand.

**Risk.** A generator bug can make code and model agree with each other (L2 passes) while both
disagree with the table. L2 can't catch that.

**To strengthen.**
- Snapshot tests of generated code, reviewed by a human when they change.
- Compare the generated output for the toy table against the hand-written spike in this repo.
- Mutation testing (Phase 5) exercises the whole pipeline end to end.

---

## Assumptions ledger

Every **Assumed** status above traces to an entry here. Add entries; don't delete them. Mark them
retired when they stop applying.

| # | Assumption | Used by | Notes |
|---|---|---|---|
| A1 | No message corruption beyond what opaque guards reject | L4 | Corruption is modelled only as a guard returning false |
| A2 | No adversary forges or injects messages (no Dolev–Yao attacker) | L4 | A plan non-goal; cryptographic analysis is out of scope |
| A3 | Timers eventually fire while armed | L4, termination | A fairness assumption (Phase 3: timer-automaton fairness) |
| A4 | A channel eventually delivers a message that is resent infinitely often | L4, termination | Phase 3; the fair-lossy channel |
| A5 | Real time doesn't matter for the properties checked | L4 | Timeouts are untimed inputs; no deadlines are claimed |
| A6 | Hooks don't panic | L2 | The model has no `Poisoned` state. The runtime handles it, but it isn't modelled |
| A7 | Results at small bounds reflect larger ones | L5 | The small scope hypothesis |
| A8 | Strong fairness over guard outcomes (single-machine termination only) | L6 | Unrealistic. Retired in Phase 3 by D-007's fairness over components |
| A9 | rustc, std, and stateright 0.31.0 behave as documented | L7 | The trusted base |

---

## References

- G. Lowe. "An Attack on the Needham-Schroeder Public-Key Authentication Protocol." *Information
  Processing Letters* 56:131–133, 1995. [doi](https://doi.org/10.1016/0020-0190(95)00144-2)
- G. Lowe. "Breaking and Fixing the Needham-Schroeder Public-Key Protocol Using FDR." *TACAS* 1996,
  LNCS 1055, Springer, pp. 147–166. [doi](https://doi.org/10.1007/3-540-61042-1_43)
- D. Jackson. *Software Abstractions: Logic, Language, and Analysis*. MIT Press, 2006
  (ISBN 978-0-262-10114-1). Revised edition 2012 (ISBN 978-0-262-01715-2).
  [Amazon (rev. ed.)](https://www.amazon.com/dp/0262017156)

Everything else cited here is in `docs/theory.md`'s reference list.
