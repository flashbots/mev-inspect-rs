use crate::types::{
    classification::{ActionTrace, CallTrace},
    Classification, Protocol, Status,
};
use ethers::types::{Action, Address, CallType, Trace, TxHash};

#[derive(Debug, Clone, PartialEq)]
/// The result of an inspection of a trace along with its inspected subtraces
pub struct Inspection {
    /// Success / failure
    pub status: Status,

    //////  What
    /// All the classified / unclassified actions that happened
    pub actions: Vec<Classification>,

    ///// Where
    /// All the involved protocols
    // TODO: Should we tie this in each classified action?
    pub protocols: Vec<Protocol>,

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

impl<T: IntoIterator<Item = Trace>> From<T> for Inspection {
    fn from(traces: T) -> Self {
        // TODO: Can we get the first element in a better way?
        let mut contract = None;
        let mut from = None;
        let mut hash = None;
        let mut status = None;
        let mut proxy_impl = None;
        let mut block_number = None;

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

                        // The first receiver is the contract being used.
                        // This will either be the bot contract or the
                        if contract.is_none() {
                            contract = Some(call.to)
                        }

                        if proxy_impl.is_none()
                            && call.call_type == CallType::DelegateCall
                            && Some(call.from) == contract
                        {
                            proxy_impl = Some(call.to);
                        }

                        // The first hash is the tx hash
                        if hash.is_none() {
                            hash = trace.transaction_hash;
                        }

                        // Set the block number
                        if block_number.is_none() {
                            block_number = Some(trace.block_number);
                        }

                        if call.call_type == CallType::StaticCall
                        // || call.call_type == CallType::DelegateCall
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
            // assume success if no revert was found
            status: status.unwrap_or(Status::Success),
            // all unclassified calls
            actions,
            // start off with empty protocols since everything is unclassified
            protocols: Vec::new(),

            // set the data from the first call
            from: from.expect("msg.sender should be set"),
            contract: contract.expect("first receiver should be the bot contract"),
            proxy_impl,
            hash: hash.expect("tx hash should be set"),
            block_number: block_number.expect("block number should be set"),
        }
    }
}
