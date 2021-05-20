use crate::addresses::lookup;

use ethers::types::{Address, Bytes, U256};

use std::fmt;

// https://github.com/flashbots/mev-inspect/blob/master/src/types.ts#L65-L87
#[derive(Debug, Clone, PartialOrd, PartialEq)]
/// The types of actions
pub enum SpecificAction {
    Deposit(TokenDeposit),
    WethDeposit(Deposit),
    WethWithdrawal(Withdrawal),

    Transfer(Transfer),
    Trade(Trade),
    Liquidation(Liquidation),

    AddLiquidity(AddLiquidity),
    RemoveLiquidity(RemoveLiquidity),

    Arbitrage(Arbitrage),
    ProfitableLiquidation(ProfitableLiquidation),

    Unclassified(Bytes),

    LiquidationCheck,
}

#[derive(Debug, Clone, PartialOrd, PartialEq)]
pub struct AddLiquidity {
    pub tokens: Vec<Address>,
    pub amounts: Vec<U256>,
}

#[derive(Debug, Clone, PartialOrd, PartialEq)]
pub struct RemoveLiquidity {
    pub tokens: Vec<Address>,
    pub amounts: Vec<U256>,
}

impl From<AddLiquidity> for SpecificAction {
    fn from(src: AddLiquidity) -> Self {
        SpecificAction::AddLiquidity(src)
    }
}

impl SpecificAction {
    pub fn as_deposit(&self) -> Option<&Deposit> {
        match self {
            SpecificAction::WethDeposit(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn as_withdrawal(&self) -> Option<&Withdrawal> {
        match self {
            SpecificAction::WethWithdrawal(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn as_transfer(&self) -> Option<&Transfer> {
        match self {
            SpecificAction::Transfer(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn as_trade(&self) -> Option<&Trade> {
        match self {
            SpecificAction::Trade(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn as_arbitrage(&self) -> Option<&Arbitrage> {
        match self {
            SpecificAction::Arbitrage(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn as_liquidation(&self) -> Option<&Liquidation> {
        match self {
            SpecificAction::Liquidation(inner) => Some(inner),
            _ => None,
        }
    }

    // TODO: Can we convert these to AsRef / AsMut Options somehow?
    pub fn as_liquidation_mut(&mut self) -> Option<&mut Liquidation> {
        match self {
            SpecificAction::Liquidation(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn as_profitable_liquidation(&self) -> Option<&ProfitableLiquidation> {
        match self {
            SpecificAction::ProfitableLiquidation(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn as_add_liquidity(&self) -> Option<&AddLiquidity> {
        match self {
            SpecificAction::AddLiquidity(inner) => Some(inner),
            _ => None,
        }
    }
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
pub struct TokenDeposit {
    pub token: Address,
    pub from: Address,
    pub amount: U256,
}

impl fmt::Debug for TokenDeposit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Deposit")
            .field("token", &lookup(self.from))
            .field("from", &lookup(self.from))
            .field("amount", &self.amount)
            .finish()
    }
}

impl From<TokenDeposit> for SpecificAction {
    fn from(src: TokenDeposit) -> Self {
        SpecificAction::Deposit(src)
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
    pub fn new(t1: Transfer, t2: Transfer) -> Self {
        assert!(
            t1.from == t2.to && t2.from == t1.to,
            "Found mismatched trade"
        );
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

#[derive(Default, Clone, PartialOrd, PartialEq)]
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

#[derive(Clone, PartialOrd, PartialEq)]
pub struct ProfitableLiquidation {
    pub liquidation: Liquidation,
    pub profit: U256,
    pub token: Address,
}

impl AsRef<Liquidation> for ProfitableLiquidation {
    fn as_ref(&self) -> &Liquidation {
        &self.liquidation
    }
}

impl From<ProfitableLiquidation> for SpecificAction {
    fn from(src: ProfitableLiquidation) -> Self {
        SpecificAction::ProfitableLiquidation(src)
    }
}

impl fmt::Debug for ProfitableLiquidation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProfitableLiquidation")
            .field("liquidation", &self.liquidation)
            .field("profit", &self.profit)
            .field("token", &lookup(self.token))
            .finish()
    }
}
