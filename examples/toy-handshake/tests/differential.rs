//! Differential test: random event sequences through `Machine::step` and
//! through the model, with the same opaque-guard outcomes. Control state,
//! vars, and last row must agree after every step.
//!
//! This is the check that the generated model means the same thing as the
//! generated code. Model checking only shows that the model satisfies its
//! properties.
//!
//! Formally (theory §5): let `α(machine) = (control, vars, last)` be the
//! abstraction function (Hoare 1972) and `β(event, γ) = Action` the map from
//! a concrete event plus the guard values the hooks returned. The test checks
//! that this square commutes on every step it tries:
//!
//! ```text
//!   machine ──step(event)──▶ machine'
//!      │α                       │α
//!      ▼                        ▼
//!   s ─────model.step(β)────▶  s'
//! ```
//!
//! It is differential testing (McKeeman 1998) of a refinement claim. It
//! samples; it does not prove.

use stateright::Model;
use toy_handshake::initiator::{Event, EventKind, Hooks, Last, Machine};
use toy_handshake::model::{Action, InitiatorModel};
use toy_handshake::types::{Done, InitiatorOut, Req, Resp, Waiting};

/// xorshift64*: deterministic, no dependencies.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn bool(&mut self) -> bool {
        self.next() & 1 == 1
    }
}

/// Opaque guards return random values and log them. Actions return dummy
/// data, which the model ignores anyway.
struct Recording {
    rng: Rng,
    resp_valid: Vec<bool>,
}

impl Hooks for Recording {
    fn resp_valid(&mut self, _: &Waiting, _: &Resp) -> bool {
        let v = self.rng.bool();
        self.resp_valid.push(v);
        v
    }
    fn send_req(&mut self) -> (Waiting, Vec<InitiatorOut>) {
        (
            Waiting {
                req: Req { nonce: 0 },
            },
            Vec::new(),
        )
    }
    fn accept(&mut self, _: Waiting, _: Resp) -> (Done, Vec<InitiatorOut>) {
        (Done { session: 0 }, Vec::new())
    }
    fn discard(&mut self, s: Waiting, _: Resp) -> (Waiting, Vec<InitiatorOut>) {
        (s, Vec::new())
    }
    fn resend(&mut self, s: Waiting) -> (Waiting, Vec<InitiatorOut>) {
        (s, Vec::new())
    }
    fn give_up(&mut self, _: Waiting) -> Vec<InitiatorOut> {
        Vec::new()
    }
}

/// Turn what the runtime did into the model action with the same guard
/// outcomes. Guards that weren't called are `false`, as in the model.
fn to_action(kind: EventKind, h: &mut Recording) -> Action {
    let calls = std::mem::take(&mut h.resp_valid);
    assert!(
        calls.len() <= 1,
        "resp_valid called {} times in one step",
        calls.len()
    );
    match kind {
        EventKind::Start => Action::Start,
        EventKind::Resp => Action::Resp {
            resp_valid: calls.first().copied().unwrap_or(false),
        },
        EventKind::Timeout => Action::Timeout,
    }
}

#[test]
fn runtime_and_model_agree() {
    let model = InitiatorModel;
    let mut hooks = Recording {
        rng: Rng(0x0dd_ba11),
        resp_valid: Vec::new(),
    };
    let mut model_actions_seen = std::collections::HashSet::new();

    for run in 0..5_000 {
        let mut m = Machine::new();
        let mut s = InitiatorModel::INIT;

        for step in 0..12 {
            let ev = match hooks.rng.next() % 3 {
                0 => Event::Start,
                1 => Event::Resp(Resp {
                    nonce: 0,
                    cookie: 0,
                }),
                _ => Event::Timeout,
            };
            let kind = ev.kind();
            m.step(&mut hooks, ev).unwrap();
            let a = to_action(kind, &mut hooks);
            let ctx = format!("run {run} step {step}: {a:?} from {s:?}");

            if s.control.is_terminal() {
                // The model has no actions here; the runtime ignores the event.
                assert_eq!(m.state().control(), Some(s.control), "{ctx}");
                assert_eq!(*m.vars(), s.vars, "{ctx}");
                assert_eq!(m.last(), Last::NoRow(kind), "{ctx}");
                continue;
            }

            let mut enabled = Vec::new();
            model.actions(&s, &mut enabled);
            assert!(
                enabled.contains(&a),
                "{ctx}: model does not offer this action"
            );
            model_actions_seen.insert((s.control, a));

            s = model.next_state(&s, a).unwrap();
            assert_eq!(m.state().control(), Some(s.control), "{ctx}");
            assert_eq!(*m.vars(), s.vars, "{ctx}");
            assert_eq!(m.last(), s.last, "{ctx}");
        }
    }

    // Every (control, action) pair the model offers was hit at least once.
    let mut expected = std::collections::HashSet::new();
    for c in toy_handshake::initiator::Control::ALL {
        let mut acts = Vec::new();
        let probe = toy_handshake::model::ModelState {
            control: c,
            ..InitiatorModel::INIT
        };
        model.actions(&probe, &mut acts);
        expected.extend(acts.into_iter().map(|a| (c, a)));
    }
    assert_eq!(model_actions_seen, expected);
}
