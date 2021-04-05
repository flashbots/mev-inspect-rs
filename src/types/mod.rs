//! All the datatypes associated with MEV-Inspect
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

use ethers::types::{Address, U256};

pub use classification::Classification;
pub use evaluation::{EvalError, Evaluation};
pub use inspection::Inspection;

use crate::is_subtrace;
use crate::model::{EventLog, InternalCall};
use std::collections::BTreeMap;

pub mod actions;

pub(crate) mod classification;
pub mod evaluation;
pub(crate) mod inspection;

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

impl Protocol {
    pub fn is_uniswap(&self) -> bool {
        match self {
            Protocol::UniswapV1 | Protocol::Uniswap | Protocol::Uniswappy => true,
            _ => false,
        }
    }

    pub fn is_sake_swap(&self) -> bool {
        matches!(self, Protocol::SakeSwap)
    }

    pub fn is_sushi_swap(&self) -> bool {
        matches!(self, Protocol::Sushiswap)
    }

    pub fn is_curve(&self) -> bool {
        matches!(self, Protocol::Curve)
    }

    pub fn is_aave(&self) -> bool {
        matches!(self, Protocol::Aave)
    }

    pub fn is_compound(&self) -> bool {
        matches!(self, Protocol::Compound)
    }

    pub fn is_balancer(&self) -> bool {
        matches!(self, Protocol::Balancer)
    }

    pub fn is_zerox(&self) -> bool {
        matches!(self, Protocol::ZeroEx)
    }
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

#[derive(Debug, Clone, Copy)]
enum InspectionState {
    Resolved,
    UnResolved,
}

impl InspectionState {
    fn is_unresolved(&self) -> bool {
        matches!(self, InspectionState::UnResolved)
    }
    fn is_resolved(&self) -> bool {
        !self.is_unresolved()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Data<'a, T> {
    inner: &'a T,
    index: usize,
}

impl<'a, T> Deref for Data<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.inner
    }
}

/// to detect trades: all Internal calls
#[derive(Debug, Clone)]
pub struct TransactionData<'a> {
    pub from: Address,
    pub contract: Address,
    // TODO wrap this in an Refcell to simply call .resolve()?
    logs: BTreeMap<U256, (&'a EventLog, InspectionState)>,
    calls: BTreeMap<Vec<usize>, (&'a InternalCall, InspectionState)>,
    classifications: Vec<Classification>,
}

impl<'a> TransactionData<'a> {
    /// All the logs that are not resolved yet
    pub fn logs(&'a self) -> impl Iterator<Item = &'a EventLog> {
        self.logs
            .values()
            .filter_map(|(event, state)| state.is_unresolved().then(|| *event))
    }

    /// All the calls that are not resolved yet
    pub fn calls(&'a self) -> impl Iterator<Item = &'a InternalCall> {
        self.calls
            .values()
            .filter_map(|(call, state)| state.is_unresolved().then(|| *call))
    }

    /// All logs that occurred after the log
    pub fn logs_after(&'a self, log: &'a EventLog) -> impl Iterator<Item = &'a EventLog> {
        self.logs().filter(move |c| c.log_index > log.log_index)
    }

    /// All logs prior to log
    pub fn logs_prior(&'a self, log: &'a EventLog) -> impl Iterator<Item = &'a EventLog> {
        self.logs().filter(move |c| c.log_index < log.log_index)
    }

    /// Iterate over all the subcalls
    pub fn subcalls(&'a self, call: &'a InternalCall) -> impl Iterator<Item = &'a InternalCall> {
        self.calls().filter(move |c| {
            let t1 = &call.trace_address;
            let t2 = &c.trace_address;
            if t2 == t1 {
                false
            } else {
                is_subtrace(t1, t2)
            }
        })
    }

    /// Mark the call as resolved so that it can't be classified anymore
    pub fn resolve_call(&mut self, call: &'a InternalCall) {
        if let Some((_, state)) = self.calls.get_mut(&call.trace_address) {
            *state = InspectionState::Resolved
        }
    }

    pub fn resolve_calls(&mut self, calls: impl IntoIterator<Item = &'a InternalCall>) {
        for call in calls {
            self.resolve_call(call)
        }
    }

    pub fn resolve_log(&mut self, logs: &'a EventLog) {
        if let Some((_, state)) = self.logs.get_mut(&logs.log_index) {
            *state = InspectionState::Resolved
        }
    }

    pub fn resolve_logs(&mut self, logs: impl IntoIterator<Item = &'a EventLog>) {
        for log in logs {
            self.resolve_log(log)
        }
    }

    pub fn push_classification(&mut self, c: Classification) {
        self.classifications.push(c)
    }
}
