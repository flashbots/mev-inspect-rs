//! Database model types
use std::cmp::Ordering;
use std::convert::TryFrom;
use std::fmt;

use ethers::abi::RawLog;
use ethers::types::*;
use rust_decimal::prelude::{FromStr, ToPrimitive};
use rust_decimal::Decimal;
use tokio_postgres::{
    row::RowIndex,
    types::{FromSql, ToSql},
    Row,
};

use crate::is_subtrace;
use crate::mevdb::DbError;
use crate::types::Protocol;

/// Helper trait to convert from `tokio_postgres::Row`
pub trait SqlRowExt {
    /// create a type from the row
    fn from_row(row: &tokio_postgres::Row) -> Result<Self, DbError>
    where
        Self: Sized;
}

/// Internal helper trait
pub(crate) trait FromSqlExt {
    fn try_get_address<I>(&self, idx: I) -> Result<Address, DbError>
    where
        I: RowIndex + fmt::Display;

    fn try_get_u256<I>(&self, idx: I) -> Result<U256, DbError>
    where
        I: RowIndex + fmt::Display;

    fn try_get_h256<I>(&self, idx: I) -> Result<H256, DbError>
    where
        I: RowIndex + fmt::Display;

    fn try_get_u64<I>(&self, idx: I) -> Result<u64, DbError>
    where
        I: RowIndex + fmt::Display;

    fn try_get_usize<I>(&self, idx: I) -> Result<usize, DbError>
    where
        I: RowIndex + fmt::Display;
}

impl FromSqlExt for Row {
    fn try_get_address<I>(&self, idx: I) -> Result<Address, DbError>
    where
        I: RowIndex + fmt::Display,
    {
        Address::from_str(self.try_get(idx)?).map_err(|err| DbError::FromSqlError(err.to_string()))
    }

    fn try_get_u256<I>(&self, idx: I) -> Result<U256, DbError>
    where
        I: RowIndex + fmt::Display,
    {
        let value: Decimal = self.try_get(idx)?;
        U256::from_str_radix(&value.to_string(), 10)
            .map_err(|err| DbError::FromSqlError(err.to_string()))
    }

    fn try_get_h256<I>(&self, idx: I) -> Result<H256, DbError>
    where
        I: RowIndex + fmt::Display,
    {
        H256::from_str(self.try_get(idx)?).map_err(|err| DbError::FromSqlError(err.to_string()))
    }

    fn try_get_u64<I>(&self, idx: I) -> Result<u64, DbError>
    where
        I: RowIndex + fmt::Display,
    {
        let value: Decimal = self.try_get(idx)?;
        value
            .to_u64()
            .ok_or_else(|| DbError::FromSqlError("Failed to convert decimal to u64".to_string()))
    }

    fn try_get_usize<I>(&self, idx: I) -> Result<usize, DbError>
    where
        I: RowIndex + fmt::Display,
    {
        let value: Decimal = self.try_get(idx)?;
        value
            .to_usize()
            .ok_or_else(|| DbError::FromSqlError("Failed to convert decimal to usize".to_string()))
    }
}

#[derive(Debug, Clone, Copy, PartialOrd, PartialEq, Eq, Hash, ToSql, FromSql)]
#[postgres(name = "call_classification")]
pub enum CallClassification {
    #[postgres(name = "unknown")]
    Unknown,
    #[postgres(name = "deposit")]
    Deposit,
    #[postgres(name = "withdrawal")]
    Withdrawal,
    #[postgres(name = "transfer")]
    Transfer,
    #[postgres(name = "liquidation")]
    Liquidation,
    #[postgres(name = "addliquidity")]
    AddLiquidity,
    #[postgres(name = "removeliquidity")]
    RemoveLiquidity,
    #[postgres(name = "repay")]
    Repay,
    #[postgres(name = "borrow")]
    Borrow,
    /// A swap
    /// TODO clarify: may also be a flash swap, since "all swaps are actually flash swaps"
    ///  https://uniswap.org/docs/v2/smart-contract-integration/using-flash-swaps/
    #[postgres(name = "swap")]
    Swap,
    #[postgres(name = "flashswap")]
    FlashSwap,
}

