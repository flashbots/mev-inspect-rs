use crate::{
    addresses::{AAVE_LENDING_POOL, ETH},
    is_subtrace,
    types::{
        actions::{Liquidation, ProfitableLiquidation, SpecificAction, Transfer},
        Classification, Inspection, Protocol,
    },
    Inspector, Reducer,
};
use ethers::{
    abi::Abi,
    contract::BaseContract,
    types::{Address, U256},
};

type LiquidationCall = (Address, Address, Address, U256, bool);

pub struct Aave {
    pub pool: BaseContract,
    // TODO: Unused?
    pub core: BaseContract,
}

impl Aave {
    pub fn new() -> Self {
        Aave {
            pool: BaseContract::from({
                serde_json::from_str::<Abi>(include_str!("../../abi/aavepool.json"))
                    .expect("could not parse aave abi")
            }),
            core: BaseContract::from({
                serde_json::from_str::<Abi>(include_str!("../../abi/aavecore.json"))
                    .expect("could not parse aave core abi")
            }),
        }
    }
}

impl Reducer for Aave {
    fn reduce(&self, inspection: &mut Inspection) {
        let actions = inspection.actions.clone();
        let mut prune = Vec::new();

        let mut found = None;
        inspection
            .actions
            .iter_mut()
            .enumerate()
            .for_each(|(i, action)| {
                let t1 = action.clone().trace_address();
                match action {
                    Classification::Known(action_trace) => match &mut action_trace.action {
                        SpecificAction::Liquidation(liquidation) => {
                            for (j, c) in actions.iter().enumerate() {
                                let t2 = c.trace_address();
                                if t2 == t1 {
                                    continue;
                                }

                                if is_subtrace(&t1, &t2) {
                                    match c {
                                        Classification::Known(action_trace2) => match action_trace2
                                            .as_ref()
                                        {
                                            SpecificAction::Transfer(transfer) => {
                                                if transfer.to == liquidation.from
                                                    && (transfer.token
                                                        == liquidation.received_token
                                                        || transfer.token == *ETH)
                                                {
                                                    liquidation.received_amount = transfer.amount;
                                                    if found.is_none() {
                                                        found = Some((i, liquidation.clone()));
                                                    }
                                                }
                                            }
                                            _ => (),
                                        },
                                        Classification::Unknown(_) => {}
                                        Classification::Prune => (),
                                    };
                                    prune.push(j);
                                }
                            }
                        }
                        _ => (),
                    },
                    _ => (),
                }
            });

        // Remove the traces which were subtraces of liquidation txs. Assuming
        // the Uniswap inspector has been executed first, there will be a transfer
        // which will populate the liquidation's received amount
        prune
            .iter()
            .for_each(|p| inspection.actions[*p] = Classification::Prune);

        // Find the first trade which has the same token as the liquidation token
        // to calculate the profit
        // TODO: Is this the correct heuristic? Maybe it should be "find the last
        // transfer which happens before"
        if let Some((i, liquidation)) = found {
            let found: Option<&Transfer> = actions.iter().find_map(|classification| {
                let transfer = classification.to_action().map(|x| x.transfer()).flatten();
                if let Some(inner) = transfer {
                    if inner.token == liquidation.received_token {
                        return transfer;
                    }
                }
                None
            });

            if let Some(transfer) = found {
                if liquidation.received_amount > transfer.amount {
                    inspection.actions[i] = Classification::new(
                        ProfitableLiquidation {
                            liquidation: liquidation.clone(),
                            token: liquidation.received_token,
                            profit: liquidation.received_amount - transfer.amount,
                        },
                        Vec::new(),
                    ); // TODO: Figure out what the trace here should be
                }
            }
        }
    }
}

impl Inspector for Aave {
    fn classify(&self, inspection: &mut Inspection) {
        let actions = &mut inspection.actions;

        let protocols = actions
            .iter_mut()
            .filter_map(|action| self.inspect_one(action))
            .collect::<Vec<_>>();

        inspection.protocols.extend(&protocols[..]);
        inspection.protocols.sort_unstable();
        inspection.protocols.dedup();
    }
}

impl Aave {
    fn inspect_one(&self, action: &mut Classification) -> Option<Protocol> {
        let mut res = None;
        match action {
            Classification::Unknown(ref mut calltrace) => {
                let call = calltrace.as_ref();
                if call.to == *AAVE_LENDING_POOL {
                    res = Some(Protocol::Aave);

                    // https://github.com/aave/aave-protocol/blob/master/contracts/lendingpool/LendingPool.sol#L805
                    if let Ok((collateral, reserve, user, purchase_amount, _)) =
                        self.pool
                            .decode::<LiquidationCall, _>("liquidationCall", &call.input)
                    {
                        // Set the amount to 0. We'll set it at the reducer
                        *action = Classification::new(
                            Liquidation {
                                sent_token: reserve,
                                sent_amount: purchase_amount,

                                received_token: collateral,
                                received_amount: U256::zero(),
                                from: call.from,
                                liquidated_user: user,
                            },
                            calltrace.trace_address.clone(),
                        );
                    }
                }
            }
            Classification::Known(_) | Classification::Prune => {}
        }

        res
    }
}
