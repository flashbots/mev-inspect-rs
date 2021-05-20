use crate::model::{CallClassification, EventLog, InternalCall};
use crate::types::actions::SpecificAction;
use crate::types::{Action, Inspection, Protocol, TransactionData};
use ethers::prelude::BaseContract;
use ethers::types::Address;
use std::borrow::Cow;

pub trait Reducer {
    /// By default the reducer is empty. A consumer may optionally
    /// implement this method to perform additional actions on the classified &
    /// filtered results.
    fn reduce(&self, _: &mut Inspection);
}

/// Trait for defining an inspector for a specific DeFi protocol
pub trait Inspector: core::fmt::Debug {
    /// Classifies an inspection's actions
    fn inspect(&self, inspection: &mut Inspection);
}

/// Trait for a general protocol
///
/// TODO use classify(call) to indicate what kind of analytics should be executed on `tx`
pub trait DefiProtocol {
    /// Returns all the known contracts for the protocol
    fn base_contracts(&self) -> ProtocolContracts;

    /// The identifier
    fn protocol() -> Protocol;

    /// Whether it can be determined that the address is in fact a associated with the protocol
    fn is_protocol(&self, _: &Address) -> Option<bool> {
        None
    }

    /// Checks whether this event belongs to the protocol
    fn is_protocol_event(&self, _: &EventLog) -> bool {
        false
    }

    /// Decode the specific action the given call represents
    fn decode_call_action(&self, call: &InternalCall, tx: &TransactionData) -> Option<Action>;

    /// This will attempt to classify the call.
    ///
    /// Should return the classification of the call and the action if it is
    /// possible to decode it using only the call's input arguments.
    fn classify(&self, call: &InternalCall)
        -> Option<(CallClassification, Option<SpecificAction>)>;

    /// Inspects the transaction and classifies all internal calls
    ///
    /// This will first `classify` each call, if no action was derived from the standalone call,
    /// the action will be tried to be decoded with `decode_call_action`
    fn inspect_tx(&self, tx: &mut TransactionData) {
        // iterate over all calls that are not processed yet
        let mut actions = Vec::new();
        let mut decode_again = Vec::new();
        for call in tx.calls_mut() {
            // if a protocol can not be identified by an address, inspect it regardless
            if self.is_protocol(&call.to).unwrap_or(true) {
                if let Some((classification, action)) = self.classify(call) {
                    call.protocol = Some(Self::protocol());
                    // mark this call
                    call.classification = classification;

                    if let Some(action) = action {
                        actions.push(Action::new(action, call.trace_address.clone()));
                    } else {
                        decode_again.push(call.trace_address.clone());
                    }
                }
            }
        }

        for call in decode_again {
            if let Some(call) = tx.get_call(&call) {
                if let Some(action) = self.decode_call_action(call, tx) {
                    actions.push(action);
                }
            }
        }

        tx.extend_actions(actions.into_iter());
    }
}

/// A wrapper for `Protocol`'s contracts with helper functions
pub enum ProtocolContracts<'a> {
    None,
    /// Only one contract know, (ERC20)
    Single(&'a BaseContract),
    /// Represents a `Protocol` with two known contract types (`Uniswap`)
    Dual(&'a BaseContract, &'a BaseContract),
    /// The `Protocol` has
    Multi(Vec<Cow<'a, BaseContract>>),
}

impl<'a> ProtocolContracts<'a> {
    /// Returns an iterator over all the protocol's contracts
    pub fn iter(&self) -> Box<dyn Iterator<Item = &BaseContract> + '_> {
        match self {
            ProtocolContracts::None => {
                Box::new(std::iter::empty()) as Box<dyn Iterator<Item = &BaseContract> + '_>
            }
            ProtocolContracts::Single(c) => Box::new(std::iter::once(*c)),
            ProtocolContracts::Dual(c1, c2) => {
                Box::new(std::iter::once(*c1).chain(std::iter::once(*c2)))
            }
            ProtocolContracts::Multi(c) => Box::new(c.iter().map(Cow::as_ref)),
        }
    }
}
