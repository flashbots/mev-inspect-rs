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

use thiserror::Error;

#[derive(Debug, Clone, PartialOrd, PartialEq, Eq, Hash)]
pub enum ActionType {
    Liquidation,
    Arbitrage,
    Trade,
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
