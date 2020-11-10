use crate::{
    inspectors::{ArbitrageReducer, ERC20},
    traits::{Inspector, Reducer},
    types::Inspection,
};

use ethers::{abi::Abi, contract::BaseContract};

// mod inspector;
// mod reducer;

#[derive(Debug, Clone)]
/// An inspector for Uniswap
pub struct Balancer {
    erc20: ERC20,
    bpool: BaseContract,
    bproxy: BaseContract,
    arb: ArbitrageReducer,
}

impl Balancer {
    /// Constructor
    pub fn new() -> Self {
        Self {
            erc20: ERC20::new(),
            bpool: BaseContract::from({
                serde_json::from_str::<Abi>(include_str!("../../../abi/bpool.json"))
                    .expect("could not parse uniswap abi")
            }),
            bproxy: BaseContract::from({
                serde_json::from_str::<Abi>(include_str!("../../../abi/bproxy.json"))
                    .expect("could not parse uniswap abi")
            }),
            arb: ArbitrageReducer,
        }
    }
}
