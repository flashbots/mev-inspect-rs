//! All the datatypes associated with MEV-Inspect
pub mod actions;

pub mod evaluation;
pub use evaluation::{EvalError, Evaluation};

pub(crate) mod classification;
pub use classification::Classification;

pub(crate) mod inspection;
pub use inspection::Inspection;

#[derive(Debug, Clone, PartialOrd, PartialEq)]
pub enum Status {
    /// When a transaction reverts without touching any DeFi protocol
    Reverted,
    /// When a transaction reverts early but it had touched a DeFi protocol
    Checked,
    /// When a transaction suceeds
    Success,
}

#[derive(Debug, Clone, Copy, PartialOrd, PartialEq, Eq, Ord, Hash)]
/// The supported protocols
pub enum Protocol {
    // Uniswap & Forks
    UniswapV1,
    Uniswap,
    Uniswappy,
    Sushiswap,
    SakeSwap,

    // Other AMMs
    Curve,
    Balancer,

    // Lending / Liquidations
    Aave,
    Compound,

    // Aggregators
    ZeroEx,

    // Misc.
    Flashloan,
    DyDx,
}
