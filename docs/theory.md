# fsmgen, formally: a guided tour of the Phase 0 spike

This is a study guide to `examples/toy-handshake/`. Each section states the theory first, then
points at the code that implements it. Code references use `path:line` from the repo root, as of
this commit. The references at the end are the original papers and standard textbooks.

**Suggested reading order:**

1. §1–2 with `examples/toy-handshake/src/initiator.rs`: what the machine *is*.
2. §3–4 with `examples/toy-handshake/src/model.rs` and `src/liveness.rs`: what we *check*.
3. §5 with `examples/toy-handshake/tests/differential.rs`: why the check is about the *code*.
4. §7–8: the planted bug, and the list of things to fix.
5. §9 with Lynch's *Distributed Algorithms*, ch. 8: how one machine becomes a composed system
   (Phase 3).

---

## §1. What kind of machine is this?

### 1.1 The textbook 5-tuple

A deterministic finite automaton (Rabin & Scott 1959; Sipser ch. 1; Hopcroft, Motwani & Ullman ch. 2) is

```text
A = (Q, Σ, δ, q₀, F)
    Q   finite set of states
    Σ   finite input alphabet
    δ   : Q × Σ → Q          total transition function
    q₀  ∈ Q                  start state
    F   ⊆ Q                  accepting states
```

A DFA only *recognizes* a language. It produces no output.

### 1.2 Adding output: Mealy machines

A protocol endpoint is a *transducer*: each input causes outputs (packets to send, "established").
Mealy (1955) puts the output on the transition:

```text
M = (Q, Σ, Λ, δ, λ, q₀)      λ : Q × Σ → Λ
```

Moore (1956) puts it on the state (`λ : Q → Λ`). We are Mealy-style, because `step()` returns
outputs from the action that fired. Two small differences:

- One step can emit zero or more outputs, so `λ : Q × Σ → Λ*` (a word, the `Vec<InitiatorOut>`).
- `F` comes back, but it means *terminal* (no further transitions), not *accepting*.

### 1.3 Adding data: extended finite-state machines (EFSMs)

Counting retries in pure control states would mean `Waiting0, Waiting1, Waiting2, …`. EFSMs
(Cheng & Krishnakumar 1993; Lee & Yannakakis 1996) add variables, plus guards and updates over
them. Our variant also has opaque guards, decided by code the generator never sees. The full
signature:

```text
M = (Q, Σ, Λ, Γ, D, R, q₀, d₀, F)
    Q    control states                     Control         src/initiator.rs:55
    Σ    input letters (payloads erased)    EventKind
    Λ    outputs                            InitiatorOut    src/types.rs
    Γ    opaque guard names                 {resp_valid}    Hooks, src/initiator.rs:223
    D    var domain                         {0..=MAX_TRIES} Vars, src/initiator.rs:169
    R    ORDERED list of rows               Row r0..r7      src/initiator.rs:130
    q₀,d₀ initial configuration             (Idle, 0)
    F    terminal states                    {Done, Failed}
```

Each row is a tuple

```text
rᵢ = (qᵢ, σᵢ, φᵢ, aᵢ, uᵢ, q'ᵢ)
    qᵢ ∈ Q, σᵢ ∈ Σ          source and event
    φᵢ : D × 2^Γ → 𝔹        guard: a boolean formula over var predicates and opaque names
    aᵢ                      action: a hook, opaque, produces data and outputs
    uᵢ : D → D              var update (identity if absent)
    q'ᵢ ∈ Q                 target (`= _` means q'ᵢ = qᵢ)
```

A *configuration* is a pair `(q, d) ∈ Q × D`. Because `D` is finite, the EFSM is a finite
description of an ordinary FSM over `Q × D`. That flat machine is what the model checker explores.
For the toy, `|Q × D| = 4 × 4 = 16`. Only 10 configurations are reachable, because `Idle` only
occurs with `tries = 0` and `Failed` only with `tries = MAX_TRIES`. Writing `|Q| · |D|` instead of
listing those 16 states is the whole point of the "E", and the product is also where state explosion
comes from.

---

## §2. The transition function, row by row

### 2.1 Rows are guarded commands, but deterministic

A row is a guarded command `φ → a; u; goto q'` (Dijkstra 1975). Dijkstra's `if … fi` chooses
*nondeterministically* among the commands whose guards are true, and *aborts* if none is. We make
two different choices:

- **First match.** Among the rows for `(q, σ)`, the lowest-indexed one whose guard holds fires.
  Table order is priority, like `cond` in Lisp or a Rust `match`.
- **Skip.** If no guard holds, the machine stays at `(q, d)` and emits nothing (`Last::NoRow`).

So, writing `γ ⊆ Γ` for the set of opaque guards that return true on this step:

```text
δ(q, d, σ, γ) = (q'ᵢ, uᵢ(d))    where i = min { i | qᵢ = q ∧ σᵢ = σ ∧ φᵢ(d, γ) }
              = (q, d)          if no such i exists, or if q ∈ F
```

Once `γ` is fixed, `δ` is a total *function*. This is the formula documented on `Machine::step`
(`src/initiator.rs:295`). The `match` arms below it are that formula, unrolled.
- `src/initiator.rs:317`: the group `(Waiting, Resp)` = r4, r5. `φ₅ = ¬φ₄`, so r5 is emitted as a
  plain `else` and the hook is called once.
- `src/initiator.rs:331`: the group `(Waiting, Timeout)` = r6, r7. The guards are var predicates the
  generator can't prove exhaustive, so a third `else` arm (NoRow) is always emitted
  (`src/initiator.rs:346`).

