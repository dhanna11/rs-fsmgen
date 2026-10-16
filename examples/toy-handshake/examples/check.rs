//! Run the model checker and print a readable report.
//!
//!     cargo run -p toy-handshake --example check --features model
//!     cargo run -p toy-handshake --example check --features model,planted-bug
//!
//! Exits non-zero if any property fails.

use stateright::{Checker, Expectation, Model, Path};
use toy_handshake::liveness::{check_termination, Liveness};
use toy_handshake::model::{Action, InitiatorModel, ModelState};
use toy_handshake::MAX_TRIES;

fn fmt_state(s: &ModelState) -> String {
    format!(
        "{:<8} tries={}  last={:?}",
        format!("{:?}", s.control),
        s.vars.tries,
        s.last
    )
}

/// Prints `path`, stopping at the first state where `stop_at` holds.
///
/// stateright overwrites an `always` discovery each time BFS meets another
/// violating state, so the stored path can run past the first violation.
/// Cutting it at the first violation gives the shortest counterexample.
fn print_path(path: Path<ModelState, Action>, stop_at: impl Fn(&ModelState) -> bool) {
    for (i, (s, a)) in path.into_vec().into_iter().enumerate() {
        println!("        {i:>2}. {}", fmt_state(&s));
        if stop_at(&s) {
            break;
        }
        if let Some(a) = a {
            println!("              --{a:?}-->");
        }
    }
}

fn main() {
    let model = InitiatorModel;
    let props = model.properties();
    let checker = model.checker().spawn_bfs().join();

    println!("Initiator model, MAX_TRIES = {MAX_TRIES}");
    println!(
        "  {} unique states, max depth {}\n",
        checker.unique_state_count(),
        checker.max_depth()
    );

    let mut failed = 0;
    for p in &props {
        let found = checker.discovery(p.name);
        let (ok, kind) = match p.expectation {
            Expectation::Always => (found.is_none(), "always   "),
            Expectation::Sometimes => (found.is_some(), "sometimes"),
            Expectation::Eventually => (found.is_none(), "eventually"),
        };
        println!("  [{}] {kind} {}", if ok { " ok " } else { "FAIL" }, p.name);
        if !ok {
            failed += 1;
            if let Some(path) = found {
                println!("        counterexample:");
                print_path(path, |s| !(p.condition)(&model, s));
            }
        }
    }

    match check_termination(&InitiatorModel, |s| s.control.is_terminal(), 1_000_000) {
        Liveness::Ok { .. } => println!("  [ ok ] AG EF    terminal state always reachable"),
        Liveness::Stuck { path } => {
            failed += 1;
            println!("  [FAIL] AG EF    terminal state always reachable");
            println!("        stuck after:");
            for (i, s) in path.iter().enumerate() {
                println!("        {i:>2}. {}", fmt_state(s));
            }
        }
        Liveness::TooBig { limit } => {
            failed += 1;
            println!("  [FAIL] AG EF    gave up after {limit} states");
        }
    }

    println!();
    if failed == 0 {
        println!("all properties hold");
    } else {
        println!(
            "{failed} propert{} failed",
            if failed == 1 { "y" } else { "ies" }
        );
        std::process::exit(1);
    }
}
