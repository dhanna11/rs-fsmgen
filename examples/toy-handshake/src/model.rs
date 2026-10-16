//! TARGET OUTPUT, written by hand. This is the stateright model the generator
//! should emit for the table in `initiator.rs` (behind the `model` feature).
//!
//! Semantics:
//! - Model state = control state + `Vars` + `Last`. State data is not modeled.
//! - Any event may arrive in any non-terminal state (single-machine model; the
//!   network is Phase 3).
//! - Opaque guards are part of the action: `Resp { resp_valid }` explores
//!   both outcomes. A guard that the current (state, event) group never
//!   calls is fixed at `false`, so each distinct step appears once.
//! - Var guards and updates call the same `Vars` methods as `Machine::step`.
//! - Terminal states have no actions.
//!
//! # Formal reading (see `docs/theory.md` §3–4)
//!
//! This is a Kripke structure `K = (S, S₀, →)` with
//! `S = Q × D × Last`, `S₀ = {(Idle, 0, Init)}`, and
//! `(q, d, _) → step((q, d, _), (σ, γ))` for every action `(σ, γ)`.
//! Fixing `γ` makes the code deterministic; leaving it free makes `K`
//! nondeterministic. Every run of the real code is a path in `K`, so `K`
//! *simulates* the code (Milner 1971). Universal properties (`AG p`) that
//! hold in `K` therefore hold in the code (Clarke, Grumberg & Long 1994).
//! Counterexamples can be spurious (a `resp_valid = true` the real guard
//! would never return); refining that is CEGAR (Clarke et al. 2000).

use stateright::{Model, Property};

use crate::initiator::{Control, EventKind, Last, Row, Vars};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ModelState {
    pub control: Control,
    pub vars: Vars,
    pub last: Last,
}

/// `(σ, γ) ∈ Σ × 2^Γ`, cut down to the guards that the current
/// (state, event) group actually consults. An event plus the value of every
/// opaque guard it could hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Start,
    Resp { resp_valid: bool },
    Timeout,
}

impl Action {
    pub fn kind(self) -> EventKind {
        match self {
            Action::Start => EventKind::Start,
            Action::Resp { .. } => EventKind::Resp,
            Action::Timeout => EventKind::Timeout,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct InitiatorModel;

impl InitiatorModel {
    pub const INIT: ModelState = ModelState {
        control: Control::Idle,
        vars: Vars::INIT,
        last: Last::Init,
    };

    /// The transition relation `→`. Same arms, same order as `Machine::step`:
    /// literally the same `δ` with `γ` taken from the action instead of from
    /// hooks.
    pub fn step(s: &ModelState, a: Action) -> ModelState {
        let mut vars = s.vars;
        let (control, last) = match (s.control, a) {
            // r0: Idle + Start / send_req { tries = 0 } = Waiting
            (Control::Idle, Action::Start) => {
                vars.r0_update();
                (Control::Waiting, Last::Row(Row::IdleStart))
            }
            // r1: Idle + Resp = _
            (Control::Idle, Action::Resp { .. }) => (Control::Idle, Last::Row(Row::IdleResp)),
            // r2: Idle + Timeout = _
            (Control::Idle, Action::Timeout) => (Control::Idle, Last::Row(Row::IdleTimeout)),
            // r3: Waiting + Start = _
            (Control::Waiting, Action::Start) => (Control::Waiting, Last::Row(Row::WaitingStart)),
            (Control::Waiting, Action::Resp { resp_valid }) => {
                // r4: Waiting + Resp [resp_valid] / accept = Done
                if resp_valid {
                    (Control::Done, Last::Row(Row::WaitingRespAccept))
                }
                // r5: Waiting + Resp [!resp_valid] / discard = _
                else {
                    (Control::Waiting, Last::Row(Row::WaitingRespDiscard))
                }
            }
            (Control::Waiting, Action::Timeout) => {
                // r6: Waiting + Timeout [tries < MAX_TRIES] / resend { tries += 1 } = _
                if vars.r6_guard() {
                    vars.r6_update();
                    (Control::Waiting, Last::Row(Row::WaitingTimeoutResend))
                }
                // r7: Waiting + Timeout [tries >= MAX_TRIES] / give_up = Failed
                else if vars.r7_guard() {
                    (Control::Failed, Last::Row(Row::WaitingTimeoutGiveUp))
                } else {
                    (Control::Waiting, Last::NoRow(a.kind()))
                }
            }
            // terminal: Done, Failed
            (Control::Done | Control::Failed, _) => (s.control, Last::NoRow(a.kind())),
        };
        ModelState {
            control,
            vars,
            last,
        }
    }
}

impl Model for InitiatorModel {
    type State = ModelState;
    type Action = Action;

    fn init_states(&self) -> Vec<ModelState> {
        vec![Self::INIT]
    }

    fn actions(&self, s: &ModelState, actions: &mut Vec<Action>) {
        match s.control {
            Control::Idle => actions.extend([
                Action::Start,
                Action::Resp { resp_valid: false },
                Action::Timeout,
            ]),
            Control::Waiting => actions.extend([
                Action::Start,
                Action::Resp { resp_valid: true },
                Action::Resp { resp_valid: false },
                Action::Timeout,
            ]),
            Control::Done | Control::Failed => {}
        }
    }

    fn next_state(&self, s: &ModelState, a: Action) -> Option<ModelState> {
        Some(Self::step(s, a))
    }

    fn properties(&self) -> Vec<Property<Self>> {
        let mut props = builtin_properties();
        props.extend(crate::props::user_properties());
        props
    }
}

/// Properties every generated model gets. Termination is not here: see
/// `liveness.rs` for why.
///
/// In CTL (Clarke & Emerson 1981): `Property::always(p)` checks `AG p`
/// (safety: p holds in every reachable state), `Property::sometimes(p)`
/// checks `EF p` (reachability). BFS gives a shortest witness or
/// counterexample, up to the quirk noted in decisions D-002.
pub fn builtin_properties() -> Vec<Property<InitiatorModel>> {
    vec![
        Property::always("no deadlock", |m: &InitiatorModel, s: &ModelState| {
            let mut acts = Vec::new();
            m.actions(s, &mut acts);
            s.control.is_terminal() || !acts.is_empty()
        }),
        Property::always("vars in range", |_, s: &ModelState| s.vars.in_range()),
        Property::always("every event matches a row", |_, s: &ModelState| {
            !matches!(s.last, Last::NoRow(_))
        }),
        Property::sometimes("reachable: Idle", |_, s: &ModelState| {
            s.control == Control::Idle
        }),
        Property::sometimes("reachable: Waiting", |_, s: &ModelState| {
            s.control == Control::Waiting
        }),
        Property::sometimes("reachable: Done", |_, s: &ModelState| {
            s.control == Control::Done
        }),
        Property::sometimes("reachable: Failed", |_, s: &ModelState| {
            s.control == Control::Failed
        }),
    ]
}
