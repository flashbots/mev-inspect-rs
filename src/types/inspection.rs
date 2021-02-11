use crate::{
    addresses::{DYDX, FILTER, ZEROX},
    types::{
        classification::{ActionTrace, CallTrace},
        Classification, Protocol, Status,
    },
};
use ethers::types::{Action, Address, CallType, Trace, TxHash};
use std::{collections::HashSet, convert::TryFrom};

#[derive(Debug, Clone)]
/// The result of an inspection of a trace along with its inspected subtraces
pub struct Inspection {
    /// Success / failure
    pub status: Status,

    //////  What
    /// All the classified / unclassified actions that happened
    pub actions: Vec<Classification>,

    ///// Where
    /// All the involved protocols
    pub protocols: HashSet<Protocol>,

    // Who
    /// The sender of the transaction
    pub from: Address,
    /// The first receiver of this tx, the contract being interacted with. In case
    /// of sophisticated bots, this will be the bot's contract logic.
    pub contract: Address,
    /// If this is set, then the `contract` was a proxy and the actual logic is
    /// in this address
    pub proxy_impl: Option<Address>,

    //////  When
    /// The trace's tx hash
    pub hash: TxHash,

    /// The block number of this tx
    pub block_number: u64,
}

impl Inspection {
    // TODO: Is there a better way to do this without re-allocating?
    // Maybe this? https://doc.rust-lang.org/std/vec/struct.DrainFilter.html
    pub fn prune(&mut self) {
        self.actions = self
            .actions
            .iter()
            .filter(|action| match action {
                // Remove any of the pruned calls
                Classification::Prune => false,
                // Remove calls with 2300 gas as they are probably due to
                // the gas stipend for low level calls, which we've already
                // taken into account.
                Classification::Unknown(call) => call.as_ref().gas != 2300.into(),
                Classification::Known(_) => true,
            })
            .cloned()
            .collect();
    }

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

/// Helper type to bypass https://github.com/rust-lang/rust/issues/50133#issuecomment-646908391
pub(crate) struct TraceWrapper<T>(pub(crate) T);
impl<T: IntoIterator<Item = Trace>> TryFrom<TraceWrapper<T>> for Inspection {
    type Error = ();

    fn try_from(traces: TraceWrapper<T>) -> Result<Self, Self::Error> {
        let mut traces = traces.0.into_iter().peekable();

        // get the first trace
        let trace = match traces.peek() {
            Some(inner) => inner,
            None => return Err(()),
        };
        let call = match trace.action {
            Action::Call(ref call) => call,
            // the first action we care about must be a call. everything else
            // is junk
            _ => return Err(()),
        };

        // Filter out unwanted calls
        if FILTER.get(&call.from).is_some() || FILTER.get(&call.to).is_some() {
            return Err(());
        }

        let mut inspection = Inspection {
            status: Status::Success,
            // all unclassified calls
            actions: Vec::new(),
            // start off with empty protocols since everything is unclassified
            protocols: HashSet::new(),
            from: call.from,
            contract: call.to,
            proxy_impl: None,
            hash: trace.transaction_hash.unwrap_or_else(TxHash::zero),
            block_number: trace.block_number,
        };

        inspection.actions = traces
            .into_iter()
            .filter_map(|trace| {
                // Revert if all subtraces revert? There are counterexamples
                // e.g. when a low-level trace's revert is handled
                if trace.error.is_some() {
                    inspection.status = Status::Reverted;
                }

                match trace.action {
                    Action::Call(call) => {
                        if inspection.proxy_impl.is_none()
                            && call.call_type == CallType::DelegateCall
                            && call.from == inspection.contract
                        {
                            inspection.proxy_impl = Some(call.to);
                        }

                        if call.to == *DYDX {
                            inspection.protocols.insert(Protocol::DyDx);
                        }

                        if call.to == *ZEROX {
                            inspection.protocols.insert(Protocol::ZeroEx);
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

        Ok(inspection)
    }
}