impl CallClassification {
    /// Whether this is still not classified
    pub fn is_unknown(&self) -> bool {
        matches!(self, CallClassification::Unknown)
    }
    /// Whether this is still not classified
    pub fn is_swap(&self) -> bool {
        matches!(self, CallClassification::Swap)
    }
}

impl Default for CallClassification {
    fn default() -> Self {
        CallClassification::Unknown
    }
}

impl fmt::Display for CallClassification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", format!("{:?}", self).to_lowercase())
    }
}

impl FromStr for CallClassification {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "unknown" => Ok(CallClassification::Unknown),
            "deposit" => Ok(CallClassification::Deposit),
            "withdrawal" => Ok(CallClassification::Withdrawal),
            "transfer" => Ok(CallClassification::Transfer),
            "liquidation" => Ok(CallClassification::Liquidation),
            "addliquidity" => Ok(CallClassification::AddLiquidity),
            "removeliquidity" => Ok(CallClassification::RemoveLiquidity),
            "borrow" => Ok(CallClassification::Borrow),
            "repay" => Ok(CallClassification::Repay),
            "swap" => Ok(CallClassification::Swap),
            "flashswap" => Ok(CallClassification::FlashSwap),
            s => Err(format!("`{}` is not a valid action type", s)),
        }
    }
}

/// Database model of an internal call within a transaction
#[derive(Debug, Clone, PartialEq)]
pub struct InternalCall {
    /// The hash of the transaction this event occurred in
    pub transaction_hash: TxHash,
    /// What kind of call this was
    pub call_type: CallType,
    /// The signature of the event
    pub trace_address: Vec<usize>,
    /// transferred value
    pub value: U256,
    /// The gas used in total by this transfer
    pub gas_used: U256,
    /// The internal caller who transferred the ETH
    pub from: Address,
    /// The address who received the ETH
    pub to: Address,
    /// The protocol of the callee
    pub protocol: Option<Protocol>,
    /// The input data to the call
    pub input: Vec<u8>,
    /// What kind of call this is, if it could be determined
    pub classification: CallClassification,
}

impl InternalCall {
    /// Whether this call is associated with the given protocol
    pub fn is_protocol(&self, protocol: Protocol) -> bool {
        self.protocol == Some(protocol)
    }
}

impl Eq for InternalCall {}

impl PartialOrd for InternalCall {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for InternalCall {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.trace_address.is_empty() || is_subtrace(&self.trace_address, &other.trace_address) {
            Ordering::Less
        } else if other.trace_address.is_empty()
            || is_subtrace(&other.trace_address, &self.trace_address)
        {
            Ordering::Greater
        } else {
            for idx in 0..std::cmp::min(self.trace_address.len(), other.trace_address.len()) {
                match self.trace_address[idx] {
                    i if i > other.trace_address[idx] => return Ordering::Greater,
                    i if i < other.trace_address[idx] => return Ordering::Less,
                    _ => {}
                }
            }
            self.trace_address.len().cmp(&other.trace_address.len())
        }
    }
}

/// Helper enum that mirrors `CallType`
#[derive(Debug, ToSql, FromSql)]
#[postgres(name = "call_type")]
pub(crate) enum SqlCallType {
    #[postgres(name = "none")]
    None,
    #[postgres(name = "call")]
    Call,
    #[postgres(name = "callcode")]
    CallCode,
    #[postgres(name = "delegatecall")]
    DelegateCall,
    #[postgres(name = "staticcall")]
    StaticCall,
}

impl From<CallType> for SqlCallType {
    fn from(c: CallType) -> Self {
        match c {
            CallType::None => SqlCallType::None,
            CallType::Call => SqlCallType::Call,
            CallType::CallCode => SqlCallType::CallCode,
            CallType::DelegateCall => SqlCallType::DelegateCall,
            CallType::StaticCall => SqlCallType::StaticCall,
        }
    }
}