### 2.2 Totality, completeness, consistency

Three properties of a guard table, in the terms of Heimdahl & Leveson (1996), who analysed
exactly this for RSML state-machine requirements:

- **Totality over (q, σ):** every pair has at least one row. That's `total: true`. Rust's
  exhaustive `match` enforces it: there's no `_` arm, so adding an event without rows is a compile
  error.
- **Completeness over guards:** for every reachable `(q, d)` and every `γ`, *some* row's guard
  holds. The built-in model property "every event matches a row" checks this on reachable
  states (`src/model.rs:161`).
- **Consistency:** at most one guard in a group holds. First-match makes overlap *meaningful*
  (the earlier row wins), but overlap is still a smell. See §7: the planted bug is an
  inconsistency.

### 2.3 Why order is semantic (back to the smlang discussion)

The definition of `δ` uses `min i`. Row order is part of the machine's meaning, which is why the IR
must keep `R` as an ordered list with indices (`docs/smlang-review.md`). Grouping by `(q, σ)` and
keeping the order *within* a group is enough to compute `δ`. Grouping throws away the identity `i`,
which every backend needs to *talk about* rows.

---

## §3. The model is an abstraction of the code

### 3.1 Kripke structures

Model checkers work on Kripke structures (Kripke 1963), `K = (S, S₀, →, L)`: states, initial
states, a transition relation, and a labelling of states with atomic propositions. In
`src/model.rs`:

```text
S  = Q × D × Last                    ModelState
S₀ = {(Idle, 0, Init)}               InitiatorModel::INIT
s → s'  iff  s' = step(s, a) for some a ∈ actions(s)     src/model.rs:70, :126
L  = any Rust predicate on ModelState  (the properties)
```

(Textbook Kripke structures require `→` to be total. stateright allows states with no successors
and treats them as terminal. The usual fix-up adds a self-loop on every terminal state.)

### 3.2 Opaque guards become extra input

The real `resp_valid` depends on payload bytes the model doesn't have. The model *erases* the
payload (`Resp(Resp{..})` ↦ letter `Resp`) and makes the guard's value part of the action:
`Action::Resp { resp_valid: bool }` (`src/model.rs:41`). Formally, the abstract alphabet is
`Σ × 2^Γ`. Over that alphabet the model is deterministic again. The nondeterminism the model checker
explores is just *every possible input*, including every possible answer from every opaque guard.

### 3.3 Why results about the model apply to the code

Let `α(machine) = (control, vars, last)`. For every concrete step `c —e→ c'`, where the hooks
returned guard values `γ`, the model has a step `α(c) —(kind(e), γ)→ α(c')`. That is, `K`
*simulates* the code (Milner 1971). Abstractions of this kind preserve universal temporal
properties, ∀CTL* / ACTL* (Clarke, Grumberg & Long 1994, Thm. 5.6). Grumberg & Long (1994) state
the same preservation result for the simulation preorder in general. In particular:

- `AG p` in `K` ⇒ `AG p` in the code. **These results transfer.** ("vars in range", "Done only
  via resp_valid", and so on.)
- `EF p` in `K` does **not** transfer. The model can reach `Done` via `resp_valid = true`, but the
  real guard might never return true. Reachability properties are sanity checks on the model, not
  guarantees about the code.
- A counterexample to `AG p` in `K` may be **spurious**: it may need a guard value the real code
  can never produce. Finding and removing spurious counterexamples automatically is
  counterexample-guided abstraction refinement, CEGAR (Clarke, Grumberg, Jha, Lu & Veith 2000).
  For now a human reads the counterexample.

This argument rests on one premise: the simulation must really hold, i.e. the model's `step` must
agree with the code's `step`. That premise is §5.

### 3.4 The chaotic environment

`actions()` offers every event in every non-terminal state (`src/model.rs:126`). That is the *most
general environment*: anything can arrive at any time. Any real environment (a network, a peer) has
a subset of these behaviours. So an `AG` result here holds against *every* environment. That's
strong, but it can also make counterexamples unrealistic. Phase 3 replaces it with a modelled
network and peer.

---

## §4. Properties: safety, reachability, termination

### 4.1 Safety and liveness

Lamport (1977) named the two classes informally: *safety* means "nothing bad happens" and
*liveness* means "something good eventually happens". Alpern & Schneider (1985) gave the formal
definition of liveness and a topological characterization of both: a safety property is violated
by a finite prefix, and every property is the intersection of a safety property and a liveness
property. In temporal logic (Pnueli 1977 for linear time; Clarke & Emerson
1981 and Queille & Sifakis 1982 for branching time and model checking):

| stateright | logic | class | meaning |
|---|---|---|---|
| `Property::always(p)` | `AG p` | safety | `p` in every reachable state |
| `Property::sometimes(p)` | `EF p` | reachability | some reachable state has `p` |
| `Property::eventually(p)` | `AF p` | liveness | every maximal path hits `p` |

`always` and `sometimes` are plain graph reachability. BFS from `S₀` decides them and yields a
shortest witness or counterexample (`src/model.rs:161`).

### 4.2 Why we don't use `eventually`

`AF p` fails exactly when some maximal path avoids `p`. In a finite graph an infinite path is a
*lasso*: a stem followed by a cycle. stateright only reports counterexamples that end in a dead
end and does not detect lassos (its docs say so). Standard algorithms for lassos are nested
depth-first search (Courcoubetis, Vardi, Wolper & Yannakakis 1992) or SCC decomposition (Tarjan
1972).

Our toy *has* lassos, and in fact `AF (Done ∨ Failed)` is **false** without fairness:
`Waiting —Resp{resp_valid:false}→ Waiting` forever, or `Idle —Timeout→ Idle` forever. stateright
would wrongly say "holds", because those lassos are invisible to it.

### 4.3 What we check instead: `AG EF F`, and why that means "terminates under fairness"

`src/liveness.rs:37` checks `AG EF F`: from every reachable state, some terminal state is still
reachable. `AG EF` is a branching-time property and has no LTL equivalent (Lamport 1980 discusses
this split between linear and branching time). It is computed as a least fixpoint:

```text
EF F = μZ. F ∨ EX Z          (Tarski 1955 gives the fixpoint; Emerson & Clarke 1980 the CTL reading)
```

That's backward BFS from `F` over reversed edges (`src/liveness.rs:69`), `O(|S| + |→|)`, followed
by checking `Reach ⊆ EF F`. On failure it returns the shortest stem to a stuck state
(`src/liveness.rs:94`).

**Claim.** Suppose `K` is finite, terminal states have no successors, and every reachable
non-terminal state has a successor (the "no deadlock" property). Then `K ⊨ AG EF F` iff every
*strongly fair* maximal path is finite and ends in `F`. Strong fairness (also called compassion;
Francez 1986; Lehmann, Pnueli & Stavi 1981) says: an edge that is enabled infinitely often is
taken infinitely often.

*Proof (⇒).* Finite maximal paths end in a state with no successor, which by "no deadlock" is in
`F`. Suppose instead some strongly fair path `π` is infinite. `K` is finite, so the set `Inf(π)` of
states visited infinitely often is non-empty. Choose `s ∈ Inf(π)` minimising `dist(s, F)`, which
is finite because `s ⊨ EF F`, and is ≥ 1 because states in `F` have no successors and can't recur.
Let `s → t` be the first edge of a shortest path to `F`. `s` recurs, so that edge is enabled
infinitely often, and by fairness it's taken infinitely often, so `t ∈ Inf(π)`. But then either
`dist(t, F) = 0`, so `t ∈ F` recurs, which is impossible, or `dist(t, F) < dist(s, F)`, which
contradicts minimality. ∎

*(⇐)* If some reachable `s` cannot reach `F`, the states reachable from `s` form a closed region
with no `F` in it. A path that keeps visiting a bottom SCC of that region round-robin is strongly
fair and never terminates.

**Caveat (a real gap; see §8).** "Fair" here includes fairness over *guard outcomes*: it assumes
`Resp{resp_valid:true}` eventually happens if `Resp` keeps arriving. That's an assumption about the
environment, and it may be false for the real code. For the toy it doesn't matter, because the
escape route `Timeout*` (r6 then r7) uses only var guards. In general `AG EF F` is also not a
universal property, so §3.3's transfer argument doesn't cover it.

---

## §5. Making sure the model means the code

### 5.1 Refinement and the commuting square

§3.3 needs: for every concrete step, the abstract step taken from `α(c)` with the same guard
values lands on `α(c')`. Hoare (1972) introduced the abstraction function `α` to relate a concrete
data representation to an abstract one. The correctness condition is that this square commutes:

```text
  c  ──── Machine::step(e) ────▶  c'
  │α                              │α
  ▼                               ▼
 α(c) ── model.step(β(e, γ)) ──▶ α(c')
```

Plus two side conditions: the initial states correspond, and `β(e, γ)` is one of the actions the
model *offers* at `α(c)`. Without the second condition, the model could compute the right `step`
for an action it never explores.

### 5.2 Two defences

1. **By construction.** The var guards and updates (the precise part of `δ`) exist once, as methods
   on `Vars` (`src/initiator.rs:193`), and both `step` functions call them. The opaque part is
   structurally the same `if`/`else` chain in both. Decision D-005 records this. The generator
   will emit both arms from one IR walk.
2. **By testing.** `tests/differential.rs:98` runs 5,000 random runs. The hooks return random guard
   values and log them; `to_action` (`:81`) builds `β`. Each step checks that the square commutes
   and that `β` is offered by `actions()`. At the end it checks that every abstract edge type was
   exercised (`:158`). This is differential testing (McKeeman 1998) of the refinement claim. It
   samples; it doesn't prove.

### 5.3 `Last` is a history variable

Some properties are about *how* a state was entered: "Done only via r4". A state predicate can't
see the past, so the model carries `last: Last` (`src/initiator.rs:157`). This is a *history
variable*: an auxiliary variable that records the past without changing behaviour. Auxiliary
variables come from Owicki & Gries (1976). Abadi & Lamport (1991) show history variables, together
with their new *prophecy* variables, are enough to construct refinement mappings.
The cost is concrete. The 10 reachable configurations become 20 model states (run
`--example check`).

---

## §6. The atomic step

`step()` (`src/initiator.rs:295`) is a small transaction:

```text
take state, leaving Poisoned  →  compute (q', d') on a copy  →  commit state, vars, last
             src/initiator.rs:297                                src/initiator.rs:361
```

Actions take the source state's data *by value*. Rust's ownership has to move it out of
`self.state`, and `mem::replace` with a `Poisoned` placeholder is how you do that safely. If a hook
panics mid-way, the panic unwinds past the commit. You end up with `Poisoned` and the old `vars`,
never a half-applied `(q', d)`. It's the same idea as `std::sync::Mutex` poisoning. Every later
call returns `Err(Poisoned)` instead of panicking, so the Phase 4 Kani harness ("`step` never
panics") can hold.

The model doesn't contain `Poisoned`. It assumes hooks never panic. That's a gap (§8).

---

## §7. The planted bug, formally

Correct guards for `(Waiting, Timeout)`, with `M = MAX_TRIES`:

```text
φ₆(d) = d < M      u₆(d) = d + 1      (resend, stay)
φ₇(d) = d ≥ M                        (give up → Failed)
```

They are complete (`φ₆ ∨ φ₇ ≡ ⊤`) and consistent (`φ₆ ∧ φ₇ ≡ ⊥`): together they *partition* `D`.

The bug (`--features planted-bug`, `src/initiator.rs:193`) changes `φ₆` to `d ≤ M`. Now
`φ₆ ∧ φ₇ ≡ (d = M)`: the guards **overlap at the boundary**. First-match picks r6, so
`u₆(M) = M + 1 ∉ D`. The invariant `AG (d ∈ D)` fails, and BFS returns a shortest counterexample
of length `M + 2` (Start, then `M + 1` Timeouts). The user property
`AG (q = Failed → d = M)` fails on the same run.

Notice that the bug is a §2.2 *consistency* violation. That leads to the first item below.

---

## §8. Known gaps: your fix-it list

Ordered roughly by value-for-effort.

1. **Static consistency lint.** For each `(q, σ)` group, check that the var-guard parts are
   pairwise disjoint over `D`. Then the planted bug becomes a *compile error* rather than a
   model-check failure. `D` is finite and small, so enumeration decides it. Symbolic domains would
   need an SMT solver; that's out of scope for v1. Warn rather than error, because first-match
   overlap can be intended.
2. **Make the differential test systematic.** Instead of random runs, replay a BFS path to every
   reachable abstract state through the concrete `Machine`, using hooks that *force* the `γ` from
   the path, and check every outgoing abstract edge there. For finite `K` that covers every edge
   (`|→|` = a few dozen here).
3. **Termination fairness over guard outcomes** (§4.3 caveat). Check "terminates assuming fairness
   only over *events*, with guard outcomes chosen adversarially". That's a two-player reachability
   game: the attractor of `F` (Mazala 2002, §2.5.1; Zielonka 1998), still linear time.
4. **Panics aren't modelled.** Either add an abstract `Poisoned` sink reachable from every action
   (then "no deadlock" must exempt it), or have Kani prove the generated code panic-free and state
   that hooks must not panic.
5. **History-variable cost.** `Last` doubles the state space. Generate it only when a property
   mentions it, or let users declare ghost variables instead.
6. **stateright quirks** (D-002): `eventually` misses lassos (§4.2), and `always` discoveries are
   overwritten, so the reporter must trim paths. If these keep biting, `liveness.rs` is already
   most of an explicit-state checker of our own.
7. **Overflow.** `u₆` is `tries += 1` on a `u8`. The guard makes overflow unreachable, but only a
   proof (Kani, Phase 4) says so.

---

## §9. Beyond one machine: I/O automata

§1–8 cover a single endpoint. Phase 3 composes an initiator, a responder and a faulty network. The
formalism for that is the **input/output automaton** (Lynch & Tuttle 1987; tutorial 1989; Lynch
1996, ch. 8).

### 9.1 The ladder from Mealy to here

| # | Step | Flattens back to a finite Mealy machine? |
|---|---|---|
| 1 | Output words, `λ : Q × Σ → Λ*` | yes: the finitely many words used form the output alphabet |
| 2 | Vars, guards, updates (EFSM) | yes: states `Q × D`, `D` finite |
| 3 | Opaque guards | yes: inputs `Σ × 2^Γ` |
| 4 | Ordered rows, first match | yes: it only resolves overlap |
| 5 | Completion (`NoRow`), terminal states | yes: self-loops |
| 6 | **Composition over channels** (I/O automata) | **only if channels are bounded** |

Steps 1–5 make the description shorter without making the machine more expressive. Step 6 is the
only one that can leave finite state. Two communicating finite-state machines joined by unbounded
FIFO channels can simulate a Turing machine, so most questions about them are undecidable (Brand &
Zafiropulo 1983). So the plan's "bounded in-flight messages" is what keeps Phase 3 decidable, not a
performance tweak. Lossy channels are the friendly exception: safety stays decidable for them even
unbounded (Abdulla & Jonsson 1996).

### 9.2 The definition

Following Lynch & Tuttle (1989, §3), an I/O automaton `A` has:

```text
sig(A)    = (in, out, int)      disjoint sets of input, output and internal actions
                                ext = in ∪ out  (visible)    local = out ∪ int  (A controls these)
states(A)                       a set of states, possibly infinite
start(A)  ⊆ states(A)           non-empty
steps(A)  ⊆ states × acts × states
part(A)                         a partition of local into classes ("components")
```

with one axiom:

> **Input-enabled.** For every state `s` and every input `π`, some step `(s, π, s')` exists.
> "The automaton is unable to block its input."

The ideas, in order:

- **Who controls what.** An automaton decides *when* its outputs and internal actions happen.
  Its environment decides when inputs happen. Each action is controlled by at most one component.
  That's the other axiom the paper calls fundamental.
- **Executions and behaviours.** An execution is `s₀ π₁ s₁ π₂ …`. Its *behaviour* is the
  subsequence of external actions. The semantics is trace semantics: a *problem* is a set of
  allowed behaviours, and `A` *solves* it if every finite or fair behaviour of `A` is in the set.
- **Composition** `A₁ · … · Aₙ` requires *compatibility*: no two components share an output,
  and one component's internal actions aren't actions of any other. Composition is then the
  product of states, where:
  - **Shared actions synchronise.** When `π` happens, every component that has `π` in its
    signature takes a `π`-step at the same time. A shared action is always one component's output
    and the others' inputs, so it's a broadcast that can't be refused, because inputs are enabled.
  - **Non-shared actions interleave.** Everyone else stays put.
  - The composite's outputs are the union of the outputs. Its inputs are the inputs nobody
    outputs. The composite is input-enabled automatically.
- **Hiding.** `hide Φ A` turns some outputs into internal actions. That's how you seal the
  initiator–network–responder messages inside the system.
- **Fairness** (1989, §3.3). An execution is fair if, for every class `C` of `part(A)`:
  1. if it's finite, no action of `C` is enabled in the final state;
  2. if it's infinite, it contains infinitely many `C`-events or infinitely many states where `C`
     is disabled.

  That is **weak fairness per component**: a component that stays enabled eventually acts. Inputs
  are never subject to fairness; they belong to someone else.
- **Compositionality.** Projecting an execution of a composite onto a component gives an execution
  of that component (1989, Proposition 1). With the converses, properties of the parts give
  properties of the whole.

**Contrast with CSP** (Hoare 1978). In CSP, a communication happens only when *both* sides are
ready, so a process can refuse a message by not offering it. In an I/O automaton a receiver can
never refuse. For network protocols that's the honest model: a host can't stop packets arriving,
only decide what to do with them.

### 9.3 Our machine as an I/O automaton

| I/O automaton | fsmgen |
|---|---|
| input actions | `Event`: `Start`, `Resp(m)`, `Timeout` |
| output actions | `InitiatorOut`: `Send(m)`, `Established`, `Failed` |
| input-enabled | `total: true` plus `NoRow`: every event is accepted in every state |
| states | `(q, d, data)` plus an **outbox** (see below) |
| classes of `part` | e.g. `{send}` and `{established, failed}` |

One mismatch matters. `Machine::step` is Mealy-style: one call takes an input *and* emits its
outputs atomically. In an I/O automaton, outputs are separate actions the automaton fires later,
on its own schedule. The standard encoding:

```text
input  recv(m):   s.q, s.d := δ(...);  s.outbox := s.outbox ++ λ(...)     (always enabled)
output send(m):   pre: head(s.outbox) = m;  eff: s.outbox := tail(s.outbox)
```

The generated code doesn't change: the driver loop is the thing that drains the outbox. But the
Phase 3 *model* should keep the outbox explicit, so that "output computed but not yet sent" is a
state the checker can see.

### 9.4 The Phase 3 system

```text
hide {send_*, recv_*} ( Initiator · Chan(I→R) · Responder · Chan(R→I) · Timer_I · Timer_R )
```

- **A channel is just another I/O automaton.** Its input is `send(m)` and its output is
  `recv(m)`. Network faults are choices of channel automaton:
  - **Reordering:** the state is a multiset, not a queue.
  - **Loss:** an internal action `drop(m)`.
  - **Duplication:** `recv(m)` doesn't remove `m`.
  - **A bound `k`:** input-enabledness forces a decision when the channel is full. It must accept
    `send` anyway, so a full channel drops the message, which is itself a kind of loss.
- **Timeouts become a timer automaton's output.** It's enabled while its owner is waiting. Under
  weak fairness, an enabled timer eventually fires.
- **stateright already implements this composition.** `stateright::actor::ActorModel` has the
  channel variants `Network::{UnorderedDuplicating, UnorderedNonDuplicating, Ordered}`,
  `LossyNetwork::{Yes, No}`, and the actions `Deliver`, `Drop` and `Timeout` (stateright 0.31,
  `src/actor/{network,model}.rs`). Phase 3 can generate actors rather than writing its own
  channels.

### 9.5 Fairness, revisited

This fixes the caveat in §4.3. There, guard outcomes such as `resp_valid` were nondeterministic
choices, and "termination under strong fairness" quietly assumed `true` would eventually be
chosen. In the composed system, guard outcomes are no longer choices: they're *determined by the
message* the channel delivers. Fairness is then owed only to *components*:
- the timer eventually fires;
- the channel eventually delivers, if it is fair-lossy;
- each endpoint eventually sends what's in its outbox.

That is a much more honest assumption.

The check changes accordingly. "Every fair run terminates" fails in one of two ways:

- **An infinite fair run.** Some reachable *non-trivial* strongly connected component `S` (one that
  contains a cycle) avoids `F` and, for every class `C`, either contains a `C`-edge or contains a
  state where `C` is disabled. A cycle through `S` that covers those witnesses is a fair run that
  never terminates.
- **A finite fair run.** The run stops in a non-terminal state where no class is enabled. The
  "no deadlock" property already catches this.

Finding the first case is a linear-time pass over Tarjan's SCCs (Tarjan 1972). In Phase 3 it
replaces `AG EF F` in `src/liveness.rs`.

### 9.6 Extensions we are *not* taking

- **Real time.** `Timeout` stays an untimed input. That's sound for safety, but it can't express
  deadlines. That would need timed automata (Alur & Dill 1994) or Lynch's timed I/O automata.
- **Unbounded data in control flow.** IKE message IDs and SPIs live inside opaque guards. If one
  ever drives control flow, use a data-independence argument (Wolper 1986): a protocol that only
  compares such values for equality needs only a few distinct values in the model.
- **A malicious network.** Loss, duplication and reordering make a careless network. One that
  forges and injects messages is the Dolev–Yao attacker (Dolev & Yao 1983), which is
  cryptographic-protocol territory and a non-goal in the plan.
- **Hierarchy, stacks, probability.** Statecharts, pushdown automata, Markov chains: none needed.

### 9.7 Reading plan (Lynch, *Distributed Algorithms*, 1996)

**Warm-up (free, 28 pages):** Lynch & Tuttle, "An Introduction to Input/Output Automata" (1989).
Its §2–3 cover everything in §9.2 above, using a candy-machine example. Read this first.

**Chapter 8, "Modelling II: Asynchronous System Model".** Read all of it before Phase 3.

| § | Title | Read it for |
|---|---|---|
| 8.1 | I/O Automata | the definition, and input-enabledness ↔ `total: true` |
| 8.2 | Operations on Automata | composition and hiding: how §9.4's system is built |
| 8.3 | Fairness | the weak, per-component fairness of §9.5 |
| 8.4 | Inputs and Outputs for Problems | specifications as sets of behaviours: how to state Phase 3 properties |
| 8.5 | Properties and Proof Methods | invariants and simulation proofs, the formal side of our §5 refinement check |

**Chapter 22, "Data Link Protocols".** Read it before writing the network model. Reliable delivery
over channels that lose, duplicate and reorder messages is IKE retransmission's problem exactly.
§22.1 states the problem; §22.2 is Stenning's protocol. The plan's planted bug "responder doesn't
resend its cached reply on a duplicate" is a data-link bug.

**Skip for now:**
- Chapters 1–7: the synchronous model.
- Chapter 23, "Partially Synchronous System Models": only if timeouts need real time bounds.

---

## References

Every entry below was checked against the publisher's record (Crossref DOI metadata, publisher
pages, or library catalogues by ISBN) in October 2026. The links are:

- **doi**: the publisher's copy.
- **free**: a legitimate free copy: the author's own site, the publisher's open archive, or an
  official archive. Where it is a preprint or tech-report version rather than the published text, the
  entry says so. No shadow-library links.
- **book**: the publisher's page and Amazon (by ISBN-10).

The ACM Digital Library is now open access, so the `10.1145/…` DOIs should open without a
subscription.

### Start here (books)

- C. Baier, J.-P. Katoen. *Principles of Model Checking*. MIT Press, 2008. ISBN 978-0-262-02649-9.
  The best single textbook for §3–4.
  [book](https://mitpress.mit.edu/9780262026499/principles-of-model-checking/) ·
  [Amazon](https://www.amazon.com/dp/026202649X)
- E. M. Clarke, O. Grumberg, D. Peled. *Model Checking*. MIT Press, 1999. ISBN 978-0-262-03270-4.
  [Amazon](https://www.amazon.com/dp/0262032708).
  2nd ed.: E. M. Clarke, O. Grumberg, D. Kroening, D. Peled, H. Veith, MIT Press, 2018,
  ISBN 978-0-262-03883-6. [book](https://mitpress.mit.edu/9780262038836/model-checking/) ·
  [Amazon](https://www.amazon.com/dp/0262038838)
- M. Sipser. *Introduction to the Theory of Computation*, 3rd ed. Cengage, 2013 (released 2012).
  ISBN 978-1-133-18779-0. Ch. 1, Def. 1.5: the 5-tuple.
  [book](https://www.cengage.com/c/introduction-to-the-theory-of-computation-3e-sipser/9781133187790/) ·
  [Amazon](https://www.amazon.com/dp/113318779X)
- J. E. Hopcroft, R. Motwani, J. D. Ullman. *Introduction to Automata Theory, Languages, and
  Computation*, 3rd ed. Pearson/Addison-Wesley, 2006 (© 2007). ISBN 978-0-321-45536-9. Ch. 2.
  [book](https://www.pearson.com/en-us/subject-catalog/p/introduction-to-automata-theory-languages-and-computation/P200000003517/9780321455369) ·
  [Amazon](https://www.amazon.com/dp/0321455363)

### Automata and machines

- M. O. Rabin, D. Scott. "Finite Automata and Their Decision Problems." *IBM J. Res. Dev.*
  3(2):114–125, 1959. [doi](https://doi.org/10.1147/rd.32.0114)
- G. H. Mealy. "A Method for Synthesizing Sequential Circuits." *Bell System Technical Journal*
  34(5):1045–1079, 1955. [doi](https://doi.org/10.1002/j.1538-7305.1955.tb03788.x) ·
  [free (Internet Archive BSTJ collection)](https://archive.org/details/bstj34-5-1045)
- E. F. Moore. "Gedanken-Experiments on Sequential Machines." In C. E. Shannon, J. McCarthy (eds.),
  *Automata Studies*, Annals of Mathematics Studies 34, Princeton University Press, 1956,
  pp. 129–153. [doi](https://doi.org/10.1515/9781400882618-006)
- K.-T. Cheng, A. S. Krishnakumar. "Automatic Functional Test Generation Using the Extended Finite
  State Machine Model." *Proc. 30th Design Automation Conference (DAC)*, 1993, pp. 86–91.
  [doi](https://doi.org/10.1145/157485.164585)
- D. Lee, M. Yannakakis. "Principles and Methods of Testing Finite State Machines — A Survey."
  *Proc. IEEE* 84(8):1090–1123, 1996. [doi](https://doi.org/10.1109/5.533956)
- D. Harel. "Statecharts: A Visual Formalism for Complex Systems." *Science of Computer
  Programming* 8(3):231–274, 1987. [doi](https://doi.org/10.1016/0167-6423(87)90035-9) (free in
  Elsevier's open archive). Harel labels transitions `α(P)/S`: event, parenthesised condition,
  action (§5). The bracketed `e[c]/a` spelling came later, in STATEMATE and UML. Hierarchy, the
  paper's main subject, is a v1 non-goal.

### Guarded commands and requirements tables

- E. W. Dijkstra. "Guarded Commands, Nondeterminacy and Formal Derivation of Programs." *CACM*
  18(8):453–457, 1975. [doi](https://doi.org/10.1145/360933.360975) ·
  [free (EWD 472)](https://www.cs.utexas.edu/~EWD/ewd04xx/EWD472.PDF)
- M. P. E. Heimdahl, N. G. Leveson. "Completeness and Consistency in Hierarchical State-Based
  Requirements." *IEEE TSE* 22(6):363–377, 1996. [doi](https://doi.org/10.1109/32.508311).
  It analyses "some aspects of" completeness and consistency, validated on the TCAS II
  specification, which is written in RSML.

### Temporal logic and model checking

- S. A. Kripke. "Semantical Considerations on Modal Logic." *Acta Philosophica Fennica* 16:83–94,
  1963. No DOI. (One review in *J. Symbolic Logic* gives the title as "…for modal logics".)
- A. Pnueli. "The Temporal Logic of Programs." *Proc. 18th FOCS*, 1977, pp. 46–57.
  [doi](https://doi.org/10.1109/SFCS.1977.32)
- E. A. Emerson, E. M. Clarke. "Characterizing Correctness Properties of Parallel Programs Using
  Fixpoints." *ICALP* 1980, LNCS, Springer, pp. 169–181.
  [doi](https://doi.org/10.1007/3-540-10003-2_69)
- E. M. Clarke, E. A. Emerson. "Design and Synthesis of Synchronization Skeletons Using Branching
  Time Temporal Logic." In D. Kozen (ed.), *Logics of Programs* (workshop, May 1981), LNCS 131,
  Springer, 1982, pp. 52–71. [doi](https://doi.org/10.1007/BFb0025774)
- J.-P. Queille, J. Sifakis. "Specification and Verification of Concurrent Systems in CESAR."
  *5th Int. Symp. on Programming*, LNCS 137, Springer, 1982, pp. 337–351.
  [doi](https://doi.org/10.1007/3-540-11494-7_22)
- L. Lamport. "Proving the Correctness of Multiprocess Programs." *IEEE TSE* SE-3(2):125–143,
  1977. [doi](https://doi.org/10.1109/TSE.1977.229904) ·
  [free (author)](https://lamport.azurewebsites.net/pubs/proving.pdf). This paper introduced the
  terms "safety" and "liveness".
- L. Lamport. "'Sometime' is Sometimes 'Not Never': On the Temporal Logic of Programs." *POPL*
  1980, pp. 174–185. [doi](https://doi.org/10.1145/567446.567463) ·
  [free (author)](https://lamport.azurewebsites.net/pubs/sometime.pdf)
- B. Alpern, F. B. Schneider. "Defining Liveness." *Information Processing Letters* 21(4):181–185,
  1985. [doi](https://doi.org/10.1016/0020-0190(85)90056-0) ·
  [free (Cornell TR 85-650)](https://hdl.handle.net/1813/6495)
- C. Courcoubetis, M. Y. Vardi, P. Wolper, M. Yannakakis. "Memory-Efficient Algorithms for the
  Verification of Temporal Properties." *Formal Methods in System Design* 1(2/3):275–288, 1992.
  [doi](https://doi.org/10.1007/BF00121128) ·
  [free (author postprint, ORBi)](https://orbi.uliege.be/bitstream/2268/164300/1/CVWY%20FMSD%2092.pdf).
  Nested depth-first search.
- R. E. Tarjan. "Depth-First Search and Linear Graph Algorithms." *SIAM J. Computing*
  1(2):146–160, 1972. [doi](https://doi.org/10.1137/0201010)
- A. Tarski. "A Lattice-Theoretical Fixpoint Theorem and Its Applications." *Pacific J. Math.*
  5(2):285–309, 1955. [doi](https://doi.org/10.2140/pjm.1955.5.285) ·
  [free (publisher)](https://msp.org/pjm/1955/5-2/pjm-v5-n2-p11-s.pdf)

### Fairness and games

- N. Francez. *Fairness*. Texts and Monographs in Computer Science, Springer, 1986.
  ISBN 978-0-387-96235-1. [doi](https://doi.org/10.1007/978-1-4612-4886-6) ·
  [Amazon](https://www.amazon.com/dp/0387962352)
- D. Lehmann, A. Pnueli, J. Stavi. "Impartiality, Justice and Fairness: The Ethics of Concurrent
  Termination." *ICALP* 1981, LNCS 115, Springer, pp. 264–277.
  [doi](https://doi.org/10.1007/3-540-10843-2_22)
- R. Mazala. "Infinite Games." Ch. 2 in E. Grädel, W. Thomas, T. Wilke (eds.), *Automata, Logics,
  and Infinite Games: A Guide to Current Research*, LNCS 2500, Springer, 2002, pp. 23–38.
  §2.5.1 covers reachability games and attractors, with the `O(m + n)` bound as an exercise.
  [doi (chapter)](https://doi.org/10.1007/3-540-36387-4_2) ·
  [doi (book)](https://doi.org/10.1007/3-540-36387-4) ·
  [Amazon](https://www.amazon.com/dp/3540003886)
- W. Zielonka. "Infinite Games on Finitely Coloured Graphs with Applications to Automata on
  Infinite Trees." *Theoretical Computer Science* 200(1–2):135–183, 1998.
  [doi](https://doi.org/10.1016/S0304-3975(98)00009-7)

### Abstraction and refinement

- R. Milner. "An Algebraic Definition of Simulation Between Programs." *Proc. 2nd IJCAI*, 1971,
  pp. 481–489. [free (IJCAI)](https://www.ijcai.org/Proceedings/71/Papers/044.pdf)
- C. A. R. Hoare. "Proof of Correctness of Data Representations." *Acta Informatica*
  1(4):271–281, 1972. [doi](https://doi.org/10.1007/BF00289507)
- S. Owicki, D. Gries. "An Axiomatic Proof Technique for Parallel Programs I." *Acta Informatica*
  6(4):319–340, 1976. [doi](https://doi.org/10.1007/BF00268134). Companion paper: "Verifying
  Properties of Parallel Programs: An Axiomatic Approach." *CACM* 19(5):279–285, 1976
  ([doi](https://doi.org/10.1145/360051.360224)). Auxiliary variables come from these papers.
- M. Abadi, L. Lamport. "The Existence of Refinement Mappings." *Theoretical Computer Science*
  82(2):253–284, 1991. [doi](https://doi.org/10.1016/0304-3975(91)90224-P) (free in Elsevier's open
  archive) · [free (author, DEC SRC report 29 version)](https://lamport.azurewebsites.net/pubs/abadi-existence.pdf).
  Introduces prophecy variables.
- E. M. Clarke, O. Grumberg, D. E. Long. "Model Checking and Abstraction." *ACM TOPLAS*
  16(5):1512–1542, 1994. [doi](https://doi.org/10.1145/186025.186051) ·
  [free (author; unreachable when checked)](https://orna.cswp.cs.technion.ac.il/wp-content/uploads/sites/72/2022/12/abstraction.pdf)
- O. Grumberg, D. E. Long. "Model Checking and Modular Verification." *ACM TOPLAS*
  16(3):843–871, 1994. [doi](https://doi.org/10.1145/177492.177725)
- E. M. Clarke, O. Grumberg, S. Jha, Y. Lu, H. Veith. "Counterexample-Guided Abstraction
  Refinement." *CAV* 2000, LNCS 1855, Springer, pp. 154–169.
  [doi](https://doi.org/10.1007/10722167_15)

### Composition and I/O automata

- N. A. Lynch, M. R. Tuttle. "Hierarchical Correctness Proofs for Distributed Algorithms."
  *PODC* 1987, pp. 137–151. [doi](https://doi.org/10.1145/41840.41852). Introduces I/O
  automata.
- N. A. Lynch, M. R. Tuttle. "An Introduction to Input/Output Automata." *CWI Quarterly*
  2(3):219–246, 1989. Also MIT/LCS/TM-373.
  [free (MIT)](https://groups.csail.mit.edu/tds/papers/Lynch/CWI89.pdf). The best starting point.
- N. A. Lynch. *Distributed Algorithms*. Morgan Kaufmann, 1996. ISBN 978-1-55860-348-6. Ch. 8
  (I/O automata), ch. 22 (data link protocols).
  [book](https://www.elsevier.com/books/distributed-algorithms/lynch/978-1-55860-348-6) ·
  [Amazon](https://www.amazon.com/dp/1558603484)
- C. A. R. Hoare. "Communicating Sequential Processes." *CACM* 21(8):666–677, 1978.
  [doi](https://doi.org/10.1145/359576.359585)
- D. Brand, P. Zafiropulo. "On Communicating Finite-State Machines." *JACM* 30(2):323–342, 1983.
  [doi](https://doi.org/10.1145/322374.322380)
- P. A. Abdulla, B. Jonsson. "Verifying Programs with Unreliable Channels." *Information and
  Computation* 127(2):91–101, 1996. [doi](https://doi.org/10.1006/inco.1996.0053)
- R. Alur, D. L. Dill. "A Theory of Timed Automata." *Theoretical Computer Science*
  126(2):183–235, 1994. [doi](https://doi.org/10.1016/0304-3975(94)90010-8)
- P. Wolper. "Expressing Interesting Properties of Programs in Propositional Temporal Logic."
  *POPL* 1986, pp. 184–193. [doi](https://doi.org/10.1145/512644.512661)
- D. Dolev, A. C. Yao. "On the Security of Public Key Protocols." *IEEE Trans. Information
  Theory* 29(2):198–208, 1983. [doi](https://doi.org/10.1109/TIT.1983.1056650)
- stateright actor model (Phase 3 target):
  [docs.rs, stateright 0.31 `actor`](https://docs.rs/stateright/0.31.0/stateright/actor/index.html)

### Testing

- W. M. McKeeman. "Differential Testing for Software." *Digital Technical Journal* 10(1):100–107,
  1998. No DOI. [free (HP's DTJ archive, the official copy; unreachable when checked)](https://www.hpl.hp.com/hpjournal/dtj/vol10num1/vol10num1art9.pdf)

### For later phases

- L. Lamport. *Specifying Systems: The TLA+ Language and Tools for Hardware and Software Engineers*.
  Addison-Wesley, 2002. ISBN 978-0-321-14306-8. (Phase 6.)
  [free for personal use (author)](https://lamport.azurewebsites.net/tla/book.html) ·
  [Amazon](https://www.amazon.com/dp/032114306X)
