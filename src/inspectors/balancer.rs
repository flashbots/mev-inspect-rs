use crate::{
    traits::{Inspector},
    types::Inspection,
};

use ethers::{abi::Abi, contract::BaseContract};

#[derive(Debug, Clone)]
/// An inspector for Uniswap
pub struct Balancer {
    bpool: BaseContract,
    bproxy: BaseContract,
}

impl Balancer {
    /// Constructor
    pub fn new() -> Self {
        Self {
            bpool: BaseContract::from({
                serde_json::from_str::<Abi>(include_str!("../../abi/bpool.json"))
                    .expect("could not parse uniswap abi")
            }),
            bproxy: BaseContract::from({
                serde_json::from_str::<Abi>(include_str!("../../abi/bproxy.json"))
                    .expect("could not parse uniswap abi")
            }),
        }
    }
}