impl From<SqlCallType> for CallType {
    fn from(c: SqlCallType) -> Self {
        match c {
            SqlCallType::None => CallType::None,
            SqlCallType::Call => CallType::Call,
            SqlCallType::CallCode => CallType::CallCode,
            SqlCallType::DelegateCall => CallType::DelegateCall,
            SqlCallType::StaticCall => CallType::StaticCall,
        }
    }
}

impl SqlRowExt for InternalCall {
    fn from_row(row: &Row) -> Result<Self, DbError>
    where
        Self: Sized,
    {
        let transaction_hash = row.try_get_h256("transaction_hash")?;

        let trace_address: Vec<Decimal> = row.try_get("trace_address")?;
        let trace_address = trace_address
            .into_iter()
            .map(|trace| {
                trace
                    .to_usize()
                    .ok_or_else(|| DbError::FromSqlError("Failed to convert to usize".to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let value = row.try_get_u256("value")?;
        let gas_used = row.try_get_u256("gas_used")?;
        let from = row.try_get_address("caller")?;
        let to = row.try_get_address("callee")?;
        let classification = row.try_get("classification")?;
        let call_type: SqlCallType = row.try_get("call_type")?;

        let protocol = if let Ok(proto) = row.try_get::<_, &str>("protocol") {
            if proto.is_empty() {
                None
            } else {
                Some(Protocol::from_str(proto).map_err(DbError::FromSqlError)?)
            }
        } else {
            None
        };

        Ok(Self {
            transaction_hash,
            trace_address,
            call_type: call_type.into(),
            value,
            gas_used,
            from,
            to,
            protocol,
            input: row.try_get("input")?,
            classification,
        })
    }
}

/// Database model of an ethereum event
#[derive(Debug, Clone)]
pub struct EventLog {
    /// Who issued this event
    pub address: Address,
    /// The hash of the transaction this event occurred in
    pub transaction_hash: TxHash,
    /// The signature of the event
    pub signature: H256,
    /// The raw Ethereum log
    pub raw_log: RawLog,
    /// the index of the log's transaction in the block
    pub transaction_index: u64,
    /// log position within the block
    pub log_index: U256,
    /// The number of the block
    pub block_number: u64,
}

impl TryFrom<Log> for EventLog {
    type Error = ();

    // tries to convert a `ethers::Log` and fails if it's an anonymous log or not included yet
    fn try_from(value: Log) -> Result<Self, Self::Error> {
        let Log {
            topics,
            data,
            block_number,
            transaction_hash,
            transaction_index,
            log_index,
            ..
        } = value;

        if topics.is_empty() {
            return Err(());
        }
        let signature = topics[0];

        Ok(Self {
            address: value.address,
            transaction_hash: transaction_hash.ok_or(())?,
            signature,
            raw_log: RawLog {
                topics,
                data: data.to_vec(),
            },
            transaction_index: transaction_index.ok_or(())?.as_u64(),
            log_index: log_index.ok_or(())?,
            block_number: block_number.ok_or(())?.as_u64(),
        })
    }
}

impl SqlRowExt for EventLog {
    fn from_row(row: &Row) -> Result<Self, DbError>
    where
        Self: Sized,
    {
        let transaction_hash = row.try_get_h256("transaction_hash")?;

        let topics: Vec<&str> = row.try_get("topics")?;
        let topics = topics
            .into_iter()
            .map(|topic| {
                H256::from_str(topic).map_err(|err| DbError::FromSqlError(err.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let data: Vec<u8> = row.try_get("data")?;

        let address = row.try_get_address("address")?;
        let transaction_index = row.try_get_u64("transaction_index")?;
        let signature = row.try_get_h256("signature")?;
        let log_index = row.try_get_u256("log_index")?;
        let block_number = row.try_get_u64("block_number")?;

        Ok(Self {
            address,
            transaction_hash,
            signature,
            raw_log: RawLog { topics, data },
            transaction_index,
            log_index,
            block_number,
        })
    }
}
