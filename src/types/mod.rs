//! All the datatypes associated with MEV-Inspect
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::str::FromStr;

use ethers::types::{Action as TraceAction, Address, CallType, Trace, TxHash, U256};

pub use classification::Classification;
pub use evaluation::{EvalError, Evaluation};
pub use inspection::Inspection;

use crate::{
    addresses::{DYDX, FILTER, ZEROX},
    is_subtrace,
    model::{EventLog, InternalCall},
    types::actions::SpecificAction,
};
use ethers::contract::EthLogDecode;
use itertools::Itertools;
use std::collections::{BTreeMap, HashMap};

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

    /// Misc erc20
    Erc20,
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
            "erc20" => Ok(Protocol::Erc20),
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
    calls: Vec<InternalCall>,
    calls_idx: HashMap<CallTraceAddress, usize>,

    /// calls and their logs (indices) identified by `call.to == log.adress `
    logs_by: BTreeMap<CallTraceAddress, Vec<U256>>,
    /// actions identified in this transaction
    actions: Vec<Action>,
}

impl TransactionData {
    pub fn create(
        traces: impl IntoIterator<Item = Trace>,
        logs: Vec<EventLog>,
    ) -> Result<Self, ()> {
        let mut traces = traces.into_iter().peekable();

        // get the first trace
        let trace = match traces.peek() {
            Some(inner) => inner,
            None => return Err(()),
        };
        let initial_call = match trace.action {
            TraceAction::Call(ref call) => call,
            // the first action we care about must be a call. everything else
            // is junk
            _ => return Err(()),
        };

        // Filter out unwanted calls
        if FILTER.get(&initial_call.to).is_some() {
            return Err(());
        }

        let mut status = Status::Success;
        let mut proxy_impl = None;
        let from = initial_call.from;
        let contract = initial_call.to;
        let hash = trace.transaction_hash.unwrap_or_else(TxHash::zero);
        let block_number = trace.block_number;
        let transaction_position = trace.transaction_position.expect("Trace has position");

        let logs: BTreeMap<_, _> = logs
            .into_iter()
            .map(|log| {
                (
                    log.log_index,
                    TransactionLog {
                        inner: log,
                        assigned_to_call: None,
                    },
                )
            })
            .collect();

        let calls: Vec<_> = traces
            .into_iter()
            .filter_map(|trace| {
                // Revert if all subtraces revert? There are counterexamples
                // e.g. when a low-level trace's revert is handled
                if trace.error.is_some() {
                    status = Status::Reverted;
                }

                if let TraceAction::Call(call) = trace.action {
                    // find internal calls
                    let internal_call = InternalCall {
                        transaction_hash: trace.transaction_hash.expect("tx exists"),
                        call_type: call.call_type.clone(),
                        trace_address: trace.trace_address,
                        value: call.value,
                        gas_used: call.gas,
                        from: call.from,
                        to: call.to,
                        input: call.input.to_vec(),
                        protocol: None,
                        classification: Default::default(),
                    };

                    if proxy_impl.is_none()
                        && call.call_type == CallType::DelegateCall
                        && call.from == contract
                    {
                        proxy_impl = Some(call.to);
                    }

                    Some(internal_call)
                } else {
                    None
                }
            })
            .collect();

        let calls_idx = calls
            .iter()
            .enumerate()
            .map(|(idx, call)| (call.trace_address.clone(), idx))
            .collect();

        let logs_by = calls
            .iter()
            .map(|call| {
                let call_logs: Vec<_> = logs
                    .values()
                    .filter(|log| log.address == call.to)
                    .map(|log| log.log_index)
                    .collect();
                (call.trace_address.clone(), call_logs)
            })
            .collect();

        let mut inspection = Self {
            status,
            // all unclassified calls
            actions: Vec::new(),
            from,
            contract,
            protocol: None,
            proxy_impl,
            hash,
            block_number,
            transaction_position,
            logs,
            calls,
            calls_idx,
            logs_by,
        };

        if inspection.contract == *DYDX {
            inspection.protocol = Some(Protocol::DyDx);
        }

        if inspection.contract == *ZEROX {
            inspection.protocol = Some(Protocol::ZeroEx);
        }

        Ok(inspection)
    }

    pub fn get_call(&self, trace_address: &CallTraceAddress) -> Option<&InternalCall> {
        self.calls_idx
            .get(trace_address)
            .map(|idx| &self.calls[*idx])
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
            .iter()
            .filter_map(|call| call.classification.is_unknown().then(|| call))
    }

    /// All the calls that are not resolved yet
    pub fn calls_mut(&mut self) -> impl Iterator<Item = &mut InternalCall> {
        self.calls
            .iter_mut()
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

    /// All logs issued by the callee (`call.to`) if this call
    pub fn logs_by_callee<'a: 'b, 'b>(
        &'a self,
        trace_address: &'b [usize],
    ) -> impl Iterator<Item = &TransactionLog> {
        self.logs_by
            .get(trace_address)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(move |idx| &self.logs[&idx])
    }

    /// Returns an iterator over all logs that resulted due to this call
    pub fn call_logs<'a: 'b, 'b>(
        &'a self,
        trace_address: &'b [usize],
    ) -> impl Iterator<Item = (&InternalCall, &EventLog)> {
        self.logs_by_callee(trace_address)
            .chain(
                self.subcalls(trace_address)
                    .flat_map(|sub| self.logs_by_callee(&sub.trace_address)),
            )
            .sorted_by_key(|log| log.log_index)
            .map(move |log| {
                let call = log
                    .assigned_call()
                    .and_then(|t| self.get_call(t))
                    .expect("call exist; qed");

                (call, &log.inner)
            })
    }

    /// Returns an iterator over all logs that resulted due to this call that could successfully be decoded
    pub fn call_logs_decoded<'a: 'b, 'b, T: EthLogDecode>(
        &'a self,
        trace_address: &'b [usize],
    ) -> impl Iterator<Item = (&InternalCall, &EventLog, T)> {
        self.call_logs(trace_address).filter_map(|(call, log)| {
            T::decode_log(&log.raw_log)
                .map(|decoded| (call, log, decoded))
                .ok()
        })
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

    /// Add a series of actions and keeps them sorted by call trace
    pub fn extend_actions(&mut self, actions: impl Iterator<Item = Action>) {
        for action in actions {
            let num_parents = self
                .actions
                .iter()
                .take_while(|probe| {
                    let t1 = &probe.call;
                    let t2 = &action.call;
                    t1 == t2 || is_subtrace(t1, t2)
                })
                .count();
            self.actions.insert(num_parents, action)
        }
    }

    /// Iterator over all the actions identified for this tx sorted by the associated call
    pub fn actions(&self) -> impl Iterator<Item = &Action> {
        self.actions.iter()
    }

    /// Iterator over all the actions identified for this tx sorted by the associated call
    pub fn actions_mut(&mut self) -> impl Iterator<Item = &mut Action> {
        self.actions.iter_mut()
    }

    pub fn remove_action(&mut self, idx: usize) -> Action {
        self.actions.remove(idx)
    }

    pub fn get_action_mut(&mut self, idx: usize) -> Option<&mut Action> {
        self.actions.get_mut(idx)
    }
}
