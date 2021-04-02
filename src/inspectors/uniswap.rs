use crate::{
    addresses::{AAVE_LENDING_POOL_CORE, PROTOCOLS},
    inspectors::find_matching,
    traits::Inspector,
    types::{
        actions::{AddLiquidity as AddLiquidityAct, Trade},
        Classification, Inspection, Protocol, Status,
    },
};

use ethers::{abi::Abi, contract::BaseContract};
use ethers::{
    contract::decode_function_data,
    types::{Address, Bytes, Call as TraceCall, CallType, U256},
};

// TODO convert these to EthAbiType

// Type aliases for Uniswap's `swap` return types
type SwapTokensFor = (U256, U256, Vec<Address>, Address, U256);
type SwapEthFor = (U256, Vec<Address>, Address, U256);
type PairSwap = (U256, U256, Address, Bytes);
/// (tokenA, tokenB, amountADesired, amountBDesired, amountAMin, amountBMin, to, deadline)
/// See https://uniswap.org/docs/v2/smart-contracts/router02/#addliquidity
type AddLiquidity = (Address, Address, U256, U256, U256, U256, Address, U256);
//
// /// (token, amountTokenDesired, amountTokenMin, amountETHMin, to, deadline)
// /// See https://uniswap.org/docs/v2/smart-contracts/router02/#addliquidityeth
// type AddLiquidityEth = (Address, U256, U256, U256, Address, U256);
//
// /// (tokenA, tokenB, liquidity, amountAMin, amountBMin, to, deadline)
// /// See https://uniswap.org/docs/v2/smart-contracts/router02/#removeliquidity
// type RemoveLiquidity = (Address, Address, U256, U256, U256, Address, U256);
//
// /// (tokenA, liquidity, amountTokenMin, amountETHMin, to, deadline)
// /// See https://uniswap.org/docs/v2/smart-contracts/router02/#removeliquidityeth
// type RemoveLiquidityEth = (Address, U256, U256, U256, Address, U256);

#[derive(Debug, Clone)]
/// An inspector for Uniswap
pub struct Uniswap {
    router: BaseContract,
    pair: BaseContract,
}

impl Inspector for Uniswap {
    fn inspect(&self, inspection: &mut Inspection) {
        let num_protocols = inspection.protocols.len();
        let actions = inspection.actions.to_vec();

        let mut prune: Vec<usize> = Vec::new();
        let mut has_trade = false;
        for i in 0..inspection.actions.len() {
            let action = &mut inspection.actions[i];

            if let Some(calltrace) = action.as_call() {
                let call = calltrace.as_ref();
                let preflight = self.is_preflight(call);

                // we classify AddLiquidity calls in order to find sandwich attacks
                // by removing/adding liquidity before/after a trade
                if let Ok((token0, token1, amount0, amount1, _, _, _, _)) = self
                    .router
                    .decode::<AddLiquidity, _>("addLiquidity", &call.input)
                {
                    let trace_address = calltrace.trace_address.clone();
                    *action = Classification::new(
                        AddLiquidityAct {
                            tokens: vec![token0, token1],
                            amounts: vec![amount0, amount1],
                        },
                        trace_address,
                    );
                } else if let Ok((_, _, _, bytes)) =
                    self.pair.decode::<PairSwap, _>("swap", &call.input)
                {
                    // add the protocol
                    let protocol = uniswappy(&call);
                    inspection.protocols.insert(protocol);

                    // skip flashswaps -- TODO: Get an example tx.
                    if !bytes.as_ref().is_empty() {
                        eprintln!("Flashswaps are not supported. {:?}", inspection.hash);
                        continue;
                    }

                    let res = find_matching(
                        // Iterate backwards
                        actions.iter().enumerate().rev().skip(actions.len() - i),
                        // Get a transfer
                        |t| t.transfer(),
                        // We just want the first transfer, no need to filter for anything
                        |_| true,
                        // `check_all=true` because there might be other known calls
                        // before that, due to the Uniswap V2 architecture.
                        true,
                    );

                    if let Some((idx_in, transfer_in)) = res {
                        let res = find_matching(
                            actions.iter().enumerate().skip(i + 1),
                            // Get a transfer
                            |t| t.transfer(),
                            // We just want the first transfer, no need to filter for anything
                            |_| true,
                            // `check_all = false` because the first known external call
                            // after the `swap` must be a transfer out
                            false,
                        );

                        if let Some((idx_out, transfer_out)) = res {
                            // change the action to a trade
                            *action = Classification::new(
                                Trade {
                                    t1: transfer_in.clone(),
                                    t2: transfer_out.clone(),
                                },
                                Vec::new(),
                            );
                            // if a trade has been made, then we will not try
                            // to flag this as "checked"
                            has_trade = true;
                            // prune the 2 trades
                            prune.push(idx_in);
                            prune.push(idx_out);
                        }
                    }
                } else if (call.call_type == CallType::StaticCall && preflight) || self.check(call)
                {
                    let protocol = uniswappy(&call);
                    inspection.protocols.insert(protocol);
                    *action = Classification::Prune;
                }
            }
        }

        prune
            .iter()
            .for_each(|p| inspection.actions[*p] = Classification::Prune);

        // If there are less than 2 classified actions (i.e. we didn't execute more
        // than 1 trade attempt, and if there were checked protocols
        // in this transaction, then that means there was an arb check which reverted early
        if inspection.protocols.len() > num_protocols
            && inspection
                .actions
                .iter()
                .filter_map(|x| x.as_action())
                .count()
                < 2
            && !has_trade
        {
            inspection.status = Status::Checked;
        }
    }
}

