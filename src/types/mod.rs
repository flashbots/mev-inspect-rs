//! All the datatypes associated with MEV-Inspect
pub mod actions;

pub mod evaluation;
pub use evaluation::Evaluation;

pub(crate) mod classification;
pub use classification::Classification;

mod inspection;
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

#[derive(Debug, Clone, Copy, PartialOrd, PartialEq, Eq, Ord)]
/// The supported protocols
pub enum Protocol {
    Uniswap,
    Sushiswap,
    UniswapClone,
    Aave,
    KnownBot,
    Flashloan,
}
