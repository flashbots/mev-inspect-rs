//! All the datatypes associated with MEV-Inspect
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::str::FromStr;

use ethers::types::{Address, TxHash, U256};

pub use classification::Classification;
pub use evaluation::{EvalError, Evaluation};
pub use inspection::Inspection;

use crate::is_subtrace;
use crate::model::{EventLog, InternalCall};
use crate::types::actions::SpecificAction;
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
    UniswapV2,
    UniswapV3,
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
            Protocol::UniswapV1
            | Protocol::UniswapV2
            | Protocol::UniswapV3
            | Protocol::Uniswappy => true,
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
            "uniswapv2" => Ok(Protocol::UniswapV2),
            "uniswapv3" => Ok(Protocol::UniswapV3),
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

/// Type alias for trace address of an internal
pub type CallTraceAddress = Vec<usize>;

/// An `EventLog` that can be assigned to a call
#[derive(Debug, Clone)]
pub struct TransactionLog {
    pub inner: EventLog,
    /// The trace of the call this event is assigned to
    assigned_to_call: Option<CallTraceAddress>,
}

impl TransactionLog {
    /// Assign this log to the call identified by the given trace address
    pub fn assign_to(&mut self, trace_address: CallTraceAddress) -> Option<CallTraceAddress> {
        self.assigned_to_call.replace(trace_address)
    }

    /// Remove the trace address of the assigned call, if there is one
    pub fn un_assign(&mut self) -> Option<CallTraceAddress> {
        self.assigned_to_call.take()
    }

    /// Whether this event is assigned to a call
    pub fn is_assigned(&self) -> bool {
        self.assigned_to_call.is_some()
    }

    /// Returns the trace address of the call this event is assigned to
    pub fn assigned_call(&self) -> Option<&CallTraceAddress> {
        self.assigned_to_call.as_ref()
    }
}

impl AsRef<U256> for TransactionLog {
    fn as_ref(&self) -> &U256 {
        &self.log_index
    }
}

impl Deref for TransactionLog {
    type Target = EventLog;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for TransactionLog {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
/// Represents an identified action, initiated by the `call` and the `logs` involved
#[derive(Debug, Clone)]
pub struct Action {
    /// The actual action
    pub inner: SpecificAction,
    /// The call responsible for this action
    pub call: CallTraceAddress,
    /// The log indices of the logs used
    pub logs: Vec<U256>,
}

impl Action {
    pub fn new(inner: SpecificAction, call: CallTraceAddress) -> Self {
        Self::with_logs(inner, call, Vec::new())
    }

    pub fn with_logs(inner: SpecificAction, call: CallTraceAddress, logs: Vec<U256>) -> Self {
        Self { inner, call, logs }
    }
}

/// To detect trades: all Internal calls
// TODO drop `Inspection` in favor of this model?
#[derive(Debug, Clone)]
pub struct TransactionData {
    /// Success / failure
    pub status: Status,

    // Who
    /// The sender of the transaction
    pub from: Address,

    /// The first receiver of this tx, the contract being interacted with. In case
    /// of sophisticated bots, this will be the bot's contract logic.
    pub contract: Address,

    ///// The protocol of the `contract`
    pub protocol: Option<Protocol>,

    /// If this is set, then the `contract` was a proxy and the actual logic is
    /// in this address
    pub proxy_impl: Option<Address>,

    //////  When
    /// The trace's tx hash
    pub hash: TxHash,

    /// The block number of this tx
    pub block_number: u64,

    /// Transaction position
    pub transaction_position: usize,

    /// log_index  -> Log
    logs: BTreeMap<U256, TransactionLog>,
    /// All internal calls sorted by trace
    calls: BTreeMap<CallTraceAddress, InternalCall>,
    /// actions identified in this transaction
    actions: Vec<Action>,
}

impl TransactionData {
    pub fn new(inspection: &Inspection) -> Self {
        todo!()
    }

    pub fn get_call(&self, trace_address: &CallTraceAddress) -> Option<&InternalCall> {
        self.calls.get(trace_address)
    }

    /// All the logs that are not assigned to a call yet
    pub fn logs(&self) -> impl Iterator<Item = &TransactionLog> {
        self.logs.values().filter(|log| !log.is_assigned())
    }

    /// All the logs that are assigned to a call
    pub fn assigned_logs(&self) -> impl Iterator<Item = (&CallTraceAddress, &EventLog)> {
        self.logs
            .values()
            .filter_map(|log| log.assigned_call().map(|trace| (trace, &log.inner)))
    }

    /// All the logs
    pub fn all_logs(&self) -> impl Iterator<Item = &TransactionLog> {
        self.logs.values()
    }

    /// All the logs that are not resolved yet and issued by the given address
    pub fn logs_from(&self, address: Address) -> impl Iterator<Item = &TransactionLog> {
        self.logs().filter(move |log| log.address == address)
    }

    /// All the calls that are still unknown
    pub fn calls(&self) -> impl Iterator<Item = &InternalCall> {
        self.calls
            .values()
            .filter_map(|call| call.classification.is_unknown().then(|| call))
    }

    /// All the calls that are not resolved yet
    pub fn calls_mut(&mut self) -> impl Iterator<Item = &mut InternalCall> {
        self.calls
            .values_mut()
            .filter_map(|call| call.classification.is_unknown().then(|| call))
    }

    /// All unassigned logs that occurred after the log
    pub fn logs_after(&self, log_index: impl AsRef<U256>) -> impl Iterator<Item = &TransactionLog> {
        let index = *log_index.as_ref();
        self.logs().filter(move |c| c.log_index > index)
    }

    /// All unassigned logs prior to log
    pub fn logs_prior(&self, log_index: impl AsRef<U256>) -> impl Iterator<Item = &TransactionLog> {
        let index = *log_index.as_ref();
        self.logs().filter(move |c| c.log_index < index)
    }

    /// Returns an iterator over all logs that are assigned to sub calls of the call assigned to the log with the given index.
    pub fn sub_logs(
        &self,
        log_index: impl AsRef<U256>,
    ) -> impl Iterator<Item = (&CallTraceAddress, &EventLog)> {
        let index = *log_index.as_ref();
        let mut trace = None;
        self.assigned_logs()
            .skip_while(move |(t, log)| {
                if log.log_index == index {
                    trace = Some(*t);
                }
                trace.is_none()
            })
            .skip(1)
            .filter(move |(t2, _)| is_subtrace(trace.as_ref().expect("exists; qed"), t2))
    }

    /// Iterate over all the call's subcalls
    pub fn subcalls<'a: 'b, 'b>(
        &'a self,
        t1: &'b [usize],
    ) -> impl Iterator<Item = &'a InternalCall> + 'b {
        self.calls().filter(move |c| {
            let t2 = &c.trace_address;
            if t2 == t1 {
                false
            } else {
                is_subtrace(t1, t2)
            }
        })
    }

    /// Add a new action to the action set
    ///
    /// The action's call will be assigned to logs included in this action
    pub fn push_action(&mut self, action: Action) {
        for log in &action.logs {
            // assign the action's call
            self.logs
                .get_mut(log)
                .and_then(|l| l.assign_to(action.call.clone()));
        }
        self.actions.push(action)
    }

    /// Add a series of actions
    pub fn extend_actions(&mut self, actions: impl Iterator<Item = Action>) {
        for action in actions {
            self.push_action(action);
        }
    }

    /// Iterator over all the actions identified for this tx
    pub fn actions(&self) -> impl Iterator<Item = &Action> {
        self.actions.iter()
    }
}
