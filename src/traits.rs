use crate::model::{CallClassification, EventLog, InternalCall};
use crate::types::actions::{AddLiquidity, Deposit, Liquidation, Trade, Transfer, Withdrawal};
use crate::types::{Inspection, Protocol, TransactionData};
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
pub trait DefiProtocol {
    /// Returns all the known contracts for the protocol
    fn base_contracts(&self) -> ProtocolContracts;

    /// The identifier
    fn protocol() -> Protocol;

    /// Whether it can be determined that the address is in fact a associated with the protocol
    fn is_protocol(&self, _: &Address) -> Option<bool> {
        None
    }

    // fn is_protocol() -> bool;

    /// Checks whether this event belongs to the protocol
    fn is_protocol_event(&self, _: &EventLog) -> bool {
        todo!()
    }

    /// Checks if the internal call's target can be attributed to the protocol and whether the call
    /// can be classified.
    ///
    /// This only intends to classify the call as stand alone and without taking any context into
    /// account.
    fn classify_call(&self, call: &InternalCall) -> Option<CallClassification>;

    /// How decode an input function blob
    fn decode_add_liquidity(&self, _: &InternalCall) -> Option<AddLiquidity> {
        None
    }

    fn decode_liquidation(&self, _: &InternalCall) -> Option<Liquidation> {
        None
    }

    fn decode_transfer(&self, _: &InternalCall) -> Option<Transfer> {
        None
    }

    fn decode_deposit(&self, _: &InternalCall) -> Option<Deposit> {
        None
    }

    fn decode_withdrawal(&self, _: &InternalCall) -> Option<Withdrawal> {
        None
    }

    /// TODO this should &InternalCall (first swap) and [InternalCall] or iter to find the reverse
    fn decode_swap(&self, _: &[InternalCall]) -> Option<Trade> {
        None
    }

    fn find_trades(&self, _: &mut TransactionData) {}

    fn find_arbitrages(&self, _: &mut TransactionData) {}

    fn find_liquidation(&self, _: &mut TransactionData) {}
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
