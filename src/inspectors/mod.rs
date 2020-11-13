//! # Inspectors
//!
//! All inspectors go here. An inspector is an implementer of the `Inspector`
//! trait and is responsible for decoding a `Trace` in isolation. No sub-trace
//! specific logic needs to be written.

mod uniswap;
/// A Uniswap inspector
pub use uniswap::Uniswap;

mod curve;
/// A Curve inspector
pub use curve::Curve;

mod aave;
/// An Aave inspector
pub use aave::Aave;

mod erc20;
/// ERC20 Inspector, to be used for parsing subtraces involving transfer/transferFrom
pub use erc20::ERC20;

mod batch;
/// Takes multiple inspectors
pub use batch::BatchInspector;

mod compound;
pub use compound::Compound;

use crate::types::{
    actions::{Trade, Transfer},
    Classification,
};
use ethers::types::Address;

/// Finds the next transfer after an index in the actions array where the `from`
/// field matches the provided `address`
pub(crate) fn find_matching_transfer_after<F: Fn(&Transfer) -> bool>(
    actions: &[Classification],
    after: usize,
    check_fn: F,
) -> Option<(usize, &Transfer)> {
    let mut found_known = false;
    actions
        .iter()
        .enumerate()
        .skip(after + 1)
        .find_map(|(j, a)| {
            // Only return `Some` if this is the first known trace we encounter.
            // e.g. if it was Transfer1 -> Deposit -> Transfer2, it should return
            // None, whereas Transfer1 -> Transfer2 should return Some.
            if let Some(action) = a.to_action() {
                if let Some(t) = action.transfer() {
                    if !found_known && check_fn(t) {
                        return Some((j, t));
                    }
                }
                found_known = true;
            }
            None
        })
}

/// Finds the next trade after an index in the actions array where the
/// tokens match
pub fn find_matching_trade_after(
    actions: &[Classification],
    after: usize,
    address: Address,
) -> Option<(usize, &Trade)> {
    let mut found_known = false;
    actions
        .iter()
        .enumerate()
        .skip(after + 1)
        .find_map(|(j, a)| {
            if let Some(action) = a.to_action() {
                if let Some(t) = action.trade() {
                    if t.t2.token == address {
                        return Some((j, t));
                    }
                }
                found_known = true;
            }
            None
        })
}
