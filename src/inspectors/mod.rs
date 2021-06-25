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

mod balancer;
/// A Balancer inspector
pub use balancer::Balancer;

mod aave;
/// An Aave inspector
pub use aave::Aave;

pub(crate) mod erc20;
/// ERC20 Inspector, to be used for parsing subtraces involving transfer/transferFrom
pub use erc20::ERC20;

mod batch;
/// Takes multiple inspectors
pub use batch::{BatchEvaluationError, BatchInspector};

mod compound;
pub use compound::Compound;

mod zeroex;
pub use zeroex::ZeroEx;

use crate::types::{actions::SpecificAction, Classification};

/// Given an iterator over index,Classification tuples, it will try to cast
/// each classification to the given specific action (depending on the function given
/// to `cast`), and then it will check if it satisfies a condition. If yes, it returns
/// that classification and its index. If `check_all` is set to false, it will return
/// None if the action we're looking for is not the first known action, e.g.
/// given a [Deposit, Transfer, Trade], if we're looking for a Trade, `check_all` must
/// be set to true, otherwise once the Transfer is hit, it will return None
pub(crate) fn find_matching<'a, I, T, F1, F2>(
    mut actions: I,
    cast: F1,
    check_fn: F2,
    check_all: bool,
) -> Option<(usize, &'a T)>
where
    I: Iterator<Item = (usize, &'a Classification)>,
    F1: Fn(&SpecificAction) -> Option<&T>,
    F2: Fn(&T) -> bool,
{
    let mut found_known = false;
    actions.find_map(|(j, a)| {
        if check_all || !found_known {
            if let Some(action) = a.as_action() {
                if let Some(t) = cast(&action) {
                    if check_fn(t) {
                        return Some((j, t));
                    }
                }
                found_known = true;
            }
        }
        None
    })
}
