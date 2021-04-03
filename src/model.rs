//! Database model types
use crate::mevdb::DbError;
use ethers::types::*;
use rust_decimal::prelude::{FromStr, ToPrimitive};
use rust_decimal::Decimal;
use std::convert::TryFrom;
use std::fmt;
use tokio_postgres::row::RowIndex;
use tokio_postgres::Row;

/// Helper trait to convert from `tokio_postgres::Row`
pub trait FromSqlRow {
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

/// Representation of a Defi protocol address
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ProtocolAddress {
    /// The address of the contract
    pub address: Address,
    /// name of the protocol
    pub name: String,
}

impl FromSqlRow for ProtocolAddress {
    fn from_row(row: &Row) -> Result<Self, DbError>
    where
        Self: Sized,
    {
        let address = row.try_get_address("address")?;
        let name = row.try_get("name")?;
        Ok(Self { address, name })
    }
}

/// Representation of a contract that's either a token, proxy, router
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ProtocolJunctionAddress {
    /// The address of the contract
    pub address: Address,
    /// name of the protocol
    pub name: String,
    /// additional information
    pub info: String,
}

impl FromSqlRow for ProtocolJunctionAddress {
    fn from_row(row: &Row) -> Result<Self, DbError>
    where
        Self: Sized,
    {
        let address = row.try_get_address("address")?;
        let name = row.try_get("name")?;
        let info = row.try_get("info")?;
        Ok(Self {
            address,
            name,
            info,
        })
    }
}

/// Database model of an internal value transfer within a ethereum transaction
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct InternalTransfer {
    /// The hash of the transaction this event occurred in
    pub transaction_hash: TxHash,
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
}

impl FromSqlRow for InternalTransfer {
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

        Ok(Self {
            transaction_hash,
            trace_address,
            value,
            gas_used,
            from,
            to,
        })
    }
}

/// Database model of an ethereum event
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EventLog {
    /// The hash of the transaction this event occurred in
    pub transaction_hash: TxHash,
    /// The signature of the event
    pub signature: H256,
    /// all the other topics
    pub topics: Vec<H256>,
    /// all the other data of the log
    pub data: Vec<u8>,
    /// the index of the log's transaction in the block
    pub transaction_index: u64,
    /// log position within the block
    pub log_index: U256,
    /// log index position
    pub transaction_log_index: U256,
    /// The number of the block
    pub block_number: u64,
}

impl TryFrom<Log> for EventLog {
    type Error = ();

    // tries to convert a `ethers::Log` and fails if it's an anonymous log or not included yet
    fn try_from(value: Log) -> Result<Self, Self::Error> {
        let Log {
            mut topics,
            data,
            block_number,
            transaction_hash,
            transaction_index,
            log_index,
            transaction_log_index,
            ..
        } = value;

        if topics.is_empty() {
            return Err(());
        }
        let signature = topics.remove(0);

        Ok(Self {
            transaction_hash: transaction_hash.ok_or(())?,
            signature,
            topics,
            data: data.to_vec(),
            transaction_index: transaction_index.ok_or(())?.as_u64(),
            log_index: log_index.ok_or(())?,
            transaction_log_index: transaction_log_index.ok_or(())?,
            block_number: block_number.ok_or(())?.as_u64(),
        })
    }
}

impl FromSqlRow for EventLog {
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

        let data: Vec<u8> = row.try_get("topics")?;

        let transaction_index = row.try_get_u64("transaction_index")?;
        let signature = row.try_get_h256("signature")?;
        let log_index = row.try_get_u256("log_index")?;
        let transaction_log_index = row.try_get_u256("transaction_log_index")?;
        let block_number = row.try_get_u64("block_number")?;

        Ok(Self {
            transaction_hash,
            signature,
            topics,
            data,
            transaction_index,
            log_index,
            transaction_log_index,
            block_number,
        })
    }
}
