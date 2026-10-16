//! Termination check. In the real tool this belongs in `fsmgen-model`; it is
//! not specific to this machine.
//!
//! stateright's `Property::eventually` misses counterexamples that end in a
//! cycle (see its docs). Every machine with a retry or discard self-loop has
//! such cycles, so it would report "terminates" for machines that can livelock.
//!
//! Instead we check that a terminal state stays reachable from every reachable
//! state (`AG EF terminal`). On a finite graph, that means every run reaches a
//! terminal state under strong fairness: a step that is enabled infinitely
//! often is eventually taken.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;

use stateright::Model;

#[derive(Debug)]
pub enum Liveness<S> {
    /// Every reachable state can still reach a terminal state.
    Ok { states: usize },
    /// `path` (from an init state) ends in a state from which no terminal
    /// state is reachable.
    Stuck { path: Vec<S> },
    /// Exploration stopped at `limit` states.
    TooBig { limit: usize },
}

/// `is_terminal` says which states count as terminal. States with no
/// successors are dead ends and do not count unless `is_terminal` says so.
pub fn check_termination<M>(
    model: &M,
    is_terminal: fn(&M::State) -> bool,
    limit: usize,
) -> Liveness<M::State>
where
    M: Model,
    M::State: Clone + Eq + Hash,
{
    // Forward BFS: reachable states, parent links, reverse edges.
    let mut parent: HashMap<M::State, Option<M::State>> = HashMap::new();
    let mut preds: HashMap<M::State, Vec<M::State>> = HashMap::new();
    let mut queue = VecDeque::new();
    for s in model.init_states() {
        if !parent.contains_key(&s) {
            parent.insert(s.clone(), None);
            queue.push_back(s);
        }
    }
    while let Some(s) = queue.pop_front() {
        if parent.len() > limit {
            return Liveness::TooBig { limit };
        }
        for t in model.next_states(&s) {
            preds.entry(t.clone()).or_default().push(s.clone());
            if !parent.contains_key(&t) {
                parent.insert(t.clone(), Some(s.clone()));
                queue.push_back(t);
            }
        }
    }

    // Backward BFS from terminal states.
    let mut can_finish: HashSet<M::State> = HashSet::new();
    let mut queue: VecDeque<M::State> = parent.keys().filter(|s| is_terminal(s)).cloned().collect();
    can_finish.extend(queue.iter().cloned());
    while let Some(t) = queue.pop_front() {
        for s in preds.get(&t).into_iter().flatten() {
            if can_finish.insert(s.clone()) {
                queue.push_back(s.clone());
            }
        }
    }

    // Report the shallowest stuck state, so the path is short.
    let depth = |s: &M::State| {
        let mut n = 0;
        let mut cur = parent[s].clone();
        while let Some(p) = cur {
            n += 1;
            cur = parent[&p].clone();
        }
        n
    };
    match parent
        .keys()
        .filter(|s| !can_finish.contains(*s))
        .min_by_key(|s| depth(s))
    {
        None => Liveness::Ok {
            states: parent.len(),
        },
        Some(stuck) => {
            let mut path = vec![stuck.clone()];
            let mut cur = parent[stuck].clone();
            while let Some(p) = cur {
                cur = parent[&p].clone();
                path.push(p);
            }
            path.reverse();
            Liveness::Stuck { path }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 0 -> 1 -> 2 (terminal), and 1 <-> 3 with 3 also able to fall into the
    /// trap 4 <-> 5, which can never reach 2.
    struct Graph;

    impl Model for Graph {
        type State = u8;
        type Action = u8;
        fn init_states(&self) -> Vec<u8> {
            vec![0]
        }
        fn actions(&self, s: &u8, out: &mut Vec<u8>) {
            out.extend(match s {
                0 => &[1][..],
                1 => &[2, 3],
                3 => &[1, 4],
                4 => &[5],
                5 => &[4],
                _ => &[],
            });
        }
        fn next_state(&self, _: &u8, a: u8) -> Option<u8> {
            Some(a)
        }
    }

    #[test]
    fn finds_livelock() {
        match check_termination(&Graph, |s| *s == 2, 100) {
            Liveness::Stuck { path } => assert_eq!(path, vec![0, 1, 3, 4]),
            r => panic!("{r:?}"),
        }
    }

    #[test]
    fn cycle_that_can_exit_is_fine() {
        // Same graph without the trap: 1 <-> 3 is a cycle, but 2 stays reachable.
        struct NoTrap;
        impl Model for NoTrap {
            type State = u8;
            type Action = u8;
            fn init_states(&self) -> Vec<u8> {
                vec![0]
            }
            fn actions(&self, s: &u8, out: &mut Vec<u8>) {
                out.extend(match s {
                    0 => &[1][..],
                    1 => &[2, 3],
                    3 => &[1],
                    _ => &[],
                });
            }
            fn next_state(&self, _: &u8, a: u8) -> Option<u8> {
                Some(a)
            }
        }
        assert!(matches!(
            check_termination(&NoTrap, |s| *s == 2, 100),
            Liveness::Ok { states: 4 }
        ));
    }
}
