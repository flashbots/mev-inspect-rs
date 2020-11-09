use crate::{
    addresses::UNISWAP,
    inspectors::{ArbitrageReducer, ERC20},
    traits::{Inspector, Reducer},
    types::{
        actions::{SpecificAction, Trade},
        Classification, Inspection, Protocol,
    },
};

use ethers::{
    abi::Abi,
    contract::{decode_fn as abi_decode, BaseContract},
    types::{Address, Bytes, Call as TraceCall, U256},
};

// Type aliases for Uniswap's `swap` return types
type SwapTokensFor = (U256, U256, Vec<Address>, Address, U256);
type SwapEthFor = (U256, Vec<Address>, Address, U256);
type PairSwap = (U256, U256, Address, Bytes);

#[derive(Debug, Clone)]
/// An inspector for Uniswap
pub struct Uniswap {
    erc20: ERC20,
    router: BaseContract,
    pair: BaseContract,
    arb: ArbitrageReducer,
}

use itertools::{EitherOrBoth::*, Itertools};

impl Inspector for Uniswap {
    fn classify(&self, inspection: &mut Inspection) {
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

impl Reducer for Uniswap {
    /// Combines consecutive `Transfer`s into `Trade`s
    fn reduce(&self, inspection: &mut Inspection) {
        use Classification::Known;
        // TODO: Can we filter the zip without cloning?
        let actions = inspection.actions.clone();

        let mut prune = 0;
        inspection
            .actions
            .iter_mut()
            .enumerate()
            .zip_longest(actions.iter().skip(1))
            .for_each(|mut pair| match pair {
                Both((i, ref mut a1), ref mut a2) => {
                    // prune elements which have been reduced already
                    // TODO: Figure out how to do this properly.
                    if i > 0 && i == prune {
                        **a1 = Classification::Prune;
                        return;
                    }

                    match (&a1, &a2) {
                        (Known(ref b1), Known(ref b2)) => match (&b1.as_ref(), &b2.as_ref()) {
                            (SpecificAction::Transfer(t1), SpecificAction::Transfer(t2)) => {
                                // Hack to filter out weird dups
                                if t1.token == t2.token {
                                    **a1 = Classification::Prune;
                                    return;
                                }

                                if t1.to == t2.from {
                                    let action = Trade {
                                        t1: t1.clone(),
                                        t2: t2.clone(),
                                    };

                                    // TODO: Figure out what the trace should be here
                                    **a1 = Classification::new(action, Vec::new());

                                    // Since we paired with the next element,
                                    // we should prune it, if the element after the
                                    // next does not also execute a matching trade
                                    if let Some(actions) = actions.get(i + 2) {
                                        match &actions {
                                            Known(ref inner) => match &inner.as_ref() {
                                                SpecificAction::Transfer(t3) => {
                                                    if t2.to == t3.from {
                                                        prune = 0;
                                                        return;
                                                    }
                                                }
                                                _ => (),
                                            },
                                            _ => (),
                                        };
                                    }

                                    prune = i + 1;
                                }
                            }
                            // TODO: Is there a way to avoid doing these _ => () branches?
                            _ => (),
                        },
                        _ => (),
                    };
                }
                Left((i, ref mut a1)) => {
                    if i > 0 && i == prune {
                        **a1 = Classification::Prune;
                        return;
                    }
                }
                Right(_) => (),
            });

        // Once trades have been combined, we can go ahead and collapse the ones
        // which are arbitrages
        self.arb.reduce(inspection);
    }
}

impl Uniswap {
    /// Constructor
    pub fn new() -> Self {
        Self {
            erc20: ERC20::new(),
            router: BaseContract::from({
                serde_json::from_str::<Abi>(include_str!("../../abi/unirouterv2.json"))
                    .expect("could not parse uniswap abi")
            }),
            pair: BaseContract::from({
                serde_json::from_str::<Abi>(include_str!("../../abi/unipair.json"))
                    .expect("could not parse uniswap abi")
            }),
            arb: ArbitrageReducer,
        }
    }

    /// Inspects the call, tries to classify it, and maybe returns the protocol type
    /// that the tx was a part of
    fn inspect_one(&self, action: &mut Classification) -> Option<Protocol> {
        let mut res = None;
        match action {
            Classification::Unknown(ref mut calltrace) => {
                let call = calltrace.as_ref();
                if let Some(transfer) = self.erc20.parse(call) {
                    *action = Classification::new(transfer, calltrace.trace_address.clone());
                } else {
                    // If we could not parse the call as an ERC20 but
                    // it is targeting a Uni address, we can prune
                    // that trace
                    let protocol = self.is_uni_call(&call);
                    if protocol.is_some() {
                        *action = Classification::Prune;
                        res = protocol;
                    }
                }
            }
            Classification::Known(_) | Classification::Prune => {
                println!("Skipping already classified / pruned trace");
            }
        }

        res
    }

    // There MUST be 1 `swap` call in the traces either to the Pair directly
    // or to the router
    #[allow(unused, clippy::collapsible_if)]
    fn is_uni_call(&self, call: &TraceCall) -> Option<Protocol> {
        let mut res = None;
        // Check the call is a `swap`
        let uniswappy = UNISWAP.get(&call.to);

        if let Ok((amount0, amount1, to, data)) =
            self.pair.decode::<PairSwap, _>("swap", &call.input)
        {
            // if the data field is not empty, then there is a flashloan
            if data.as_ref().len() > 0 {
                res = Some(Protocol::Flashloan);
            } else {
                res = Some(*uniswappy.unwrap_or(&Protocol::Uniswap));
            }
        } else {
            // if there's no swap, maybe there are `swap*` calls to the router
            if let Some(&uniswappy) = uniswappy {
                for function in self.router.as_ref().functions() {
                    if function.name.starts_with("swapETH")
                        || function.name.starts_with("swapExactETH")
                    {
                        if abi_decode::<SwapEthFor, _>(function, &call.input, true).is_ok() {
                            res = Some(uniswappy)
                        }
                    } else if function.name.starts_with("swap") {
                        if abi_decode::<SwapTokensFor, _>(function, &call.input, true).is_ok() {
                            res = Some(uniswappy)
                        }
                    }
                }
            }
        }
        res
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{addresses::ADDRESSBOOK, types::Status};

    use crate::test_helpers::*;

    #[test]
    // https://etherscan.io/tx/0x123d03cef9ccd4230d111d01cf1785aed4242eb2e1e542bd792d025eb7e3cc84/advanced#internal
    fn parse_failing() {
        let mut inspection =
            get_trace("123d03cef9ccd4230d111d01cf1785aed4242eb2e1e542bd792d025eb7e3cc84");
        let uni = Uniswap::new();
        uni.inspect(&mut inspection);
        assert_eq!(inspection.status, Status::Reverted);
        assert_eq!(
            ADDRESSBOOK
                .get(&to_transfer(&inspection.actions[0]).token)
                .unwrap(),
            "ETH"
        );
    }

    mod arbitrages {
        use super::*;

        #[test]
        // https://etherscan.io/tx/0xd9306dc8c1230cc0faef22a8442d0994b8fc9a8f4c9faeab94a9a7eac8e59710
        // This trace does not use the Routers, instead it goes directly to the YFI pair contracts
        fn parse_uni_sushi_arb() {
            let mut inspection =
                get_trace("0xd9306dc8c1230cc0faef22a8442d0994b8fc9a8f4c9faeab94a9a7eac8e59710");
            let uni = Uniswap::new();
            dbg!(&inspection);
            uni.inspect(&mut inspection);
            dbg!(&inspection);

            assert_eq!(inspection.known().len(), 1);
            let arb = to_arb(&inspection.actions[2]);
            assert!(arb.profit == U256::from_dec_str("626678385524850545").unwrap());

            // the initial call and the delegate call
            assert_eq!(inspection.unknown().len(), 2);
            assert_eq!(
                inspection.protocols,
                vec![Protocol::Uniswap, Protocol::Sushiswap]
            );
        }

        // https://etherscan.io/tx/0xdfeae07360e2d7695a498e57e2054c658d1d78bbcd3c763fc8888b5433b6c6d5
        //
        // TODO: Add function: "Get next trade"

        #[test]
        fn xsp_xfi_eth_arb() {
            let mut inspection =
                get_trace("0xdfeae07360e2d7695a498e57e2054c658d1d78bbcd3c763fc8888b5433b6c6d5");
            let uni = Uniswap::new();

            dbg!(&inspection);
            uni.inspect(&mut inspection);
            dbg!(&inspection);

            // let arb = to_arb(&inspection.actions[0]);
            // assert!(arb.profit > 0.into());

            // // 4 swaps loop and the withdrawal
            // assert_eq!(inspection.known().len(), 1);
            // assert_eq!(inspection.unknown().len(), 0);
        }

        // https://etherscan.io/tx/0xddbf97f758bd0958487e18d9e307cd1256b1ad6763cd34090f4c9720ba1b4acc
        #[test]
        fn triangular_router_arb() {
            let mut inspection = read_trace("triangular_arb.json");
            let uni = Uniswap::new();

            uni.inspect(&mut inspection);
            let arb = to_arb(&inspection.actions[0]);
            assert_eq!(arb.profit, U256::from_dec_str("9196963592118237").unwrap());

            // 4 swaps loop and the withdrawal
            assert_eq!(inspection.known().len(), 1);
            assert_eq!(inspection.unknown().len(), 0);
        }
    }

    mod simple_trades {
        use super::*;

        #[test]
        // https://etherscan.io/tx/0x46909832db6ca33317c43436c76eef4b654d7f9cbc5e64cf47079aa7ea8be845/advanced#internal
        fn parse_eth_for_exact_tokens() {
            let mut inspection =
                get_trace("46909832db6ca33317c43436c76eef4b654d7f9cbc5e64cf47079aa7ea8be845");
            let uni = Uniswap::new();
            uni.inspect(&mut inspection);

            let actions = &inspection.actions;
            assert_eq!(
                ADDRESSBOOK.get(&to_transfer(&actions[0]).token).unwrap(),
                "ETH"
            );

            let is_deposit = is_weth(&actions[1], true);
            assert!(is_deposit);

            // Second is the trade
            let trade = to_trade(&actions[2]);
            assert_eq!(ADDRESSBOOK.get(&trade.t1.token).unwrap(), "WETH");
            assert_eq!(trade.t1.amount, 664510977762648404u64.into());
            assert_eq!(
                trade.t2.amount,
                U256::from_dec_str("499000000000000000000").unwrap()
            );

            // Third is the ETH refund
            assert_eq!(
                ADDRESSBOOK.get(&to_transfer(&actions[3]).token).unwrap(),
                "ETH"
            );

            assert_eq!(inspection.status, Status::Success);
            assert_eq!(inspection.known().len(), 4);
            assert_eq!(inspection.unknown().len(), 0);
        }

        #[test]
        // https://etherscan.io/tx/0xeef0edcc4ce9aa85db5bc6a788b5a770dcc0d13eb7df4e7c008c1ac6666cd989
        fn parse_exact_tokens_for_eth() {
            let mut inspection = read_trace("exact_tokens_for_eth.json");
            let uni = Uniswap::new();
            uni.inspect(&mut inspection);

            let actions = &inspection.actions;

            // The router makes the first action by transferFrom'ing the tokens we're
            // sending in
            let trade = to_trade(&actions[0]);
            assert_eq!(ADDRESSBOOK.get(&trade.t2.token).unwrap(), "WETH");

            assert_eq!(inspection.known().len(), 3);
            assert_eq!(inspection.unknown().len(), 0);

            assert!(is_weth(&actions[1], false));

            // send the eth to the buyer
            assert_eq!(
                ADDRESSBOOK.get(&to_transfer(&actions[2]).token).unwrap(),
                "ETH"
            );

            assert_eq!(inspection.status, Status::Success);
        }

        #[test]
        // https://etherscan.io/tx/0x622519e27d56ea892c6e5e479b68e1eb6278e222ed34d0dc4f8f0fd254723def/advanced#internal
        fn parse_exact_tokens_for_tokens() {
            let mut inspection =
                get_trace("622519e27d56ea892c6e5e479b68e1eb6278e222ed34d0dc4f8f0fd254723def");
            let uni = Uniswap::new();
            uni.inspect(&mut inspection);
            assert_eq!(inspection.status, Status::Success);

            let actions = &inspection.actions;
            let trade = to_trade(&actions[0]);
            assert_eq!(ADDRESSBOOK.get(&trade.t1.token).unwrap(), "YFI");
            assert_eq!(ADDRESSBOOK.get(&trade.t2.token).unwrap(), "WETH");

            assert_eq!(inspection.known().len(), 1);
            assert_eq!(inspection.unknown().len(), 0);
        }

        #[test]
        // https://etherscan.io/tx/0x72493a035de37b73d3fcda2aa20852f4196165f3ce593244e51fa8e7c80bc13f/advanced#internal
        fn parse_exact_eth_for_tokens() {
            let mut inspection =
                get_trace("72493a035de37b73d3fcda2aa20852f4196165f3ce593244e51fa8e7c80bc13f");
            let uni = Uniswap::new();
            uni.inspect(&mut inspection);
            assert_eq!(inspection.status, Status::Success);

            assert_eq!(inspection.known().len(), 3);
            assert_eq!(inspection.unknown().len(), 0);

            assert_eq!(
                ADDRESSBOOK
                    .get(&to_transfer(&inspection.actions[0]).token)
                    .unwrap(),
                "ETH"
            );

            assert!(is_weth(&inspection.actions[1], true));

            let trade = to_trade(&inspection.actions[2]);
            assert_eq!(ADDRESSBOOK.get(&trade.t1.token).unwrap(), "WETH");
            assert_eq!(
                trade.t1.amount,
                U256::from_dec_str("4500000000000000000").unwrap()
            );
            assert_eq!(
                trade.t2.amount,
                U256::from_dec_str("13044604442132612367").unwrap()
            );
        }
    }
}
