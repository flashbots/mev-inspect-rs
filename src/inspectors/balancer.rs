use crate::{
    addresses::BALANCER_PROXY,
    inspectors::find_matching,
    traits::Inspector,
    types::{actions::Trade, Classification, Inspection, Protocol},
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
        let actions = inspection.actions.to_vec();
        let mut prune = Vec::new();
        for i in 0..inspection.actions.len() {
            let action = &mut inspection.actions[i];

            if let Some(calltrace) = action.to_call() {
                let call = calltrace.as_ref();
                let (token_in, _, token_out, _, _) = if let Ok(inner) = self
                    .bpool
                    .decode::<Swap, _>("swapExactAmountIn", &call.input)
                {
                    inner
                } else if let Ok(inner) = self
                    .bpool
                    .decode::<Swap, _>("swapExactAmountOut", &call.input)
                {
                    inner
                } else {
                    if self.check(calltrace.as_ref())
                        && !inspection.protocols.contains(&Protocol::Balancer)
                    {
                        inspection.protocols.push(Protocol::Balancer);
                    }
                    continue;
                };

                // In Balancer, the 2 subtraces of the `swap*` call are the transfers
                // In both cases, the in asset is being transferred _to_ the pair,
                // and the out asset is transferred _from_ the pair
                let t1 = find_matching(
                    actions.iter().enumerate().skip(i + 1),
                    |t| t.transfer(),
                    |t| t.token == token_in,
                    true,
                );

                let t2 = find_matching(
                    actions.iter().enumerate().skip(i + 1),
                    |t| t.transfer(),
                    |t| t.token == token_out,
                    true,
                );

                match (t1, t2) {
                    (Some((j, t1)), Some((k, t2))) => {
                        if t1.from != t2.to {
                            continue;
                        }

                        *action =
                            Classification::new(Trade::new(t1.clone(), t2.clone()), Vec::new());
                        prune.push(j);
                        prune.push(k);

                        if !inspection.protocols.contains(&Protocol::Balancer) {
                            inspection.protocols.push(Protocol::Balancer);
                        }
                    }
                    _ => {}
                };
            }
        }

        prune
            .iter()
            .for_each(|p| inspection.actions[*p] = Classification::Prune);
        // TODO: Add checked calls
    }
}

impl Balancer {
    fn check(&self, call: &TraceCall) -> bool {
        // TODO: Adjust for exchange proxy calls
        call.to == *BALANCER_PROXY
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;
    use crate::{
        inspectors::ERC20,
        reducers::{ArbitrageReducer, TradeReducer},
        types::Inspection,
        Inspector, Reducer,
    };

    struct MyInspector {
        erc20: ERC20,
        balancer: Balancer,
        trade: TradeReducer,
        arb: ArbitrageReducer,
    }

    impl MyInspector {
        fn inspect(&self, inspection: &mut Inspection) {
            self.erc20.inspect(inspection);
            self.balancer.inspect(inspection);
            self.trade.reduce(inspection);
            self.arb.reduce(inspection);
            inspection.prune();
        }

        fn new() -> Self {
            Self {
                erc20: ERC20::new(),
                balancer: Balancer::new(),
                trade: TradeReducer::new(),
                arb: ArbitrageReducer::new(),
            }
        }
    }

    #[test]
    fn bot_trade() {
        let mut inspection = read_trace("balancer_trade.json");
        let bal = MyInspector::new();
        bal.inspect(&mut inspection);

        let known = inspection.known();
        dbg!(&known);
        assert_eq!(known.len(), 4);
        let t1 = known[0].as_ref().transfer().unwrap();
        assert_eq!(
            t1.amount,
            U256::from_dec_str("134194492674651541324").unwrap()
        );
        let trade = known[1].as_ref().trade().unwrap();
        assert_eq!(
            trade.t1.amount,
            U256::from_dec_str("7459963749616500736").unwrap()
        );
        let _t2 = known[2].as_ref().transfer().unwrap();
        let _t3 = known[3].as_ref().transfer().unwrap();
    }
}