fn uniswappy(call: &TraceCall) -> Protocol {
    if let Some(protocol) = PROTOCOLS.get(&call.to) {
        *protocol
    } else if let Some(protocol) = PROTOCOLS.get(&call.from) {
        *protocol
    } else {
        Protocol::Uniswappy
    }
}

impl Uniswap {
    /// Constructor
    pub fn new() -> Self {
        Self {
            router: BaseContract::from({
                serde_json::from_str::<Abi>(include_str!("../../abi/unirouterv2.json"))
                    .expect("could not parse uniswap abi")
            }),
            pair: BaseContract::from({
                serde_json::from_str::<Abi>(include_str!("../../abi/unipair.json"))
                    .expect("could not parse uniswap abi")
            }),
        }
    }

    fn is_preflight(&self, call: &TraceCall) -> bool {
        // There's a function selector clash here with Aave's getReserves
        // function in the core, which we do not care about here
        // https://github.com/aave/aave-protocol/search?q=%22function+getReserves%28%29%22
        call.to != *AAVE_LENDING_POOL_CORE
            && call.from != *AAVE_LENDING_POOL_CORE
            && call
                .input
                .as_ref()
                .starts_with(&ethers::utils::id("getReserves()"))
    }

    // There MUST be 1 `swap` call in the traces either to the Pair directly
    // or to the router
    #[allow(clippy::collapsible_if)]
    fn check(&self, call: &TraceCall) -> bool {
        if self.is_preflight(call) {
            true
        } else {
            for function in self.router.as_ref().functions() {
                if function.name.starts_with("swapETH") || function.name.starts_with("swapExactETH")
                {
                    if decode_function_data::<SwapEthFor, _>(function, &call.input, true).is_ok() {
                        return true;
                    }
                } else if function.name.starts_with("swap") {
                    if decode_function_data::<SwapTokensFor, _>(function, &call.input, true).is_ok()
                    {
                        return true;
                    }
                }
            }

            false
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::test_helpers::*;
    use crate::{
        addresses::ADDRESSBOOK,
        reducers::{ArbitrageReducer, TradeReducer},
        types::{Protocol, Status},
        Reducer,
    };
    use crate::{inspectors::ERC20, types::Inspection, Inspector};
    use ethers::types::U256;

    // inspector that does all 3 transfer/trade/arb combos
    struct MyInspector {
        erc20: ERC20,
        uni: Uniswap,
        trade: TradeReducer,
        arb: ArbitrageReducer,
    }

    impl MyInspector {
        fn inspect(&self, inspection: &mut Inspection) {
            self.erc20.inspect(inspection);
            self.uni.inspect(inspection);

            self.trade.reduce(inspection);
            self.arb.reduce(inspection);

            inspection.prune();
        }

        fn new() -> Self {
            Self {
                erc20: ERC20::new(),
                uni: Uniswap::new(),
                trade: TradeReducer,
                arb: ArbitrageReducer::new(),
            }
        }
    }

    mod arbitrages {
        use super::*;

        #[test]
        // https://etherscan.io/tx/0xd9306dc8c1230cc0faef22a8442d0994b8fc9a8f4c9faeab94a9a7eac8e59710
        // This trace does not use the Routers, instead it goes directly to the YFI pair contracts
        fn parse_uni_sushi_arb() {
            let mut inspection =
                get_trace("0xd9306dc8c1230cc0faef22a8442d0994b8fc9a8f4c9faeab94a9a7eac8e59710");
            let uni = MyInspector::new();
            uni.inspect(&mut inspection);

            let known = inspection.known();
            assert_eq!(known.len(), 3);

            let arb = known[1].as_ref().arbitrage().unwrap();
            assert!(arb.profit == U256::from_dec_str("626678385524850545").unwrap());

            // the initial call and the delegate call
            assert_eq!(inspection.unknown().len(), 7);
            assert_eq!(
                inspection.protocols,
                crate::set![Protocol::Sushiswap, Protocol::Uniswap]
            );
        }

        // https://etherscan.io/tx/0xdfeae07360e2d7695a498e57e2054c658d1d78bbcd3c763fc8888b5433b6c6d5
        #[test]
        fn xsp_xfi_eth_arb() {
            let mut inspection =
                get_trace("0xdfeae07360e2d7695a498e57e2054c658d1d78bbcd3c763fc8888b5433b6c6d5");
            let uni = MyInspector::new();
            uni.inspect(&mut inspection);

            let known = inspection.known();

            assert!(known[0].as_ref().deposit().is_some());
            let arb = known[1].as_ref().arbitrage().unwrap();
            assert_eq!(arb.profit, U256::from_dec_str("23939671034095067").unwrap());
            assert!(known[2].as_ref().withdrawal().is_some());
            assert_eq!(inspection.unknown().len(), 10);
        }

        // https://etherscan.io/tx/0xddbf97f758bd0958487e18d9e307cd1256b1ad6763cd34090f4c9720ba1b4acc
        #[test]
        fn triangular_router_arb() {
            let mut inspection = read_trace("triangular_arb.json");
            let uni = MyInspector::new();

            uni.inspect(&mut inspection);

            let known = inspection.known();

            let arb = known[0].as_ref().arbitrage().unwrap();
            assert_eq!(arb.profit, U256::from_dec_str("9196963592118237").unwrap());

            assert_eq!(inspection.known().len(), 1);
            assert_eq!(inspection.unknown().len(), 2);
        }
    }

    #[test]
    // https://etherscan.io/tx/0x123d03cef9ccd4230d111d01cf1785aed4242eb2e1e542bd792d025eb7e3cc84/advanced#internal
    fn router_insufficient_amount() {
        let mut inspection =
            get_trace("123d03cef9ccd4230d111d01cf1785aed4242eb2e1e542bd792d025eb7e3cc84");
        let uni = MyInspector::new();
        uni.inspect(&mut inspection);
        assert_eq!(inspection.status, Status::Checked); // This is a check
        let known = inspection.known();
        let transfer = known[0].as_ref().transfer().unwrap();
        assert_eq!(ADDRESSBOOK.get(&transfer.token).unwrap(), "ETH");
    }

    #[test]
    // Traces which either reverted or returned early on purpose, after checking
    // for an arb opportunity and seeing that it won't work.
    fn checked() {
        let both = crate::set![Protocol::Uniswap, Protocol::Sushiswap];
        let uni = crate::set![Protocol::Uniswap];
        for (trace, protocols) in &[
            (
                "0x2f85ce5bb5f7833e052897fa4a070615a4e21a247e1ccc2347a3882f0e73943d",
                &both,
            ),
            (
                // sakeswap is uniswappy
                "0xd9df5ae2e9e18099913559f71473866758df3fd25919be605c71c300e64165fd",
                &crate::set![Protocol::Uniswappy, Protocol::Uniswap],
            ),
            (
                "0xfd24e512dc90bd1ca8a4f7987be6122c1fa3221b261e8728212f2f4d980ee4cd",
                &both,
            ),
            (
                "0xf5f0b7e1c1761eff33956965f90b6d291fa2ff3c9907b450d483a58932c54598",
                &both,
            ),
            (
                "0x4cf1a912197c2542208f7c1b5624fa5ea75508fa45f41c28f7e6aaa443d14db2",
                &both,
            ),
            (
                "0x9b08b7c8efe5cfd40c012b956a6031f60c076bc07d5946888a0d55e5ed78b38a",
                &uni,
            ),
            (
                "0xe43734199366c665e341675e0f6ea280745d7d801924815b2c642dc83c8756d6",
                &both,
            ),
            (
                "0x243b4b5bf96d345f690f6b17e75031dc634d0e97c47d73cbecf2327250077591",
                &both,
            ),
            (
                "0x52311e6ec870f530e84f79bbb08dce05c95d80af5a3cb29ab85d128a15dbea8d",
                &uni,
            ),
        ] {
            let mut inspection = get_trace(trace);
            let uni = MyInspector::new();
            uni.inspect(&mut inspection);
            assert_eq!(inspection.status, Status::Checked);
            assert_eq!(inspection.protocols, **protocols);
        }
    }

    #[test]
    // https://etherscan.io/tx/0xb9d415abb21007d6d947949113b91b2bf33c82d291d510e23a08e64ce80bf5bf
    fn bot_trade() {
        let mut inspection = read_trace("bot_trade.json");
        let uni = MyInspector::new();
        uni.inspect(&mut inspection);

        let known = inspection.known();

        assert_eq!(known.len(), 4);
        let t1 = known[0].as_ref().transfer().unwrap();
        assert_eq!(
            t1.amount,
            U256::from_dec_str("155025667786800022191").unwrap()
        );
        let trade = known[1].as_ref().trade().unwrap();
        assert_eq!(
            trade.t1.amount,
            U256::from_dec_str("28831175112148480867").unwrap()
        );
        let _t2 = known[2].as_ref().transfer().unwrap();
        let _t3 = known[3].as_ref().transfer().unwrap();
    }

    mod simple_transfers {
        use super::*;

        #[test]
        // https://etherscan.io/tx/0x46909832db6ca33317c43436c76eef4b654d7f9cbc5e64cf47079aa7ea8be845/advanced#internal
        fn parse_eth_for_exact_tokens() {
            let mut inspection =
                get_trace("46909832db6ca33317c43436c76eef4b654d7f9cbc5e64cf47079aa7ea8be845");
            let uni = MyInspector::new();
            uni.inspect(&mut inspection);

            let known = inspection.known();

            let transfer = known[0].as_ref().transfer().unwrap();
            assert_eq!(ADDRESSBOOK.get(&transfer.token).unwrap(), "ETH");

            let deposit = known[1].as_ref().deposit();
            assert!(deposit.is_some());

            // Second is the trade
            let trade = known[2].as_ref().trade().unwrap();
            assert_eq!(ADDRESSBOOK.get(&trade.t1.token).unwrap(), "WETH");
            assert_eq!(trade.t1.amount, 664510977762648404u64.into());
            assert_eq!(
                trade.t2.amount,
                U256::from_dec_str("499000000000000000000").unwrap()
            );

            // Third is the ETH refund
            let transfer = known[3].as_ref().transfer().unwrap();
            assert_eq!(ADDRESSBOOK.get(&transfer.token).unwrap(), "ETH");

            assert_eq!(inspection.status, Status::Success);
            assert_eq!(inspection.known().len(), 4);
            assert_eq!(inspection.unknown().len(), 2);
        }

        #[test]
        // https://etherscan.io/tx/0xeef0edcc4ce9aa85db5bc6a788b5a770dcc0d13eb7df4e7c008c1ac6666cd989
        fn parse_exact_tokens_for_eth() {
            let mut inspection = read_trace("exact_tokens_for_eth.json");
            let uni = MyInspector::new();
            uni.inspect(&mut inspection);

            let known = inspection.known();

            // The router makes the first action by transferFrom'ing the tokens we're
            // sending in
            let trade = known[0].as_ref().trade().unwrap();
            assert_eq!(ADDRESSBOOK.get(&trade.t2.token).unwrap(), "WETH");

            assert_eq!(inspection.known().len(), 3);
            assert_eq!(inspection.unknown().len(), 2);

            let withdrawal = known[1].as_ref().withdrawal();
            assert!(withdrawal.is_some());

            // send the eth to the buyer
            let transfer = known[2].as_ref().transfer().unwrap();
            assert_eq!(ADDRESSBOOK.get(&transfer.token).unwrap(), "ETH");

            assert_eq!(inspection.status, Status::Success);
        }

        #[test]
        // https://etherscan.io/tx/0x622519e27d56ea892c6e5e479b68e1eb6278e222ed34d0dc4f8f0fd254723def/advanced#internal
        fn parse_exact_tokens_for_tokens() {
            let mut inspection =
                get_trace("622519e27d56ea892c6e5e479b68e1eb6278e222ed34d0dc4f8f0fd254723def");
            let uni = MyInspector::new();
            uni.inspect(&mut inspection);
            assert_eq!(inspection.status, Status::Success);

            let known = inspection.known();
            let trade = known[0].as_ref().trade().unwrap();
            assert_eq!(ADDRESSBOOK.get(&trade.t1.token).unwrap(), "YFI");
            assert_eq!(ADDRESSBOOK.get(&trade.t2.token).unwrap(), "WETH");
            assert_eq!(inspection.known().len(), 1);
            assert_eq!(inspection.unknown().len(), 2);
        }

        #[test]
        // https://etherscan.io/tx/0x72493a035de37b73d3fcda2aa20852f4196165f3ce593244e51fa8e7c80bc13f/advanced#internal
        fn parse_exact_eth_for_tokens() {
            let mut inspection =
                get_trace("72493a035de37b73d3fcda2aa20852f4196165f3ce593244e51fa8e7c80bc13f");
            let uni = MyInspector::new();
            uni.inspect(&mut inspection);
            assert_eq!(inspection.status, Status::Success);

            let known = inspection.known();
            assert_eq!(known.len(), 3);
            assert_eq!(inspection.unknown().len(), 2);

            let transfer = known[0].as_ref().transfer().unwrap();
            assert_eq!(ADDRESSBOOK.get(&transfer.token).unwrap(), "ETH");

            let deposit = known[1].as_ref().deposit();
            assert!(deposit.is_some());

            let trade = known[2].as_ref().trade().unwrap();
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
