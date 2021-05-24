use ethers::{
    contract::{abigen, BaseContract, EthLogDecode},
    types::{Address, U256},
};

use crate::model::{CallClassification, EventLog, InternalCall};
use crate::types::actions::{SpecificAction, TokenDeposit};
use crate::types::{Action, TransactionData};
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

    fn protocol(&self) -> Protocol {
        Protocol::Aave
    }

    fn is_protocol(&self, call: &InternalCall) -> Option<Option<Protocol>> {
        if call.to == *AAVE_LENDING_POOL {
            Some(Some(self.protocol()))
        } else {
            None
        }
    }

    fn is_protocol_event(&self, log: &EventLog) -> bool {
        AavePoolEvents::decode_log(&log.raw_log).is_ok()
    }

    fn decode_call_action(&self, call: &InternalCall, tx: &TransactionData) -> Option<Action> {
        dbg!(call.classification);
        match call.classification {
            CallClassification::Liquidation => {
                // eventually emitted by the liquidation manager
                // https://github.com/aave/aave-protocol/blob/master/contracts/lendingpool/LendingPoolLiquidationManager.sol#L279
                if let Some((_, log, liquidation)) = tx
                    .call_logs_decoded::<LiquidationCallFilter>(&call.trace_address)
                    .next()
                {
                    let action = Liquidation {
                        sent_token: liquidation.reserve,
                        sent_amount: liquidation.purchase_amount,
                        received_token: liquidation.collateral,
                        received_amount: liquidation.liquidated_collateral_amount,
                        from: call.from,
                        liquidated_user: liquidation.user,
                    };
                    return Some(Action::with_logs(
                        action.into(),
                        call.trace_address.clone(),
                        vec![log.log_index],
                    ));
                }
            }
            CallClassification::Deposit => {
                if let Some((_, log, deposit)) = tx
                    .call_logs_decoded::<DepositFilter>(&call.trace_address)
                    .next()
                {
                    let action = TokenDeposit {
                        token: deposit.reserve,
                        from: deposit.user,
                        amount: deposit.amount,
                    };
                    return Some(Action::with_logs(
                        action.into(),
                        call.trace_address.clone(),
                        vec![log.log_index],
                    ));
                }
            }
            _ => {}
        };
        None
    }

    fn classify(
        &self,
        call: &InternalCall,
    ) -> Option<(CallClassification, Option<SpecificAction>)> {
        if self
            .pool
            .decode::<LiquidationCall, _>("liquidationCall", &call.input)
            .is_ok()
        {
            // https://github.com/aave/aave-protocol/blob/master/contracts/lendingpool/LendingPool.sol#L215
            Some((CallClassification::Liquidation, None))
        } else if self
            .pool
            .decode::<DepositCall, _>("deposit", &call.input)
            .is_ok()
        {
            // will fire an event https://github.com/aave/aave-protocol/blob/master/contracts/lendingpool/LendingPool.sol#L46
            Some((CallClassification::Deposit, None))
        } else if self
            .pool
            .decode::<RepayCall, _>("repay", &call.input)
            .is_ok()
        {
            // https://github.com/aave/aave-protocol/blob/master/contracts/lendingpool/LendingPool.sol#L102
            Some((CallClassification::Repay, None))
        } else if self
            .pool
            .decode::<BorrowCall, _>("borrow", &call.input)
            .is_ok()
        {
            // https://github.com/aave/aave-protocol/blob/master/contracts/lendingpool/LendingPool.sol#L80
            Some((CallClassification::Borrow, None))
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
        TxReducer,
    };

    use super::*;
    use crate::test_helpers::read_tx;

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

        fn inspect_tx(&self, tx: &mut TransactionData) {
            self.aave.inspect_tx(tx);
            self.erc20.inspect_tx(tx);
            self.reducer.reduce_tx(tx);
        }

        fn new() -> Self {
            Self {
                aave: Aave::new(),
                erc20: ERC20::new(),
                reducer: LiquidationReducer,
            }
        }
    }

    #[test]
    fn simple_liquidation2() {
        let mut tx = read_tx("simple_liquidation.data.json");
        let aave = MyInspector::new();
        aave.inspect_tx(&mut tx);

        let liquidation = tx.actions().liquidations().next().unwrap();

        assert_eq!(
            liquidation.sent_amount.to_string(),
            "11558317402311470764075"
        );
        assert_eq!(
            liquidation.received_amount.to_string(),
            "1100830609991235507621"
        );
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
