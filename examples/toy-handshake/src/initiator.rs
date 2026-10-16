//! TARGET OUTPUT, written by hand. This is what `statemachine!` should expand
//! to for the table below. Nothing here is generated yet (Phase 0).
//!
//! ```text
//! statemachine! {
//!     name: Initiator,
//!     out: InitiatorOut,
//!     states: { *Idle, Waiting(Waiting), Done(Done), Failed },
//!     events: { Start, Resp(Resp), Timeout },
//!     terminal: [Done, Failed],
//!     vars: { tries: u8 in 0..=MAX_TRIES = 0 },
//!     transitions: {
//!         /* r0 */ Idle    + Start                                   / send_req { tries = 0 }  = Waiting,
//!         /* r1 */ Idle    + Resp                                                              = _,
//!         /* r2 */ Idle    + Timeout                                                           = _,
//!         /* r3 */ Waiting + Start                                                             = _,
//!         /* r4 */ Waiting + Resp    [resp_valid]                    / accept                  = Done,
//!         /* r5 */ Waiting + Resp    [!resp_valid]                   / discard                 = _,
//!         /* r6 */ Waiting + Timeout [tries < MAX_TRIES]             / resend   { tries += 1 } = _,
//!         /* r7 */ Waiting + Timeout [tries >= MAX_TRIES]            / give_up                 = Failed,
//!     },
//!     total: true,
//! }
//! ```
//!
//! Every item below is either used by the runtime, by the model, or by both.
//! The items shared with the model ([`Control`], [`EventKind`], [`Row`],
//! [`Last`], [`Vars`]) are the contract that keeps the two from drifting.
//!
//! # Formal reading (see `docs/theory.md`)
//!
//! The table defines an extended finite-state machine (EFSM)
//! `M = (Q, Σ, Λ, Γ, D, R, q₀, d₀, F)`:
//!
//! | symbol | meaning                          | here                              |
//! |--------|----------------------------------|-----------------------------------|
//! | `Q`    | control states                   | [`Control`]                       |
//! | `Σ`    | input alphabet (payloads erased) | [`EventKind`]                     |
//! | `Λ`    | output alphabet                  | `InitiatorOut`                    |
//! | `Γ`    | opaque guard names               | `{resp_valid}` ([`Hooks`])        |
//! | `D`    | var domain                       | `{0..=MAX_TRIES}` ([`Vars`])      |
//! | `R`    | **ordered** list of rows         | [`Row`], r0..r7                   |
//! | `q₀,d₀`| initial configuration            | `Idle`, `tries = 0`               |
//! | `F`    | terminal states                  | `{Done, Failed}`                  |
//!
//! [`Machine::step`] is the transition function `δ` (first-match over `R`)
//! together with the output function `λ` (whatever the action returns).

use super::types::{Done, InitiatorOut, Resp, Waiting};
use super::MAX_TRIES;

/// `Q`: the data-free part of [`State`]. This is what the model checker sees.
/// `q₀ = Idle`, `F = {Done, Failed}` (theory §1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Control {
    Idle,
    Waiting,
    Done,
    Failed,
}

impl Control {
    pub const ALL: [Control; 4] = [
        Control::Idle,
        Control::Waiting,
        Control::Done,
        Control::Failed,
    ];

    /// `terminal: [Done, Failed]`
    pub const fn is_terminal(self) -> bool {
        matches!(self, Control::Done | Control::Failed)
    }
}

/// Runtime state: control state plus the user's per-state data.
#[derive(Debug)]
pub enum State {
    Idle,
    Waiting(Waiting),
    Done(Done),
    Failed,
    /// A hook panicked during a transition. Every later `step()` returns
    /// `Err(Poisoned)`; nothing is ever half-updated.
    Poisoned,
}

impl State {
    /// `None` only for [`State::Poisoned`].
    pub fn control(&self) -> Option<Control> {
        match self {
            State::Idle => Some(Control::Idle),
            State::Waiting(_) => Some(Control::Waiting),
            State::Done(_) => Some(Control::Done),
            State::Failed => Some(Control::Failed),
            State::Poisoned => None,
        }
    }
}

#[derive(Debug)]
pub enum Event {
    Start,
    Resp(Resp),
    Timeout,
}

/// `Σ`: the data-free part of [`Event`]. Erasing payloads is the first
/// abstraction step: `Resp(Resp { .. })` becomes the letter `Resp` (theory §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EventKind {
    Start,
    Resp,
    Timeout,
}

impl Event {
    pub fn kind(&self) -> EventKind {
        match self {
            Event::Start => EventKind::Start,
            Event::Resp(_) => EventKind::Resp,
            Event::Timeout => EventKind::Timeout,
        }
    }
}

