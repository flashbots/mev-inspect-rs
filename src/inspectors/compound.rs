use crate::{
    actions_after,
    addresses::{CETH, COMPTROLLER, COMP_ORACLE, WETH},
    traits::Inspector,
    types::{
        actions::{Liquidation, SpecificAction},
        Classification, Inspection, Protocol, Status,
    },
    DefiProtocol, ProtocolContracts,
};
use ethers::{
    abi::FunctionExt,
    contract::{abigen, BaseContract, ContractError},
    providers::Middleware,
    types::{Address, Call, CallType, U256},
};

use crate::model::{CallClassification, InternalCall};
use std::collections::HashMap;

type LiquidateBorrow = (Address, U256, Address);
type LiquidateBorrowEth = (Address, Address);
type SeizeInternal = (Address, Address, Address, U256);

abigen!(
    Comptroller,
    "abi/comptroller.json",
    methods {
        // TODO: Fix bug in ethers-rs so that we can rename them properly
        borrowGuardianPaused(address) as borrow_guardian_paused2;
        mintGuardianPaused(address) as mint_guardian_paused2;
        actionGuardianPaused(address) as action_paused2;
    },
);

abigen!(CToken, "abi/ctoken.json",);
abigen!(CEther, "abi/cether.json",);

#[derive(Debug, Clone)]
/// An inspector for Compound liquidations
pub struct Compound {
    ctoken: BaseContract,
    cether: BaseContract,
    comptroller: BaseContract,
    ctoken_to_token: HashMap<Address, Address>,
}

impl DefiProtocol for Compound {
    fn base_contracts(&self) -> ProtocolContracts {
        use std::borrow::Cow::Borrowed;
        ProtocolContracts::Multi(vec![
            Borrowed(&self.ctoken),
            Borrowed(&self.cether),
            Borrowed(&self.comptroller),
        ])
    }

    fn protocol() -> Protocol {
        Protocol::Compound
    }

    fn classify_call(&self, call: &InternalCall) -> Option<CallClassification> {
        self.cether
            .decode::<LiquidateBorrowEth, _>("liquidateBorrow", &call.input)
            .map(|_| CallClassification::Liquidation)
            .or_else(|_| {
                self.ctoken
                    .decode::<LiquidateBorrow, _>("liquidateBorrow", &call.input)
                    .map(|_| CallClassification::Liquidation)
            })
            .ok()
    }
}

impl Inspector for Compound {
    fn inspect(&self, inspection: &mut Inspection) {
        let mut found = false;
        for i in 0..inspection.actions.len() {
            // split in two so that we can iterate mutably without cloning
            let (action, subtraces) = actions_after(&mut inspection.actions, i);

            // if the provided action is a liquidation, start parsing all the subtraces
            if let Some((mut liquidation, trace)) = self.try_as_liquidation(&action) {
                inspection.protocols.insert(Protocol::Compound);

                // omit the double-counted Dcall
                if let Some(ref call_type) = action.as_call().map(|call| &call.call.call_type) {
                    if matches!(call_type, CallType::DelegateCall) {
                        continue;
                    }
                }

                // once we find the `seize` call, parse it
                if let Some(seized) = subtraces.iter().find_map(|seize| self.try_as_seize(seize)) {
                    liquidation.received_amount = seized.2;

                    *action = Classification::new(liquidation, trace);
                    if inspection.status != Status::Reverted {
                        inspection.status = Status::Success;
                    }
                    found = true;
                }
            } else if self.is_preflight(&action) && !found {
                // insert an empty liquidation for the actions upstream
                *action = Classification::new(SpecificAction::LiquidationCheck, Vec::new());
                // a pre-flight is only marked as "Checked" if a successful
                // liquidation was not already found before it
                inspection.status = Status::Checked;
            }
        }
    }
}

impl Compound {
    /// Constructor
    pub fn new<T: IntoIterator<Item = (Address, Address)>>(ctoken_to_token: T) -> Self {
        Self {
            ctoken: BaseContract::from(CTOKEN_ABI.clone()),
            cether: BaseContract::from(CETHER_ABI.clone()),
            comptroller: BaseContract::from(COMPTROLLER_ABI.clone()),
            ctoken_to_token: ctoken_to_token.into_iter().collect(),
        }
    }

    /// Instantiates Compound with all live markets
    ///
    /// # Panics
    ///
    /// - If the `Ctoken.underlying` call fails
    pub async fn create<M: Middleware>(
        provider: std::sync::Arc<M>,
    ) -> Result<Self, ContractError<M>> {
        let comptroller = Comptroller::new(*COMPTROLLER, provider.clone());

        let markets = comptroller.get_all_markets().call().await?;
        let futs = markets
            .into_iter()
            .map(|market| {
                let provider = provider.clone();
                async move {
                    if market != *CETH {
                        (
                            market,
                            CToken::new(market, provider)
                                .underlying()
                                .call()
                                .await
                                .expect("could not get underlying"),
                        )
                    } else {
                        (market, *WETH)
                    }
                }
            })
            .collect::<Vec<_>>();
        let res = futures::future::join_all(futs).await;

        Ok(Compound::new(res))
    }

