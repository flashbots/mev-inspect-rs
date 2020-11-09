use crate::{
    addresses::{AAVE_LENDING_POOL_CORE, ETH},
    is_subtrace,
    types::{
        actions::{Liquidation, SpecificAction},
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
        inspection.actions.iter_mut().for_each(|action| {
            let t1 = action.clone().trace_address();
            match action {
                Classification::Known(action_trace) => match &mut action_trace.action {
                    SpecificAction::Liquidation(liquidation) => {
                        for (i, c) in actions.iter().enumerate() {
                            let t2 = c.trace_address();
                            if t2 == t1 {
                                continue;
                            }

                            if is_subtrace(&t1, &t2) {
                                match c {
                                    Classification::Known(action_trace2) => {
                                        match action_trace2.as_ref() {
                                            SpecificAction::Transfer(transfer) => {
                                                if transfer.to == liquidation.from
                                                    && (transfer.token
                                                        == liquidation.received_token
                                                        || transfer.token == *ETH)
                                                {
                                                    liquidation.received_amount = transfer.amount;
                                                }
                                            }
                                            _ => (),
                                        }
                                    }
                                    Classification::Unknown(_) => {}
                                    Classification::Prune => (),
                                };
                                prune.push(i);
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
            // TODO: Make this understand liquidations
            Classification::Unknown(ref mut calltrace) => {
                let call = calltrace.as_ref();
                if call.to == *AAVE_LENDING_POOL_CORE {
                    res = Some(Protocol::Aave);

                    // https://github.com/aave/aave-protocol/blob/master/contracts/lendingpool/LendingPool.sol#L805
                    if let Ok((collateral, reserve, user, purchase_amount, _)) =
                        self.pool
                            .decode::<LiquidationCall, _>("liquidationCall", &call.input)
                    {
                        *action = Classification::new(
                            Liquidation {
                                sent_token: reserve,
                                sent_amount: purchase_amount,

                                received_token: collateral,
                                received_amount: U256::zero(), // TODO: How to get the amount we received?
                                from: call.from,
                                liquidated_user: user,
                            },
                            calltrace.trace_address.clone(),
                        );
                    }
                }
            }
            Classification::Known(_) => {
                println!("Skipping already classified trace");
            }
            Classification::Prune => {
                println!("Skipping already pruned trace");
            }
        }

        res
    }
}
