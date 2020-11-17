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

mod erc20;
/// ERC20 Inspector, to be used for parsing subtraces involving transfer/transferFrom
pub use erc20::ERC20;

mod batch;
/// Takes multiple inspectors
pub use batch::BatchInspector;

mod compound;
pub use compound::Compound;

use crate::types::{actions::SpecificAction, Classification};

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
            if let Some(action) = a.to_action() {
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
