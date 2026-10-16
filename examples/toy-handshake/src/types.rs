//! Hand-written: the data carried by states, events, and outputs.
//!
//! None of this is visible to the model checker. The generator only needs the
//! type *names* (from the `states:`/`events:`/`out:` sections of the table).

/// Request sent by the initiator. Kept in [`Waiting`] so it can be resent verbatim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Req {
    pub nonce: u64,
}

/// Response from the peer. Valid only if it echoes our nonce.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resp {
    pub nonce: u64,
    pub cookie: u64,
}

/// Data for `State::Waiting`.
#[derive(Debug, PartialEq, Eq)]
pub struct Waiting {
    pub req: Req,
}

/// Data for `State::Done`.
#[derive(Debug, PartialEq, Eq)]
pub struct Done {
    pub session: u64,
}

/// Everything `step()` can ask the driver to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InitiatorOut {
    Send(Req),
    Established { session: u64 },
    Failed,
}
