//! Phase 0 spike: the output fsmgen should generate for a toy request/response
//! handshake, written by hand.
//!
//! Generated in the real tool (hand-written here):
//! - [`initiator`]: runtime enums, `Vars`, `Hooks` trait, `Machine::step`
//! - [`model`]: stateright model and built-in properties
//! - `initiator.dot`: diagram
//!
//! Hand-written in the real tool too:
//! - [`types`]: state, event, and output data
//! - [`hooks`]: the `Hooks` impl
//! - [`props`]: protocol-specific properties
//!
//! Support code that would live in `fsmgen-model`:
//! - [`liveness`]: termination check

#![forbid(unsafe_code)]

pub mod hooks;
pub mod initiator;
#[cfg(feature = "model")]
pub mod liveness;
#[cfg(feature = "model")]
pub mod model;
#[cfg(feature = "model")]
pub mod props;
pub mod types;

/// Resends before giving up. Referenced by the table's var guards.
pub const MAX_TRIES: u8 = 3;
