use crate::{
    addresses::UNISWAP,
    inspectors::Uniswap,
    types::{Classification, Protocol},
};

use ethers::{
    contract::decode_fn as abi_decode,
    types::{Address, Bytes, Call as TraceCall, U256},
};

// Type aliases for Uniswap's `swap` return types
type SwapTokensFor = (U256, U256, Vec<Address>, Address, U256);
type SwapEthFor = (U256, Vec<Address>, Address, U256);
type PairSwap = (U256, U256, Address, Bytes);

impl Uniswap {
    /// Inspects the call, tries to classify it, and maybe returns the protocol type
    /// that the tx was a part of
    pub(crate) fn inspect_one(&self, action: &mut Classification) -> Option<Protocol> {
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
mod simple_trades {
    use super::*;
    use crate::{addresses::ADDRESSBOOK, test_helpers::*, types::Status, Inspector};

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
