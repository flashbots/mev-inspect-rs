use ethers::{
    contract::{abigen, BaseContract, EthLogDecode},
    types::{Address, U256},
};

use crate::model::{CallClassification, EventLog, InternalCall};
use crate::{
    addresses::AAVE_LENDING_POOL,
    types::{actions::Liquidation, Classification, Inspection, Protocol},
    DefiProtocol, Inspector, ProtocolContracts,
};

// https://github.com/aave/aave-protocol/blob/master/contracts/lendingpool/LendingPool.sol

/// _collateral, reserve, user, _purchaseAmount, _receiveAToken
type LiquidationCall = (Address, Address, Address, U256, bool);
/// reserve, amount, _referralCode
type DepositCall = (Address, U256, u16);
/// _reserve, amount, onbehalfof
type RepayCall = (Address, U256, Address);
/// reserve, amount, interestRateMode, referralcode
type BorrowCall = (Address, U256, U256, u16);

abigen!(AavePool, "abi/aavepool.json");
#[derive(Clone, Debug)]
pub struct Aave {
    pub pool: BaseContract,
}

impl Aave {
    pub fn new() -> Self {
        Aave {
            pool: BaseContract::from(AAVEPOOL_ABI.clone()),
        }
    }
}

impl DefiProtocol for Aave {
    fn base_contracts(&self) -> ProtocolContracts {
        ProtocolContracts::Single(&self.pool)
    }

    fn protocol() -> Protocol {
        Protocol::Aave
    }

    fn is_protocol(&self, to: &Address) -> Option<bool> {
        Some(*to == *AAVE_LENDING_POOL)
    }

    fn is_protocol_event(&self, log: &EventLog) -> bool {
        AavePoolEvents::decode_log(&log.raw_log).is_ok()
    }

    fn classify_call(&self, call: &InternalCall) -> Option<CallClassification> {
        if self
            .pool
            .decode::<LiquidationCall, _>("liquidationCall", &call.input)
            .is_ok()
        {
            Some(CallClassification::Liquidation)
        } else if self
            .pool
            .decode::<DepositCall, _>("deposit", &call.input)
            .is_ok()
        {
            Some(CallClassification::Deposit)
        } else if self
            .pool
            .decode::<RepayCall, _>("repay", &call.input)
            .is_ok()
        {
            Some(CallClassification::Repay)
        } else if self
            .pool
            .decode::<BorrowCall, _>("borrow", &call.input)
            .is_ok()
        {
            Some(CallClassification::Borrow)
        } else {
            None
        }
    }
}

impl Inspector for Aave {
    fn inspect(&self, inspection: &mut Inspection) {
        for action in inspection.actions.iter_mut() {
            match action {
                Classification::Unknown(ref mut calltrace) => {
                    let call = calltrace.as_ref();
                    if call.to == *AAVE_LENDING_POOL {
                        inspection.protocols.insert(Protocol::Aave);

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
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        inspectors::ERC20, reducers::LiquidationReducer, test_helpers::read_trace, Reducer,
    };

    use super::*;

    struct MyInspector {
        aave: Aave,
        erc20: ERC20,
        reducer: LiquidationReducer,
    }

    impl MyInspector {
        fn inspect(&self, inspection: &mut Inspection) {
            self.aave.inspect(inspection);
            self.erc20.inspect(inspection);
            self.reducer.reduce(inspection);
            inspection.prune();
        }

        fn new() -> Self {
            Self {
                aave: Aave::new(),
                erc20: ERC20::new(),
                reducer: LiquidationReducer::new(),
            }
        }
    }

    #[tokio::test]
    async fn simple_liquidation() {
        let mut inspection = read_trace("simple_liquidation.json");
        let aave = MyInspector::new();
        aave.inspect(&mut inspection);

        let liquidation = inspection
            .known()
            .iter()
            .find_map(|x| x.as_ref().as_liquidation())
            .cloned()
            .unwrap();

        assert_eq!(
            liquidation.sent_amount.to_string(),
            "11558317402311470764075"
        );
        assert_eq!(
            liquidation.received_amount.to_string(),
            "1100830609991235507621"
        );
    }
}
