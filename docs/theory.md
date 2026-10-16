# fsmgen, formally: a guided tour of the Phase 0 spike

This is a study guide to `examples/toy-handshake/`. Each section states the theory first, then
points at the code that implements it. Code references use `path:line` from the repo root, as of
this commit. The references at the end are the original papers and standard textbooks.

**Suggested reading order:**

1. §1–2 with `examples/toy-handshake/src/initiator.rs`: what the machine *is*.
2. §3–4 with `examples/toy-handshake/src/model.rs` and `src/liveness.rs`: what we *check*.
3. §5 with `examples/toy-handshake/tests/differential.rs`: why the check is about the *code*.
4. §7–8: the planted bug, and the list of things to fix.

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
(Cheng & Krishnakumar 1993; Lee & Yannakakis 1996 §VI) add variables, plus guards and updates over
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
*simulates* the code (Milner 1971). Simulation preserves universal temporal properties, ∀CTL* /
ACTL* (Clarke, Grumberg & Long 1994). In particular:

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
*liveness* means "something good eventually happens". Alpern & Schneider (1985) made them precise.
A safety property is violated by a finite prefix. Every property is the intersection of a safety
property and a liveness property. In temporal logic (Pnueli 1977 for linear time; Clarke & Emerson
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
EF F = μZ. F ∨ EX Z          (Tarski 1955 gives the fixpoint; Emerson & Clarke the CTL reading)
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
see the past, so the model carries `last: Last` (`src/initiator.rs:157`). Abadi & Lamport (1991)
call this a *history variable*: it adds information about the past without changing behaviour.
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
   game: the attractor of `F` (Emerson & Jutla 1991; Grädel, Thomas & Wilke 2002), still linear
   time.
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

## References

**Automata and machines**
- M. O. Rabin, D. Scott. "Finite Automata and Their Decision Problems." *IBM J. Res. Dev.* 3(2):114–125, 1959.
- G. H. Mealy. "A Method for Synthesizing Sequential Circuits." *Bell System Technical Journal* 34(5):1045–1079, 1955.
- E. F. Moore. "Gedanken-Experiments on Sequential Machines." In *Automata Studies*, Annals of Mathematics Studies 34, Princeton, 1956, pp. 129–153.
- M. Sipser. *Introduction to the Theory of Computation*, 3rd ed. Cengage, 2012. Ch. 1.
- J. E. Hopcroft, R. Motwani, J. D. Ullman. *Introduction to Automata Theory, Languages, and Computation*, 3rd ed. Addison-Wesley, 2006. Ch. 2.
- K.-T. Cheng, A. S. Krishnakumar. "Automatic Functional Test Generation Using the Extended Finite State Machine Model." *DAC* 1993.
- D. Lee, M. Yannakakis. "Principles and Methods of Testing Finite State Machines — A Survey." *Proc. IEEE* 84(8):1090–1123, 1996.
- D. Harel. "Statecharts: A Visual Formalism for Complex Systems." *Science of Computer Programming* 8(3):231–274, 1987. (Source of the `event [guard] / action` notation; hierarchy is a v1 non-goal.)

**Guarded commands and requirements tables**
- E. W. Dijkstra. "Guarded Commands, Nondeterminacy and Formal Derivation of Programs." *CACM* 18(8):453–457, 1975.
- M. P. E. Heimdahl, N. G. Leveson. "Completeness and Consistency in Hierarchical State-Based Requirements." *IEEE TSE* 22(6):363–377, 1996.

**Temporal logic and model checking**
- S. A. Kripke. "Semantical Considerations on Modal Logic." *Acta Philosophica Fennica* 16:83–94, 1963.
- A. Pnueli. "The Temporal Logic of Programs." *FOCS* 1977, pp. 46–57.
- E. M. Clarke, E. A. Emerson. "Design and Synthesis of Synchronization Skeletons Using Branching Time Temporal Logic." *Logic of Programs* 1981, LNCS 131.
- J.-P. Queille, J. Sifakis. "Specification and Verification of Concurrent Systems in CESAR." *Int. Symp. on Programming* 1982, LNCS 137.
- L. Lamport. "Proving the Correctness of Multiprocess Programs." *IEEE TSE* SE-3(2):125–143, 1977.
- L. Lamport. "'Sometime' is Sometimes 'Not Never'." *POPL* 1980.
- B. Alpern, F. B. Schneider. "Defining Liveness." *Information Processing Letters* 21(4):181–185, 1985.
- C. Baier, J.-P. Katoen. *Principles of Model Checking*. MIT Press, 2008. (The best single textbook for §3–4.)
- E. M. Clarke, O. Grumberg, D. Peled. *Model Checking*. MIT Press, 1999.
- C. Courcoubetis, M. Y. Vardi, P. Wolper, M. Yannakakis. "Memory-Efficient Algorithms for the Verification of Temporal Properties." *Formal Methods in System Design* 1(2/3):275–288, 1992.
- R. E. Tarjan. "Depth-First Search and Linear Graph Algorithms." *SIAM J. Computing* 1(2):146–160, 1972.
- A. Tarski. "A Lattice-Theoretical Fixpoint Theorem and Its Applications." *Pacific J. Math.* 5(2):285–309, 1955.

**Fairness and games**
- N. Francez. *Fairness*. Springer, 1986.
- D. Lehmann, A. Pnueli, J. Stavi. "Impartiality, Justice and Fairness: The Ethics of Concurrent Termination." *ICALP* 1981, LNCS 115.
- E. A. Emerson, C. S. Jutla. "Tree Automata, Mu-Calculus and Determinacy." *FOCS* 1991.
- E. Grädel, W. Thomas, T. Wilke (eds.). *Automata, Logics, and Infinite Games*. LNCS 2500, Springer, 2002.

**Abstraction and refinement**
- R. Milner. "An Algebraic Definition of Simulation Between Programs." *IJCAI* 1971.
- C. A. R. Hoare. "Proof of Correctness of Data Representations." *Acta Informatica* 1(4):271–281, 1972.
- M. Abadi, L. Lamport. "The Existence of Refinement Mappings." *Theoretical Computer Science* 82(2):253–284, 1991.
- E. M. Clarke, O. Grumberg, D. E. Long. "Model Checking and Abstraction." *ACM TOPLAS* 16(5):1512–1542, 1994.
- E. M. Clarke, O. Grumberg, S. Jha, Y. Lu, H. Veith. "Counterexample-Guided Abstraction Refinement." *CAV* 2000, LNCS 1855.

**Testing**
- W. M. McKeeman. "Differential Testing for Software." *Digital Technical Journal* 10(1):100–107, 1998.

**For later phases**
- L. Lamport. *Specifying Systems: The TLA+ Language and Tools for Hardware and Software Engineers*. Addison-Wesley, 2002. (Phase 6.)
