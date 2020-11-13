use crate::{
    types::{actions::SpecificAction, Classification, Inspection},
    HistoricalPrice,
};

use ethers::{
    contract::ContractError,
    providers::Middleware,
    types::{TxHash, U256},
};

use thiserror::Error;

#[derive(Debug, Clone, PartialOrd, PartialEq)]
pub enum ActionType {
    Liquidation,
    Arbitrage,
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
    pub actions: Vec<ActionType>,
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
        provider: &T,
        prices: &HistoricalPrice<T>,
    ) -> Result<Self, EvalError<T>>
    where
        T: 'static,
    {
        let receipt = provider
            .get_transaction_receipt(inspection.hash)
            .await
            .map_err(EvalError::Provider)?
            .ok_or(EvalError::TxNotFound(inspection.hash))?;

        let tx = provider
            .get_transaction(inspection.hash)
            .await
            .map_err(EvalError::Provider)?
            .ok_or(EvalError::TxNotFound(inspection.hash))?;

        // TODO: Figure out how to sum up liquidations & arbs while pruning
        // aggressively
        // TODO: If an Inspection is CHECKED and contains >1 trading protocol,
        // then probably this is an Arbitrage?
        let mut actions = Vec::new();
        let mut profit = U256::zero();
        for action in &inspection.actions {
            match action {
                Classification::Known(action) => match action.as_ref() {
                    SpecificAction::Arbitrage(arb) => {
                        if arb.profit > 0.into() {
                            actions.push(ActionType::Arbitrage);
                            profit += prices
                                .quote(arb.token, arb.profit, inspection.block_number)
                                .await
                                .map_err(EvalError::Contract)?;
                        }
                    }
                    SpecificAction::Liquidation(liq) => {
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

                        actions.push(ActionType::Liquidation)
                    }
                    SpecificAction::ProfitableLiquidation(liq) => {
                        actions.push(ActionType::Liquidation);
                        profit += prices
                            .quote(liq.token, liq.profit, inspection.block_number)
                            .await
                            .map_err(EvalError::Contract)?;
                    }
                    _ => (),
                },
                _ => (),
            };
        }

        Ok(Evaluation {
            inspection,
            gas_used: receipt.gas_used.unwrap_or(U256::zero()),
            gas_price: tx.gas_price,
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
