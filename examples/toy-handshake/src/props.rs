//! Hand-written: protocol-specific properties, added to the generated model's
//! built-in ones.

use stateright::Property;

use crate::initiator::{Control, Last, Row};
use crate::model::{InitiatorModel, ModelState};
use crate::MAX_TRIES;

pub fn user_properties() -> Vec<Property<InitiatorModel>> {
    vec![
        // Done is entered only via `[resp_valid]` (r4).
        Property::always("Done only via resp_valid", |_, s: &ModelState| {
            s.control != Control::Done || s.last == Last::Row(Row::WaitingRespAccept)
        }),
        // We give up after exactly MAX_TRIES resends, not sooner or later.
        Property::always(
            "Failed after exactly MAX_TRIES resends",
            |_, s: &ModelState| s.control != Control::Failed || s.vars.tries == MAX_TRIES,
        ),
    ]
}