/// `R`: one variant per table row, in table order. Order is priority: for a
/// given (state, event), the first row whose guard holds fires (theory §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Row {
    /// r0: `Idle + Start / send_req { tries = 0 } = Waiting`
    IdleStart,
    /// r1: `Idle + Resp = _`
    IdleResp,
    /// r2: `Idle + Timeout = _`
    IdleTimeout,
    /// r3: `Waiting + Start = _`
    WaitingStart,
    /// r4: `Waiting + Resp [resp_valid] / accept = Done`
    WaitingRespAccept,
    /// r5: `Waiting + Resp [!resp_valid] / discard = _`
    WaitingRespDiscard,
    /// r6: `Waiting + Timeout [tries < MAX_TRIES] / resend { tries += 1 } = _`
    WaitingTimeoutResend,
    /// r7: `Waiting + Timeout [tries >= MAX_TRIES] / give_up = Failed`
    WaitingTimeoutGiveUp,
}

/// What the most recent `step()` did. Kept by the runtime (for tracing) and
/// by the model (so properties can talk about how a state was entered).
///
/// In the model this is a *history variable* (Owicki & Gries 1976): it
/// records part of the past so a state predicate can express a property of
/// the last transition. It costs state space: 10 configurations become 20
/// (theory §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Last {
    Init,
    Row(Row),
    /// No row fired: the state is terminal, or every guard in the
    /// (state, event) group was false. The machine stayed put.
    NoRow(EventKind),
}

/// `d ∈ D`: the model-visible variables. The declared domain `D` is smaller
/// than the Rust type (`u8` allows 0..=255); [`Vars::in_range`] is the
/// predicate `d ∈ D`, checked as an invariant by the model (theory §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Vars {
    pub tries: u8,
}

impl Vars {
    pub const INIT: Vars = Vars { tries: 0 };

    /// Declared ranges: `tries: u8 in 0..=MAX_TRIES`.
    pub fn in_range(&self) -> bool {
        (0..=MAX_TRIES).contains(&self.tries)
    }

    // Var guards and updates, one fn per row that has them. Both
    // `Machine::step` and the model call these; there is no second copy of
    // any expression.

    /// r0: `{ tries = 0 }`
    #[inline]
    pub fn r0_update(&mut self) {
        self.tries = 0;
    }

    /// r6: `[tries < MAX_TRIES]`
    #[inline]
    pub fn r6_guard(&self) -> bool {
        #[cfg(not(feature = "planted-bug"))]
        let ok = self.tries < MAX_TRIES;
        // PLANTED BUG: the table row was edited to `[tries <= MAX_TRIES]`.
        #[cfg(feature = "planted-bug")]
        let ok = self.tries <= MAX_TRIES;
        ok
    }

    /// r6: `{ tries += 1 }`
    #[inline]
    pub fn r6_update(&mut self) {
        self.tries += 1;
    }

    /// r7: `[tries >= MAX_TRIES]`
    #[inline]
    pub fn r7_guard(&self) -> bool {
        self.tries >= MAX_TRIES
    }
}

/// Everything the model does *not* see: opaque guards (`Γ`) and the data side
/// of actions. The model replaces each guard with an unconstrained boolean;
/// that is an existential abstraction (theory §3).
///
/// Implemented by hand. One method per unique guard and action in the table;
/// each signature comes from the source, event, and target types of the
/// rows that use it. Unit-typed states and events are left out of the
/// parameter list.
pub trait Hooks {
    /// `[resp_valid]` (r4, r5). Borrows: if it returns false, r5 still needs
    /// the state and event.
    fn resp_valid(&mut self, s: &Waiting, e: &Resp) -> bool;

    /// r0: `Idle + Start = Waiting`
    fn send_req(&mut self) -> (Waiting, Vec<InitiatorOut>);
    /// r4: `Waiting + Resp = Done`
    fn accept(&mut self, s: Waiting, e: Resp) -> (Done, Vec<InitiatorOut>);
    /// r5: `Waiting + Resp = Waiting`
    fn discard(&mut self, s: Waiting, e: Resp) -> (Waiting, Vec<InitiatorOut>);
    /// r6: `Waiting + Timeout = Waiting`
    fn resend(&mut self, s: Waiting) -> (Waiting, Vec<InitiatorOut>);
    /// r7: `Waiting + Timeout = Failed`
    fn give_up(&mut self, s: Waiting) -> Vec<InitiatorOut>;
}

/// Returned by `step()` after a hook has panicked mid-transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Poisoned;

