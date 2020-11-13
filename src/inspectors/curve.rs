use crate::{
    addresses::CURVE_REGISTRY,
    traits::Inspector,
    types::{Inspection, Protocol},
};

use ethers::{abi::Abi, contract::BaseContract};
use ethers::{
    contract::decode_fn as abi_decode,
    contract::{abigen, ContractError},
    providers::Middleware,
    types::{Address, Call as TraceCall, U256},
};
use std::collections::HashSet;

// Type aliases for Curve
type Exchange = (u128, u128, U256, U256);

#[derive(Debug, Clone)]
/// An inspector for Curve
pub struct Curve {
    pool: BaseContract,
    pools: HashSet<Address>,
}

abigen!(
    CurveRegistry,
    "abi/curveregistry.json",
    methods {
        find_pool_for_coins(address,address,uint256) as find_pool_for_coins2;
    }
);

impl Inspector for Curve {
    fn inspect(&self, inspection: &mut Inspection) {
        for i in 0..inspection.actions.len() {
            let action = &mut inspection.actions[i];

            if let Some(calltrace) = action.to_call() {
                if self.is_curve_call(calltrace.as_ref())
                    && !inspection.protocols.contains(&Protocol::Curve)
                {
                    inspection.protocols.push(Protocol::Curve);
                }
            }
        }
        // TODO: Add checked calls
    }
}

impl Curve {
    /// Constructor
    pub fn new() -> Self {
        Self {
            pool: BaseContract::from({
                serde_json::from_str::<Abi>(include_str!("../../abi/curvepool.json"))
                    .expect("could not parse uniswap abi")
            }),
            pools: HashSet::new(),
        }
    }

    pub async fn create<M: Middleware>(
        provider: std::sync::Arc<M>,
    ) -> Result<Self, ContractError<M>> {
        let mut this = Self::new();
        let registry = CurveRegistry::new(*CURVE_REGISTRY, provider);

        let pool_count = registry.pool_count().call().await?;
        for i in 0..pool_count.as_u64() {
            this.pools
                .insert(registry.pool_list(i as i128).call().await?);
        }

        Ok(this)
    }

    fn is_curve_call(&self, call: &TraceCall) -> bool {
        if !self.pools.is_empty() && self.pools.get(&call.to).is_none() {
            return false;
        }
        for function in self.pool.as_ref().functions() {
            // exchange & exchange_underlying
            if function.name.starts_with("exchange")
                && abi_decode::<Exchange, _>(function, &call.input, true).is_ok()
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
    use ethers::providers::Provider;
    use std::convert::TryFrom;

    #[tokio::test]
    async fn instantiate() {
        let provider =
            Provider::try_from("https://mainnet.infura.io/v3/c60b0bb42f8a4c6481ecd229eddaca27")
                .unwrap();
        let curve = Curve::create(std::sync::Arc::new(provider)).await.unwrap();

        assert_eq!(curve.pools.len(), 8);
    }
}
