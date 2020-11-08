use crate::inspectors::addresses::ADDRESSBOOK;
use ethers::types::{Address, Bytes, Call, TxHash, U256};
use rustc_hex::ToHex;
use std::fmt;

pub struct Evaluation {
    // Maybe add Tx & its Receipt?
    pub hash: TxHash,

    pub status: Status,

    pub actions: Vec<SpecificAction>,

    pub unknown_calls: Vec<Call>,

    pub inferred_type: Protocol, // TODO: Chagne to Arb/Liquidation/Bot/?

    pub profit: U256,
}

// https://github.com/flashbots/mev-inspect/blob/master/src/types.ts#L65-L87
#[derive(Debug, Clone, PartialOrd, PartialEq)]
/// The types of actions
pub enum SpecificAction {
    // Base building blocks
    Transfer(Transfer),
    Liquidation(Liquidation),

    Trade(Trade),

    // Complex ones
    Arbitrage(Arbitrage),

    WethDeposit(Deposit),
    WethWithdrawal(Withdrawal),

    Unclassified(Bytes),
}

#[derive(Debug, Clone, PartialOrd, PartialEq)]
pub struct ActionTrace {
    pub action: SpecificAction,
    pub trace_address: Vec<usize>,
}

impl AsRef<SpecificAction> for ActionTrace {
    fn as_ref(&self) -> &SpecificAction {
        &self.action
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallTrace {
    pub call: Call,
    pub trace_address: Vec<usize>,
}

impl AsRef<Call> for CallTrace {
    fn as_ref(&self) -> &Call {
        &self.call
    }
}

impl SpecificAction {
    pub fn liquidation(&self) -> Option<&Liquidation> {
        match self {
            SpecificAction::Liquidation(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn trade(&self) -> Option<&Trade> {
        match self {
            SpecificAction::Trade(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn transfer(&self) -> Option<&Transfer> {
        match self {
            SpecificAction::Transfer(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn deposit(&self) -> Option<&Deposit> {
        match self {
            SpecificAction::WethDeposit(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn withdrawal(&self) -> Option<&Withdrawal> {
        match self {
            SpecificAction::WethWithdrawal(inner) => Some(inner),
            _ => None,
        }
    }
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

#[derive(Clone, PartialEq)]
pub enum Classification {
    Known(ActionTrace),
    Unknown(CallTrace),
    Prune,
}

impl Classification {
    /// Gets the trace address in this call (Empty if Prune)
    pub fn trace_address(&self) -> Vec<usize> {
        match &self {
            Classification::Known(inner) => inner.trace_address.clone(),
            Classification::Unknown(inner) => inner.trace_address.clone(),
            Classification::Prune => vec![],
        }
    }

    pub fn prune_subcalls(&self, classifications: &mut [Classification]) {
        let t1 = self.trace_address();

        for c in classifications.iter_mut() {
            let t2 = c.trace_address();
            if t2 == t1 {
                continue;
            }

            if is_subtrace(&t1, &t2) {
                *c = Classification::Prune;
            }
        }
    }

    pub fn subcalls(&self, classifications: &[Classification]) -> Vec<Classification> {
        let t1 = self.trace_address();

        let mut v = Vec::new();
        for c in classifications.iter() {
            let t2 = c.trace_address();

            if is_subtrace(&t1, &t2) {
                v.push(c.clone());
            }
        }
        v
    }
}

impl Classification {
    pub fn new<T: Into<SpecificAction>>(action: T, trace_address: Vec<usize>) -> Self {
        Classification::Known(ActionTrace {
            action: action.into(),
            trace_address,
        })
    }

    pub fn to_action(&self) -> Option<&SpecificAction> {
        match self {
            Classification::Known(ref inner) => Some(&inner.action),
            _ => None,
        }
    }
}

impl fmt::Debug for Classification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self {
            Classification::Known(action) => write!(f, "{:#?}", action),
            Classification::Unknown(CallTrace {
                call,
                trace_address,
            }) => f
                .debug_struct("TraceCall")
                .field("from", &lookup(call.from))
                .field("to", &lookup(call.to))
                .field("value", &call.value)
                .field("gas", &call.gas)
                .field("input", &call.input.as_ref().to_hex::<String>())
                .field("call_type", &call.call_type)
                .field("trace", trace_address)
                .finish(),
            Classification::Prune => f.debug_tuple("Pruned").finish(),
        }
    }
}

impl From<CallTrace> for Classification {
    fn from(call: CallTrace) -> Self {
        Classification::Unknown(call)
    }
}

impl From<ActionTrace> for Classification {
    fn from(action: ActionTrace) -> Self {
        Classification::Known(action)
    }
}

#[derive(Debug, Clone, PartialOrd, PartialEq)]
pub enum Status {
    // Reverted(String),
    Reverted,
    Success,
}

fn lookup(address: Address) -> String {
    ADDRESSBOOK
        .get(&address)
        .unwrap_or(&format!("{:?}", &address).to_string())
        .clone()
}

#[derive(Clone, PartialOrd, PartialEq)]
/// A token transfer
pub struct Transfer {
    pub from: Address,
    pub to: Address,
    pub amount: U256,
    pub token: Address,
}

impl From<Transfer> for SpecificAction {
    fn from(src: Transfer) -> Self {
        SpecificAction::Transfer(src)
    }
}

// Manually implemented Debug (and Display?) for datatypes so that we
// can get their token names instead of using addresses. TODO: Could we
// also normalize the decimals? What about tokens with non-18 decimals e.g. Tether?
impl fmt::Debug for Transfer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Transfer")
            .field("from", &lookup(self.from))
            .field("to", &lookup(self.to))
            .field("amount", &self.amount)
            .field("token", &lookup(self.token))
            .finish()
    }
}

#[derive(Clone, PartialOrd, PartialEq)]
pub struct Deposit {
    pub from: Address,
    pub amount: U256,
}

impl From<Deposit> for SpecificAction {
    fn from(src: Deposit) -> Self {
        SpecificAction::WethDeposit(src)
    }
}

impl fmt::Debug for Deposit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Deposit")
            .field("from", &lookup(self.from))
            .field("amount", &self.amount)
            .finish()
    }
}

#[derive(Clone, PartialOrd, PartialEq)]
pub struct Withdrawal {
    pub to: Address,
    pub amount: U256,
}

impl From<Withdrawal> for SpecificAction {
    fn from(src: Withdrawal) -> Self {
        SpecificAction::WethWithdrawal(src)
    }
}

impl fmt::Debug for Withdrawal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Withdrawal")
            .field("to", &lookup(self.to))
            .field("amount", &self.amount)
            .finish()
    }
}

#[derive(Debug, Clone, PartialOrd, PartialEq)]
pub struct Trade {
    pub t1: Transfer,
    pub t2: Transfer,
}

impl From<Trade> for SpecificAction {
    fn from(src: Trade) -> Self {
        SpecificAction::Trade(src)
    }
}

impl Trade {
    /// Creates a new trade made up of 2 matching transfers
    pub fn merge(t1: Transfer, t2: Transfer) -> Self {
        assert!(t1.from == t2.to && t2.from == t1.to);
        Self { t1, t2 }
    }
}

#[derive(Clone, PartialOrd, PartialEq)]
pub struct Arbitrage {
    pub profit: U256,
    pub token: Address,
    pub to: Address,
}

impl From<Arbitrage> for SpecificAction {
    fn from(src: Arbitrage) -> Self {
        SpecificAction::Arbitrage(src)
    }
}

impl fmt::Debug for Arbitrage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Arbitrage")
            .field("profit", &self.profit)
            .field("to", &lookup(self.to))
            .field("token", &lookup(self.token))
            .finish()
    }
}

#[derive(Clone, PartialOrd, PartialEq)]
pub struct Liquidation {
    pub sent_token: Address,
    pub sent_amount: U256,

    pub received_token: Address,
    pub received_amount: U256,

    pub from: Address,
    pub liquidated_user: Address,
}

impl From<Liquidation> for SpecificAction {
    fn from(src: Liquidation) -> Self {
        SpecificAction::Liquidation(src)
    }
}

impl fmt::Debug for Liquidation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Liquidation")
            .field("sent_token", &lookup(self.sent_token))
            .field("sent_amount", &self.sent_amount)
            .field("received_token", &lookup(self.received_token))
            .field("received_amount", &self.received_amount)
            .field("liquidated_user", &lookup(self.liquidated_user))
            .field("from", &lookup(self.from))
            .finish()
    }
}

/// Checks if `a2` is a subtrace of `a1`
pub fn is_subtrace(a1: &[usize], a2: &[usize]) -> bool {
    if a1.is_empty() {
        return false;
    }

    a1 == &a2[..std::cmp::min(a1.len(), a2.len())]
}

#[cfg(test)]
mod tests {
    use super::is_subtrace;

    #[test]
    fn check() {
        let test_cases = vec![
            (vec![0], vec![0, 1], true),
            (vec![0], vec![0, 0], true),
            (vec![0, 1], vec![0, 1, 0], true),
            (vec![0, 1], vec![0, 1, 1], true),
            (vec![0, 1], vec![0, 2], false),
            (vec![0, 1], vec![0], false),
            (vec![], vec![0, 1], false),
            (vec![15], vec![15, 0, 3, 22, 0, 0], true),
        ];

        for (a1, a2, expected) in test_cases {
            assert_eq!(is_subtrace(&a1, &a2), expected);
        }
    }
}
