use crate::types::{actions::SpecificAction, Classification, Inspection};

use ethers::{
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
    pub async fn new<T: Middleware>(
        inspection: Inspection,
        provider: &T,
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
        let mut actions = Vec::new();
        let mut profit = U256::zero();
        for action in &inspection.actions {
            match action {
                Classification::Known(action) => match action.as_ref() {
                    SpecificAction::Arbitrage(arb) => {
                        actions.push(ActionType::Arbitrage);
                        profit += arb.profit;
                    }
                    SpecificAction::Liquidation(_) => actions.push(ActionType::Liquidation),
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
    M::Error: 'static,
{
    #[error(transparent)]
    Provider(M::Error),
    #[error("Transaction was not found {0}")]
    TxNotFound(TxHash),
}
