//! All the datatypes associated with MEV-Inspect
pub mod actions;

mod evaluation;
pub use evaluation::Evaluation;

mod classification;
pub use classification::Classification;

mod inspection;
pub use inspection::Inspection;

#[derive(Debug, Clone, PartialOrd, PartialEq)]
pub enum Status {
    // Reverted(String),
    Reverted,
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
