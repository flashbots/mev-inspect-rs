//! All the datatypes associated with MEV-Inspect
pub mod actions;

pub mod evaluation;
pub use evaluation::{EvalError, Evaluation};

pub(crate) mod classification;
pub use classification::Classification;

pub(crate) mod inspection;
pub use inspection::Inspection;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialOrd, PartialEq)]
pub enum Status {
    /// When a transaction reverts without touching any DeFi protocol
    Reverted,
    /// When a transaction reverts early but it had touched a DeFi protocol
    Checked,
    /// When a transaction succeeds
    Success,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl FromStr for Status {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "reverted" | "Reverted" => Ok(Status::Reverted),
            "checked" | "Checked" => Ok(Status::Checked),
            "success" | "Success" => Ok(Status::Success),
            s => Err(format!("`{}` is nat a valid status", s)),
        }
    }
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

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", format!("{:?}", self).to_lowercase())
    }
}

impl FromStr for Protocol {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "uniswapv1" => Ok(Protocol::UniswapV1),
            "uniswap" => Ok(Protocol::Uniswap),
            "uniswappy" => Ok(Protocol::Uniswappy),
            "sushiswap" => Ok(Protocol::Sushiswap),
            "sakeswap" => Ok(Protocol::SakeSwap),
            "curve" => Ok(Protocol::Curve),
            "balancer" => Ok(Protocol::Balancer),
            "aave" => Ok(Protocol::Aave),
            "compound" => Ok(Protocol::Compound),
            "zeroex" => Ok(Protocol::ZeroEx),
            "flashloan" => Ok(Protocol::Flashloan),
            "dydx" => Ok(Protocol::DyDx),
            s => Err(format!("`{}` is nat a valid protocol", s)),
        }
    }
}
