#![allow(clippy::too_many_arguments)]
use crate::{
    addresses::CURVE_REGISTRY,
    traits::Inspector,
    types::{actions::AddLiquidity, Classification, Inspection, Protocol},
    DefiProtocol, ProtocolContracts,
};

use crate::model::{CallClassification, InternalCall};
use ethers::{
    abi::parse_abi,
    contract::{abigen, ContractError},
    contract::{decode_function_data, BaseContract},
    providers::Middleware,
    types::{Address, Call as TraceCall, U256},
};
use std::collections::HashMap;

// Type aliases for Curve
type Exchange = (u128, u128, U256, U256);

#[derive(Debug, Clone)]
/// An inspector for Curve
pub struct Curve {
    pool: BaseContract,
    pool4: BaseContract,
    pools: HashMap<Address, Vec<Address>>,
}

abigen!(
    CurveRegistry,
    "abi/curveregistry.json",
    methods {
        find_pool_for_coins(address,address,uint256) as find_pool_for_coins2;
    }
);
abigen!(CurvePool, "abi/curvepool.json");

impl DefiProtocol for Curve {
    fn base_contracts(&self) -> ProtocolContracts {
        ProtocolContracts::Dual(&self.pool, &self.pool4)
    }

    fn protocol() -> Protocol {
        Protocol::Curve
    }

    fn classify_call(&self, call: &InternalCall) -> Option<CallClassification> {
        self.as_add_liquidity(&call.to, &call.input)
            .map(|_| CallClassification::Liquidation)
    }
}

impl Inspector for Curve {
    fn inspect(&self, inspection: &mut Inspection) {
        let mut prune = Vec::new();
        for i in 0..inspection.actions.len() {
            let action = &mut inspection.actions[i];

            if let Some(calltrace) = action.as_call() {
                let call = calltrace.as_ref();
                if self.check(call) {
                    inspection.protocols.insert(Protocol::Curve);
                }

                if let Some(liquidity) = self.as_add_liquidity(&call.to, &call.input) {
                    *action = Classification::new(liquidity, calltrace.trace_address.clone());
                    prune.push(i);
                }
            }
        }

        let actions = inspection.actions.to_vec();
        prune
            .into_iter()
            .for_each(|idx| actions[idx].prune_subcalls(&mut inspection.actions));
        // TODO: Add checked calls
    }
}

impl Curve {
    /// Constructor
    pub fn new<T: IntoIterator<Item = (Address, Vec<Address>)>>(pools: T) -> Self {
        Self {
            pool: BaseContract::from(CURVEPOOL_ABI.clone()),
            pool4: parse_abi(&[
                "function add_liquidity(uint256[4] calldata amounts, uint256 deadline) external",
            ])
            .expect("could not parse curve 4-pool abi")
            .into(),
            pools: pools.into_iter().collect(),
        }
    }

    fn as_add_liquidity(&self, to: &Address, data: impl AsRef<[u8]>) -> Option<AddLiquidity> {
        let tokens = self.pools.get(to)?;
        // adapter for Curve's pool-specific abi decoding
        // TODO: Do we need to add the tripool?
        let amounts = match tokens.len() {
            2 => self
                .pool
                .decode::<([U256; 2], U256), _>("add_liquidity", data)
                .map(|x| x.0.to_vec()),
            4 => self
                .pool4
                .decode::<([U256; 4], U256), _>("add_liquidity", data)
                .map(|x| x.0.to_vec()),
            _ => return None,
        }
        .ok()?;

        Some(AddLiquidity {
            tokens: tokens.clone(),
            amounts,
        })
    }

    pub async fn create<M: Middleware>(
        provider: std::sync::Arc<M>,
    ) -> Result<Self, ContractError<M>> {
        let mut this = Self::new(vec![]);
        let registry = CurveRegistry::new(*CURVE_REGISTRY, provider);

        let pool_count = registry.pool_count().call().await?;
        // TODO: Cache these locally.
        for i in 0..pool_count.as_u64() {
            let pool = registry.pool_list(i.into()).call().await?;
            let tokens = registry.get_underlying_coins(pool).call().await?;
            this.pools.insert(pool, tokens.to_vec());
        }

        Ok(this)
    }

    fn check(&self, call: &TraceCall) -> bool {
        if !self.pools.is_empty() && self.pools.get(&call.to).is_none() {
            return false;
        }
        for function in self.pool.as_ref().functions() {
            // exchange & exchange_underlying
            if function.name.starts_with("exchange")
                && decode_function_data::<Exchange, _>(function, &call.input, true).is_ok()
            {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        inspectors::ERC20,
        reducers::{ArbitrageReducer, TradeReducer},
        test_helpers::read_trace,
        Reducer,
    };
    use ethers::providers::Provider;
    use std::convert::TryFrom;

    #[tokio::test]
    async fn instantiate() {
        let provider =
            Provider::try_from("https://mainnet.infura.io/v3/c60b0bb42f8a4c6481ecd229eddaca27")
                .unwrap();
        let curve = Curve::create(std::sync::Arc::new(provider)).await.unwrap();

        assert!(!curve.pools.is_empty());
    }

    struct MyInspector {
        inspector: Curve,
        erc20: ERC20,
        reducer1: TradeReducer,
        reducer2: ArbitrageReducer,
    }

    impl MyInspector {
        fn inspect(&self, inspection: &mut Inspection) {
            self.inspector.inspect(inspection);
            self.erc20.inspect(inspection);
            self.reducer1.reduce(inspection);
            self.reducer2.reduce(inspection);
            inspection.prune();
        }

        fn new() -> Self {
            Self {
                inspector: Curve::new(vec![]),
                erc20: ERC20::new(),
                reducer1: TradeReducer,
                reducer2: ArbitrageReducer::default(),
            }
        }
    }

    #[tokio::test]
    async fn simple_arb() {
        let mut inspection = read_trace("simple_curve_arb.json");
        let inspector = MyInspector::new();
        inspector.inspect(&mut inspection);

        let arb = inspection
            .known()
            .iter()
            .find_map(|x| x.as_ref().arbitrage())
            .cloned()
            .unwrap();
        assert_eq!(arb.profit.to_string(), "45259140804");
    }
}