#[derive(Debug)]
pub struct Machine {
    state: State,
    vars: Vars,
    last: Last,
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine {
    pub fn new() -> Self {
        Machine {
            state: State::Idle,
            vars: Vars::INIT,
            last: Last::Init,
        }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn vars(&self) -> &Vars {
        &self.vars
    }

    pub fn last(&self) -> Last {
        self.last
    }

    /// Feed one event. Synchronous and I/O-free; outputs are returned for the
    /// driver to act on.
    ///
    /// If a hook panics, the panic propagates, `vars` is unchanged, and the
    /// state is left as [`State::Poisoned`].
    ///
    /// Formally (theory §2): for configuration `(q, d)`, letter `σ`, and the
    /// guard values `γ ⊆ Γ` the hooks happen to return,
    ///
    /// ```text
    /// δ(q, d, σ, γ) = (q'ᵢ, uᵢ(d))   where i = min { i | qᵢ = q ∧ σᵢ = σ ∧ φᵢ(d, γ) }
    ///               = (q, d)         if no such i, or q ∈ F       (Last::NoRow)
    /// ```
    ///
    /// `q'ᵢ` is row i's target (`= _` means `q`), `uᵢ` its var update,
    /// `φᵢ` its guard. The step is atomic: either `(q', d')` is committed, or
    /// the machine ends up `Poisoned` with `d` untouched (theory §6).
    pub fn step<H: Hooks>(&mut self, h: &mut H, ev: Event) -> Result<Vec<InitiatorOut>, Poisoned> {
        let kind = ev.kind();
        let state = core::mem::replace(&mut self.state, State::Poisoned);
        // Var updates go to a copy that is committed only after the action
        // returns.
        let mut vars = self.vars;

        let (next, last, out) = match (state, ev) {
            // r0: Idle + Start / send_req { tries = 0 } = Waiting
            (State::Idle, Event::Start) => {
                let (t, out) = h.send_req();
                vars.r0_update();
                (State::Waiting(t), Last::Row(Row::IdleStart), out)
            }
            // r1: Idle + Resp = _
            (State::Idle, Event::Resp(_)) => (State::Idle, Last::Row(Row::IdleResp), Vec::new()),
            // r2: Idle + Timeout = _
            (State::Idle, Event::Timeout) => (State::Idle, Last::Row(Row::IdleTimeout), Vec::new()),
            // r3: Waiting + Start = _
            (State::Waiting(s), Event::Start) => {
                (State::Waiting(s), Last::Row(Row::WaitingStart), Vec::new())
            }
            (State::Waiting(s), Event::Resp(e)) => {
                // r4: Waiting + Resp [resp_valid] / accept = Done
                if h.resp_valid(&s, &e) {
                    let (t, out) = h.accept(s, e);
                    (State::Done(t), Last::Row(Row::WaitingRespAccept), out)
                }
                // r5: Waiting + Resp [!resp_valid] / discard = _
                // (`!resp_valid` directly after `resp_valid`: plain `else`,
                // the guard is not called twice)
                else {
                    let (t, out) = h.discard(s, e);
                    (State::Waiting(t), Last::Row(Row::WaitingRespDiscard), out)
                }
            }
            (State::Waiting(s), Event::Timeout) => {
                // r6: Waiting + Timeout [tries < MAX_TRIES] / resend { tries += 1 } = _
                if vars.r6_guard() {
                    let (t, out) = h.resend(s);
                    // PLANTED DRIFT: the runtime skips the update; the model doesn't.
                    #[cfg(not(feature = "planted-drift"))]
                    vars.r6_update();
                    (State::Waiting(t), Last::Row(Row::WaitingTimeoutResend), out)
                }
                // r7: Waiting + Timeout [tries >= MAX_TRIES] / give_up = Failed
                else if vars.r7_guard() {
                    let out = h.give_up(s);
                    (State::Failed, Last::Row(Row::WaitingTimeoutGiveUp), out)
                }
                // The generator cannot prove var guards exhaustive, so it
                // always emits this branch. The model flags it if it is
                // ever reached ("every event matches a row").
                else {
                    (State::Waiting(s), Last::NoRow(kind), Vec::new())
                }
            }
            // terminal: Done
            (State::Done(s), Event::Start | Event::Resp(_) | Event::Timeout) => {
                (State::Done(s), Last::NoRow(kind), Vec::new())
            }
            // terminal: Failed
            (State::Failed, Event::Start | Event::Resp(_) | Event::Timeout) => {
                (State::Failed, Last::NoRow(kind), Vec::new())
            }
            (State::Poisoned, _) => return Err(Poisoned),
        };

        self.state = next;
        self.vars = vars;
        self.last = last;
        Ok(out)
    }
}
