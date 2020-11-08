use ethers::types::{Action, Address, CallType, Trace, TxHash};

mod actions;
pub use actions::*;

#[derive(Debug, Clone, PartialEq)]
/// The result of an inspection of a trace along with its inspected subtraces
pub struct Inspection {
    pub actions: Vec<Classification>,
    /// The trace's tx hash
    pub hash: TxHash,
    /// The sender of this trace
    pub from: Address,
    /// Success / failure
    pub status: Status,

    pub protocols: Vec<Protocol>,
}

impl Inspection {
    /// Returns: types of protocols, types of actions (arb, liq), bot addresses and profit
    /// Bots that perform liq/arbs maybe for a profit that are nto int he addressbook should be
    /// added
    pub fn summary(&self) {}

    /// Returns all the successfully classified calls in this Inspection
    pub fn known(&self) -> Vec<ActionTrace> {
        self.actions
            .iter()
            .filter_map(|classification| match classification {
                Classification::Known(inner) => Some(inner),
                Classification::Unknown(_) | Classification::Prune => None,
            })
            .cloned()
            .collect()
    }

    /// Returns all the unsuccessfully classified calls in this Inspection
    pub fn unknown(&self) -> Vec<CallTrace> {
        self.actions
            .iter()
            .filter_map(|classification| match classification {
                Classification::Unknown(inner) => Some(inner),
                Classification::Known(_) | Classification::Prune => None,
            })
            .cloned()
            .collect()
    }
}

impl<T: IntoIterator<Item = Trace>> From<T> for Inspection {
    fn from(traces: T) -> Self {
        let mut from = None;
        let mut hash = None;
        let mut status = None;
        let actions: Vec<Classification> = traces
            .into_iter()
            .filter_map(|trace| {
                // Revert if all subtraces revert? There are counterexamples
                // e.g. when a low-level trace's revert is handled
                if status.is_none() && trace.error.is_some() {
                    status = Some(Status::Reverted);
                }

                match trace.action {
                    Action::Call(call) => {
                        // The first call is the msg.sender
                        if from.is_none() {
                            from = Some(call.from)
                        }

                        // The first hash is the tx hash
                        if hash.is_none() {
                            hash = trace.transaction_hash;
                        }

                        if call.call_type == CallType::StaticCall
                            || call.call_type == CallType::DelegateCall
                        {
                            return None;
                        }

                        Some(
                            CallTrace {
                                call,
                                trace_address: trace.trace_address,
                            }
                            .into(),
                        )
                    }
                    _ => None,
                }
            })
            .collect();

        Inspection {
            hash: hash.expect("tx hash should be set"),
            actions,
            from: from.expect("msg.sender should be set"),
            status: status.unwrap_or(Status::Success),

            protocols: Vec::new(),
        }
    }
}
