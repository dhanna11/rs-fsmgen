//! Hand-written: the real logic behind the table's guards and actions.
//!
//! This is ordinary Rust. A real protocol would do parsing, crypto, and
//! packet building here.

use crate::initiator::Hooks;
use crate::types::{Done, InitiatorOut, Req, Resp, Waiting};

/// The initiator side's context. Nonces come from a counter so tests are
/// deterministic; a real implementation would use an RNG.
#[derive(Debug, Default)]
pub struct Client {
    next_nonce: u64,
}

impl Client {
    pub fn new(first_nonce: u64) -> Self {
        Client {
            next_nonce: first_nonce,
        }
    }

    fn session_id(nonce: u64, cookie: u64) -> u64 {
        // Stand-in for key derivation.
        nonce.rotate_left(17) ^ cookie.wrapping_mul(0x9e37_79b9_7f4a_7c15)
    }
}

impl Hooks for Client {
    fn resp_valid(&mut self, s: &Waiting, e: &Resp) -> bool {
        e.nonce == s.req.nonce
    }

    fn send_req(&mut self) -> (Waiting, Vec<InitiatorOut>) {
        let req = Req {
            nonce: self.next_nonce,
        };
        self.next_nonce = self.next_nonce.wrapping_add(1);
        (Waiting { req: req.clone() }, vec![InitiatorOut::Send(req)])
    }

    fn accept(&mut self, s: Waiting, e: Resp) -> (Done, Vec<InitiatorOut>) {
        let session = Self::session_id(s.req.nonce, e.cookie);
        (
            Done { session },
            vec![InitiatorOut::Established { session }],
        )
    }

    fn discard(&mut self, s: Waiting, _e: Resp) -> (Waiting, Vec<InitiatorOut>) {
        (s, Vec::new())
    }

    fn resend(&mut self, s: Waiting) -> (Waiting, Vec<InitiatorOut>) {
        let out = vec![InitiatorOut::Send(s.req.clone())];
        (s, out)
    }

    fn give_up(&mut self, _s: Waiting) -> Vec<InitiatorOut> {
        vec![InitiatorOut::Failed]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::initiator::{Control, Event, Last, Machine, Poisoned, Row, State};
    use crate::MAX_TRIES;

    fn started() -> (Machine, Client) {
        let mut m = Machine::new();
        let mut c = Client::new(7);
        let out = m.step(&mut c, Event::Start).unwrap();
        assert_eq!(out, vec![InitiatorOut::Send(Req { nonce: 7 })]);
        (m, c)
    }

    #[test]
    fn happy_path() {
        let (mut m, mut c) = started();
        let out = m
            .step(
                &mut c,
                Event::Resp(Resp {
                    nonce: 7,
                    cookie: 99,
                }),
            )
            .unwrap();
        let session = Client::session_id(7, 99);
        assert_eq!(out, vec![InitiatorOut::Established { session }]);
        assert!(matches!(m.state(), State::Done(Done { session: s }) if *s == session));
        assert_eq!(m.last(), Last::Row(Row::WaitingRespAccept));
    }

    #[test]
    fn wrong_nonce_is_dropped() {
        let (mut m, mut c) = started();
        let out = m
            .step(
                &mut c,
                Event::Resp(Resp {
                    nonce: 8,
                    cookie: 1,
                }),
            )
            .unwrap();
        assert!(out.is_empty());
        assert_eq!(m.state().control(), Some(Control::Waiting));
        assert_eq!(m.last(), Last::Row(Row::WaitingRespDiscard));
    }

    #[test]
    #[cfg_attr(feature = "planted-bug", should_panic)] // the planted bug breaks this
    fn resends_max_tries_times_then_fails() {
        let (mut m, mut c) = started();
        for i in 1..=MAX_TRIES {
            let out = m.step(&mut c, Event::Timeout).unwrap();
            assert_eq!(out, vec![InitiatorOut::Send(Req { nonce: 7 })]);
            assert_eq!(m.vars().tries, i);
        }
        let out = m.step(&mut c, Event::Timeout).unwrap();
        assert_eq!(out, vec![InitiatorOut::Failed]);
        assert_eq!(m.state().control(), Some(Control::Failed));
    }

    #[test]
    fn terminal_states_ignore_events() {
        let (mut m, mut c) = started();
        m.step(
            &mut c,
            Event::Resp(Resp {
                nonce: 7,
                cookie: 0,
            }),
        )
        .unwrap();
        for ev in [
            Event::Start,
            Event::Timeout,
            Event::Resp(Resp {
                nonce: 7,
                cookie: 0,
            }),
        ] {
            let kind = ev.kind();
            assert!(m.step(&mut c, ev).unwrap().is_empty());
            assert_eq!(m.state().control(), Some(Control::Done));
            assert_eq!(m.last(), Last::NoRow(kind));
        }
    }

    /// A hook that panics must leave the machine Poisoned with vars untouched.
    #[test]
    fn panicking_hook_poisons() {
        struct Boom;
        impl Hooks for Boom {
            fn resp_valid(&mut self, _: &Waiting, _: &Resp) -> bool {
                unreachable!()
            }
            fn send_req(&mut self) -> (Waiting, Vec<InitiatorOut>) {
                panic!("hook failed")
            }
            fn accept(&mut self, _: Waiting, _: Resp) -> (Done, Vec<InitiatorOut>) {
                unreachable!()
            }
            fn discard(&mut self, _: Waiting, _: Resp) -> (Waiting, Vec<InitiatorOut>) {
                unreachable!()
            }
            fn resend(&mut self, _: Waiting) -> (Waiting, Vec<InitiatorOut>) {
                unreachable!()
            }
            fn give_up(&mut self, _: Waiting) -> Vec<InitiatorOut> {
                unreachable!()
            }
        }

        let mut m = Machine::new();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            m.step(&mut Boom, Event::Start)
        }));
        assert!(r.is_err());
        assert!(matches!(m.state(), State::Poisoned));
        assert_eq!(m.vars().tries, 0);
        assert_eq!(m.step(&mut Client::new(0), Event::Start), Err(Poisoned));
    }
}
