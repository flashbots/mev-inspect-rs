//! # Inspectors
//!
//! All inspectors go here. An inspector is an implementer of the `Inspector`
//! trait and is responsible for decoding a `Trace` in isolation. No sub-trace
//! specific logic needs to be written.

mod uniswap;
/// A Uniswap inspector
pub use uniswap::Uniswap;

mod aave;
/// An Aave inspector
pub use aave::Aave;

mod arb;
/// Combines trades to arbitrages
pub use arb::ArbitrageReducer;

mod erc20;
/// ERC20 Inspector, to be used for parsing subtraces involving transfer/transferFrom
pub use erc20::ERC20;

mod batch;
/// Takes multiple inspectors
pub use batch::BatchInspector;
