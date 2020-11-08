use super::{
    addresses::ETH,
    types::{is_subtrace, Liquidation, Protocol, SpecificAction},
    Classification, Inspection, Inspector, Reducer,
};
use ethers::{
    abi::Abi,
    contract::BaseContract,
    types::{Address, U256},
};

pub struct Aave {
    pub pool: BaseContract,
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
    // Prunes a liquidation's subtraces and also populates the `received_amount`
    // field
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

        // Remove the required subtraces
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

type LiquidationCall = (Address, Address, Address, U256, bool);

impl Aave {
    fn inspect_one(&self, action: &mut Classification) -> Option<Protocol> {
        let mut res = None;
        match action {
            // TODO: Make this understand liquidations
            Classification::Unknown(ref mut calltrace) => {
                let call = calltrace.as_ref();
                // Cull by setting the action to Prune
                // Lending pool core 0x3dfd23A6c5E8BbcFc9581d2E864a68feb6a076d3
                // Lending pool 0x398eC7346DcD622eDc5ae82352F02bE94C62d119
                if call.to
                    == "398eC7346DcD622eDc5ae82352F02bE94C62d119"
                        .parse::<Address>()
                        .unwrap()
                {
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
