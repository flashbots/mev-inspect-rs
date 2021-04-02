use crate::{
    types::{actions::SpecificAction, Inspection, Status},
    HistoricalPrice,
};

use ethers::{
    contract::ContractError,
    providers::Middleware,
    types::{TxHash, U256},
};
use std::collections::HashSet;

use crate::mevdb::DbError;
use crate::model::FromSqlRow;
use crate::types::Protocol;
use ethers::types::Address;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::fmt;
use std::str::FromStr;
use thiserror::Error;
use tokio_postgres::Row;

#[derive(Debug, Clone, PartialOrd, PartialEq, Eq, Hash)]
pub enum ActionType {
    Liquidation,
    Arbitrage,
    Trade,
}

impl fmt::Display for ActionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", format!("{:?}", self).to_lowercase())
    }
}

impl FromStr for ActionType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "liquidation" | "Liquidation" => Ok(ActionType::Liquidation),
            "arbitrage" | "Arbitrage" => Ok(ActionType::Arbitrage),
            "trade" | "Trade" => Ok(ActionType::Trade),
            s => Err(format!("`{}` is nat a valid action type", s)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Evaluation {
    /// The internal inspection which produced this evaluation
    pub inspection: Inspection,
    /// The gas used in total by this transaction
    pub gas_used: U256,
    /// The gas price used in this transaction
    pub gas_price: U256,
    /// The actions involved
    pub actions: HashSet<ActionType>,
    /// The money made by this transfer
    pub profit: U256,
}

impl AsRef<Inspection> for Evaluation {
    fn as_ref(&self) -> &Inspection {
        &self.inspection
    }
}

impl Evaluation {
    /// Takes an inspection and reduces it to the data format which will be pushed
    /// to the database.
    pub async fn new<T: Middleware>(
        inspection: Inspection,
        prices: &HistoricalPrice<T>,
        gas_used: U256,
        gas_price: U256,
    ) -> Result<Self, EvalError<T>>
    where
        T: 'static,
    {
        // TODO: Figure out how to sum up liquidations & arbs while pruning
        // aggressively
        // TODO: If an Inspection is CHECKED and contains >1 trading protocol,
        // then probably this is an Arbitrage?
        let mut actions = HashSet::new();
        let mut profit = U256::zero();
        for action in &inspection.actions {
            // only get the known actions
            let action = if let Some(action) = action.as_action() {
                action
            } else {
                continue;
            };

            // set their action type
            use SpecificAction::*;
            match action {
                Arbitrage(_) => {
                    actions.insert(ActionType::Arbitrage);
                }
                Liquidation(_) | ProfitableLiquidation(_) | LiquidationCheck => {
                    actions.insert(ActionType::Liquidation);
                }
                Trade(_) => {
                    actions.insert(ActionType::Trade);
                }
                _ => {}
            };

            // dont try to calculate & normalize profits for unsuccessful txs
            if inspection.status != Status::Success {
                continue;
            }

            match action {
                SpecificAction::Arbitrage(arb) => {
                    if arb.profit > 0.into() {
                        profit += prices
                            .quote(arb.token, arb.profit, inspection.block_number)
                            .await
                            .map_err(EvalError::Contract)?;
                    }
                }
                SpecificAction::Liquidation(liq) => {
                    if liq.sent_amount == U256::MAX {
                        eprintln!(
                            "U256::max detected in {}, skipping profit calculation",
                            inspection.hash
                        );
                        continue;
                    }
                    let res = futures::future::join(
                        prices.quote(liq.sent_token, liq.sent_amount, inspection.block_number),
                        prices.quote(
                            liq.received_token,
                            liq.received_amount,
                            inspection.block_number,
                        ),
                    )
                    .await;

                    match res {
                        (Ok(amount_in), Ok(amount_out)) => {
                            profit += amount_out.saturating_sub(amount_in);
                        }
                        _ => println!("Could not fetch prices from Uniswap"),
                    };

                    if res.0.is_err() {
                        println!("Sent: {} of token {:?}", liq.sent_amount, liq.sent_token);
                    }

                    if res.1.is_err() {
                        println!(
                            "Received: {} of token {:?}",
                            liq.received_amount, liq.received_token
                        );
                    }
                }
                SpecificAction::ProfitableLiquidation(liq) => {
                    profit += prices
                        .quote(liq.token, liq.profit, inspection.block_number)
                        .await
                        .map_err(EvalError::Contract)?;
                }
                _ => (),
            };
        }

        Ok(Evaluation {
            inspection,
            gas_used,
            gas_price,
            actions,
            profit,
        })
    }
}

impl FromSqlRow for Evaluation {
    fn from_row(row: &Row) -> Result<Self, DbError>
    where
        Self: Sized,
    {
        let hash = TxHash::from_str(row.try_get("hash")?)
            .map_err(|err| DbError::FromSqlError(err.to_string()))?;

        let status = Status::from_str(row.try_get("status")?).map_err(DbError::FromSqlError)?;

        let block_number: Decimal = row.try_get("block_number")?;

        let gas_price: Decimal = row.try_get("gas_price")?;
        let gas_price = U256::from_str_radix(&gas_price.to_string(), 10)
            .map_err(|err| DbError::FromSqlError(err.to_string()))?;

        let gas_used: Decimal = row.try_get("gas_used")?;
        let gas_used = U256::from_str_radix(&gas_used.to_string(), 10)
            .map_err(|err| DbError::FromSqlError(err.to_string()))?;

        let revenue: Decimal = row.try_get("revenue")?;
        let revenue = U256::from_str_radix(&revenue.to_string(), 10)
            .map_err(|err| DbError::FromSqlError(err.to_string()))?;

        let protocols: Vec<&str> = row.try_get("protocols")?;
        let protocols = protocols
            .into_iter()
            .map(Protocol::from_str)
            .collect::<Result<HashSet<_>, _>>()
            .map_err(DbError::FromSqlError)?;

        let actions: Vec<&str> = row.try_get("actions")?;
        let actions = actions
            .into_iter()
            .map(ActionType::from_str)
            .collect::<Result<HashSet<_>, _>>()
            .map_err(DbError::FromSqlError)?;

        let from = Address::from_str(row.try_get("eoa")?)
            .map_err(|err| DbError::FromSqlError(err.to_string()))?;

        let contract = Address::from_str(row.try_get("contract")?)
            .map_err(|err| DbError::FromSqlError(err.to_string()))?;

        let proxy: Option<&str> = row.try_get("proxy_impl")?;
        let proxy_impl = if let Some(proxy) = proxy {
            if proxy.is_empty() {
                None
            } else {
                Some(
                    Address::from_str(proxy)
                        .map_err(|err| DbError::FromSqlError(err.to_string()))?,
                )
            }
        } else {
            None
        };

        let transaction_position: Decimal = row.try_get("transaction_position")?;

        Ok(Self {
            inspection: Inspection {
                status,
                actions: vec![],
                protocols,
                from,
                contract,
                proxy_impl,
                hash,
                block_number: block_number
                    .to_u64()
                    .ok_or_else(|| DbError::FromSqlError("Failed to convert to u64".to_string()))?,
                transaction_position: transaction_position.to_usize().ok_or_else(|| {
                    DbError::FromSqlError("Failed to convert to usize".to_string())
                })?,
            },
            gas_used,
            gas_price,
            actions,
            profit: revenue,
        })
    }
}

// TODO: Can we do something about the generic static type bounds?
#[derive(Debug, Error)]
pub enum EvalError<M: Middleware>
where
    M: 'static,
{
    #[error(transparent)]
    Provider(M::Error),
    #[error("Transaction was not found {0}")]
    TxNotFound(TxHash),
    #[error(transparent)]
    Contract(ContractError<M>),
}
