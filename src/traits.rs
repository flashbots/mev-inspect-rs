use crate::model::{CallClassification, EventLog, InternalCall};
use crate::types::actions::SpecificAction;
use crate::types::{Action, Inspection, Protocol, TransactionData};
use ethers::prelude::BaseContract;
use std::borrow::Cow;

pub trait Reducer {
    /// By default the reducer is empty. A consumer may optionally
    /// implement this method to perform additional actions on the classified &
    /// filtered results.
    fn reduce(&self, _: &mut Inspection);
}

pub trait TxReducer {
    /// By default the reducer is empty. A consumer may optionally
    /// implement this method to perform additional actions on the classified &
    /// filtered results.
    fn reduce_tx(&self, _: &mut TransactionData);
}

/// Trait for defining an inspector for a specific DeFi protocol
pub trait Inspector: core::fmt::Debug {
    /// Classifies an inspection's actions
    fn inspect(&self, inspection: &mut Inspection);
}

/// Trait for a general defi protocol
pub trait DefiProtocol {
    /// Returns all the known contracts for the protocol
    fn base_contracts(&self) -> ProtocolContracts;

    /// The general protocol identifier
    fn protocol(&self) -> Protocol;

    /// Whether it can be determined that the address is in fact associated with the protocol.
    ///
    /// Since this is non deterministic, it will return
    /// - Some(Some(proto)) when the proto could be determined
    /// - Some(None) when it cannot be ruled out that this call is in fact associated with the protocol
    /// - None when it can be ruled out
    fn is_protocol(&self, _: &InternalCall) -> Option<Option<Protocol>> {
        Some(None)
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
        inspect_tx(self, tx)
    }
}

pub(crate) fn inspect_tx<T: DefiProtocol + ?Sized>(proto: &T, tx: &mut TransactionData) {
    // iterate over all calls that are not processed yet
    let mut actions = Vec::new();
    let mut decode_again = Vec::new();
    for call in tx.calls_mut() {
        // if a protocol can not be identified by an address, inspect it regardless
        if let Some(p) = proto
            .is_protocol(&call)
            .map(|maybe_proto| maybe_proto.unwrap_or_else(|| proto.protocol()))
        {
            if let Some((classification, action)) = proto.classify(call) {
                call.protocol = Some(p);
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

    tx.extend_actions(actions.into_iter());

    for call in decode_again {
        if let Some(call) = tx.get_call(&call) {
            if let Some(action) = proto.decode_call_action(call, tx) {
                tx.push_action(action)
            }
        }
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
