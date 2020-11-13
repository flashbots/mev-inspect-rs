use crate::{
    addresses::BALANCER_PROXY,
    traits::Inspector,
    types::{Inspection, Protocol},
};

use ethers::{
    abi::Abi,
    contract::BaseContract,
    types::{Address, Call as TraceCall, U256},
};

#[derive(Debug, Clone)]
/// An inspector for Uniswap
pub struct Balancer {
    bpool: BaseContract,
    bproxy: BaseContract,
}

type Swap = (Address, U256, Address, U256, U256);

impl Inspector for Balancer {
    fn inspect(&self, inspection: &mut Inspection) {
        for i in 0..inspection.actions.len() {
            let action = &mut inspection.actions[i];

            if let Some(calltrace) = action.to_call() {
                if self.check(calltrace.as_ref())
                    && !inspection.protocols.contains(&Protocol::Balancer)
                {
                    inspection.protocols.push(Protocol::Balancer);
                }
            }
        }
        // TODO: Add checked calls
    }
}

impl Balancer {
    fn check(&self, call: &TraceCall) -> bool {
        if self
            .bpool
            .decode::<Swap, _>("swapExactAmountIn", &call.input)
            .is_ok()
            || self
                .bpool
                .decode::<Swap, _>("swapExactAmountOut", &call.input)
                .is_ok()
        {
            return true;
        } else {
            // TODO: Adjust for exchange proxy calls
            call.to == *BALANCER_PROXY
        }
    }

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
