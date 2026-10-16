//! The model checks clean, and the planted bug is caught.
//!
//!     cargo test -p toy-handshake --features model
//!     cargo test -p toy-handshake --features model,planted-bug

use stateright::{Checker, Model};
use toy_handshake::liveness::{check_termination, Liveness};
use toy_handshake::model::InitiatorModel;

#[test]
fn terminates() {
    let r = check_termination(&InitiatorModel, |s| s.control.is_terminal(), 1_000_000);
    assert!(matches!(r, Liveness::Ok { .. }), "{r:?}");
}

#[cfg(not(feature = "planted-bug"))]
#[test]
fn all_properties_hold() {
    InitiatorModel
        .checker()
        .spawn_bfs()
        .join()
        .assert_properties();
}

#[cfg(feature = "planted-bug")]
mod planted_bug {
    use super::*;
    use toy_handshake::model::Action;

    /// `[tries <= MAX_TRIES]` lets a 4th resend through. BFS finds the
    /// shortest path: Start, then MAX_TRIES + 1 timeouts.
    #[test]
    fn vars_out_of_range() {
        let checker = InitiatorModel.checker().spawn_bfs().join();
        let mut path = vec![Action::Start];
        path.extend([Action::Timeout; toy_handshake::MAX_TRIES as usize + 1]);
        checker.assert_discovery("vars in range", path);
    }

    /// The user property catches the same bug from the other side: the machine
    /// reaches Failed with the wrong count.
    #[test]
    fn failed_with_wrong_count() {
        let checker = InitiatorModel.checker().spawn_bfs().join();
        let path = checker.assert_any_discovery("Failed after exactly MAX_TRIES resends");
        assert_eq!(path.last_state().vars.tries, toy_handshake::MAX_TRIES + 1);
    }

    /// The bug doesn't affect the other properties.
    #[test]
    fn other_properties_still_hold() {
        let checker = InitiatorModel.checker().spawn_bfs().join();
        for name in [
            "no deadlock",
            "every event matches a row",
            "Done only via resp_valid",
        ] {
            checker.assert_no_discovery(name);
        }
    }
}