    /// Find the liquidation action
    fn try_as_liquidation(&self, action: &Classification) -> Option<(Liquidation, Vec<usize>)> {
        match action {
            Classification::Unknown(ref calltrace) => {
                let call = calltrace.as_ref();
                if let Ok((liquidated_user, repaid_amount, ctoken_collateral)) =
                    self.ctoken
                        .decode::<LiquidateBorrow, _>("liquidateBorrow", &call.input)
                {
                    Some((
                        Liquidation {
                            sent_token: *self.underlying(&call.to),
                            sent_amount: repaid_amount,

                            received_token: ctoken_collateral,
                            received_amount: 0.into(),

                            from: call.from,
                            liquidated_user,
                        },
                        calltrace.trace_address.clone(),
                    ))
                } else if let Ok((liquidated_user, ctoken_collateral)) =
                    self.cether
                        .decode::<LiquidateBorrowEth, _>("liquidateBorrow", &call.input)
                {
                    Some((
                        Liquidation {
                            sent_token: *self.underlying(&call.to),
                            sent_amount: call.value,

                            received_token: ctoken_collateral,
                            received_amount: 0.into(),

                            from: call.from,
                            liquidated_user,
                        },
                        calltrace.trace_address.clone(),
                    ))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    // Parses a subtrace
    fn try_as_seize(&self, call: &Call) -> Option<(Address, Address, U256)> {
        if let Ok((_seizertoken, liquidator, borrower, seizetokens)) = self
            .ctoken
            .decode::<SeizeInternal, _>("seizeInternal", &call.input)
        {
            Some((borrower, liquidator, seizetokens))
        } else if let Ok((liquidator, borrower, seizetokens)) = self
            .ctoken
            .decode::<(Address, Address, U256), _>("seize", &call.input)
        {
            Some((borrower, liquidator, seizetokens))
        } else {
            None
        }
    }

    fn is_preflight(&self, action: &Classification) -> bool {
        match action {
            Classification::Unknown(ref calltrace) => {
                let call = calltrace.as_ref();
                // checks if liquidation is allowed
                call.to == *COMPTROLLER && call.input.as_ref().starts_with(&self.comptroller.as_ref().function("liquidateBorrowAllowed").unwrap().selector()) ||
                    // checks oracle price
                    call.to == *COMP_ORACLE && call.input.as_ref().starts_with(&ethers::utils::id("getUnderlyingPrice(address)"))
            }
            _ => false,
        }
    }

    // helper for converting cToken to Token address.
    // TODO: Should this also include decimals? Or should we assume that
    // cTokens always use 8 decimals
    fn underlying<'a>(&'a self, address: &'a Address) -> &'a Address {
        if let Some(inner) = self.ctoken_to_token.get(address) {
            inner
        } else {
            &address
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        addresses::{parse_address, ADDRESSBOOK},
        test_helpers::*,
        types::Status,
        Inspector,
    };
    use ethers::providers::Provider;
    use std::convert::TryFrom;

    #[test]
    // https://etherscan.io/tx/0xb7ba825294f757f8b8b6303b2aef542bcaebc9cc0217ddfaf822200a00594ed9
    fn liquidate() {
        let mut inspection = read_trace("compound_liquidation.json");
        let ctoken_to_token = vec![(
            parse_address("0xb3319f5d18bc0d84dd1b4825dcde5d5f7266d407"),
            parse_address("0xe41d2489571d322189246dafa5ebde1f4699f498"),
        )];
        let compound = Compound::new(ctoken_to_token);
        compound.inspect(&mut inspection);

        let liquidation = inspection
            .known()
            .iter()
            .find_map(|x| x.as_ref().as_liquidation())
            .cloned()
            .unwrap();

        assert_eq!(ADDRESSBOOK.get(&liquidation.sent_token).unwrap(), "ZRX");
        // cETH has 8 decimals
        assert_eq!(liquidation.received_amount, 5250648.into());
        // ZRX has 18 decimals
        assert_eq!(liquidation.sent_amount, 653800000000000000u64.into());

        assert_eq!(inspection.protocols, crate::set![Protocol::Compound]);
        assert_eq!(inspection.status, Status::Success);
    }

    #[tokio::test]
    async fn instantiate() {
        let provider =
            Provider::try_from("https://mainnet.infura.io/v3/c60b0bb42f8a4c6481ecd229eddaca27")
                .unwrap();
        let compound = Compound::create(std::sync::Arc::new(provider))
            .await
            .unwrap();

        // cZRX -> ZRX
        assert_eq!(
            *compound.underlying(&parse_address("0xb3319f5d18bc0d84dd1b4825dcde5d5f7266d407")),
            parse_address("0xe41d2489571d322189246dafa5ebde1f4699f498"),
        );
    }
}
