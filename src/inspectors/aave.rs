use crate::{
    addresses::AAVE_LENDING_POOL,
    types::{actions::Liquidation, Classification, Inspection, Protocol},
    Inspector,
};
use ethers::{
    abi::Abi,
    contract::BaseContract,
    types::{Address, U256},
};

type LiquidationCall = (Address, Address, Address, U256, bool);

#[derive(Clone, Debug)]
pub struct Aave {
    pub pool: BaseContract,
}

impl Aave {
    pub fn new() -> Self {
        Aave {
            pool: BaseContract::from({
                serde_json::from_str::<Abi>(include_str!("../../abi/aavepool.json"))
                    .expect("could not parse aave abi")
            }),
        }
    }
}

impl Inspector for Aave {
    fn inspect(&self, inspection: &mut Inspection) {
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
