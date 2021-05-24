use ethers::{
    contract::{abigen, decode_function_data, BaseContract, EthLogDecode},
    types::{Address, Bytes, Call as TraceCall, CallType, U256},
};

use crate::inspectors::erc20::{self, ERC20};
use crate::model::{CallClassification, EventLog, InternalCall};
use crate::types::actions::{SpecificAction, Transfer};
use crate::types::{Action, TransactionData};
use crate::{
    addresses::{AAVE_LENDING_POOL_CORE, PROTOCOLS},
    inspectors::find_matching,
    traits::Inspector,
    types::{
        actions::{AddLiquidity as AddLiquidityAct, Trade},
        Classification, Inspection, Protocol, Status,
    },
    DefiProtocol, ProtocolContracts,
};
use std::collections::HashSet;

// Type aliases for Uniswap's `swap` return types
type SwapTokensFor = (U256, U256, Vec<Address>, Address, U256);
type SwapEthFor = (U256, Vec<Address>, Address, U256);
type PairSwap = (U256, U256, Address, Bytes);
/// (tokenA, tokenB, amountADesired, amountBDesired, amountAMin, amountBMin, to, deadline)
/// See https://uniswap.org/docs/v2/smart-contracts/router02/#addliquidity
type AddLiquidity = (Address, Address, U256, U256, U256, U256, Address, U256);

/// (token, amountTokenDesired, amountTokenMin, amountETHMin, to, deadline)
/// See https://uniswap.org/docs/v2/smart-contracts/router02/#addliquidityeth
type AddLiquidityEth = (Address, U256, U256, U256, Address, U256);

/// (tokenA, tokenB, liquidity, amountAMin, amountBMin, to, deadline)
/// See https://uniswap.org/docs/v2/smart-contracts/router02/#removeliquidity
type RemoveLiquidity = (Address, Address, U256, U256, U256, Address, U256);

/// (tokenA, liquidity, amountTokenMin, amountETHMin, to, deadline)
/// See https://uniswap.org/docs/v2/smart-contracts/router02/#removeliquidityeth
type RemoveLiquidityEth = (Address, U256, U256, U256, Address, U256);

abigen!(UniRouterV2, "abi/unirouterv2.json");
abigen!(UniPair, "abi/unipair.json");
abigen!(UniRouterV3, "abi/unirouterv3.json");
abigen!(UniPoolV3, "abi/unipoolv3.json");

#[derive(Debug, Clone)]
/// An inspector for Uniswap
pub struct Uniswap {
    router: BaseContract,
    pair: BaseContract,
    erc20: ERC20,
}

impl Default for Uniswap {
    fn default() -> Self {
        Self {
            router: BaseContract::from(UNIROUTERV2_ABI.clone()),
            pair: BaseContract::from(UNIPAIR_ABI.clone()),
            erc20: ERC20::new(),
        }
    }
}

impl DefiProtocol for Uniswap {
    fn base_contracts(&self) -> ProtocolContracts {
        ProtocolContracts::Dual(&self.pair, &self.router)
    }

    fn protocol(&self) -> Protocol {
        Protocol::Uniswappy
    }

    fn is_protocol_event(&self, log: &EventLog) -> bool {
        UniPairEvents::decode_log(&log.raw_log).is_ok()
    }

    fn is_protocol(&self, call: &InternalCall) -> Option<Option<Protocol>> {
        if let Some(protocol) = PROTOCOLS.get(&call.to) {
            Some(Some(*protocol))
        } else if let Some(protocol) = PROTOCOLS.get(&call.from) {
            Some(Some(*protocol))
        } else {
            Some(None)
        }
    }

    fn decode_call_action(&self, call: &InternalCall, tx: &TransactionData) -> Option<Action> {
        match call.classification {
            CallClassification::AddLiquidity => {
                // `addLiquidity` calls `transferFrom` twice resulting in two `Transfer` events (tokenA, tokenB)
                // https://github.com/Uniswap/uniswap-v2-periphery/blob/master/contracts/UniswapV2Router02.sol#L73-L74
                // for addLiquidityEth its (token, WETH)
                if let Some((_, mint_log, _)) = tx
                    .call_logs_decoded::<unipair_mod::MintFilter>(&call.trace_address)
                    .next()
                {
                    if let Some((transfer_0, transfer_1)) =
                        self.decode_token_transfers_prior(call, tx, mint_log.log_index)
                    {
                        let action = AddLiquidityAct {
                            tokens: vec![transfer_0.token, transfer_1.token],
                            amounts: vec![transfer_0.value, transfer_1.value],
                        };
                        return Some(Action::with_logs(
                            action.into(),
                            call.trace_address.clone(),
                            vec![
                                transfer_0.log_index,
                                transfer_1.log_index,
                                mint_log.log_index,
                            ],
                        ));
                    }
                }
            }
            CallClassification::Liquidation => {
                // get the burn event from the pair
                // burn https://github.com/Uniswap/uniswap-v2-core/blob/master/contracts/UniswapV2Pair.sol#L148-L149
                if let Some((_, burn_log, _)) = tx
                    .call_logs_decoded::<unipair_mod::BurnFilter>(&call.trace_address)
                    .next()
                {
                    if let Some((transfer_0, transfer_1)) =
                        self.decode_token_transfers_prior(call, tx, burn_log.log_index)
                    {
                        let action = AddLiquidityAct {
                            tokens: vec![transfer_0.token, transfer_1.token],
                            amounts: vec![transfer_0.value, transfer_1.value],
                        };
                        return Some(Action::with_logs(
                            action.into(),
                            call.trace_address.clone(),
                            vec![
                                transfer_0.log_index,
                                transfer_1.log_index,
                                burn_log.log_index,
                            ],
                        ));
                    }
                }
            }
            CallClassification::Swap => {
                let protocol = uniswappy(&call.to, &call.from);
                let protos = if protocol != self.protocol() {
                    vec![protocol]
                } else {
                    Vec::new()
                };

                // find the swap log
                if let Some((swap_call, swap_log, swap)) = tx
                    .call_logs_decoded::<unipair_mod::SwapFilter>(&call.trace_address)
                    .next()
                {
                    // swap emits at least 1 `Transfer` event before the `Swap` event
                    // https://github.com/Uniswap/uniswap-v2-core/blob/master/contracts/UniswapV2Pair.sol#L170-L171
                    if swap.amount_0_out.is_zero() || swap.amount_1_out.is_zero() {
                        // this is essentially a transfer of the token that's not `0`
                        if let Some((transfer_log, transfer)) = tx
                            .logs_prior_decoded::<erc20::TransferFilter>(swap_log.log_index)
                            .next()
                        {
                            let transfer = Transfer {
                                from: swap_call.to,
                                to: transfer.to,
                                amount: transfer.value,
                                token: transfer_log.address,
                            };
                            return Some(Action::with_logs_and_protocols(
                                transfer.into(),
                                call.trace_address.clone(),
                                vec![transfer_log.log_index, swap_log.log_index],
                                protos,
                            ));
                        }
                    } else {
                        // this is a trade and there should be two 2 transfer events before the `swap`
                        if let Some((transfer_0, transfer_1)) =
                            self.decode_token_transfers_prior(call, tx, swap_log.log_index)
                        {
                            let action = Trade {
                                t1: Transfer {
                                    from: transfer_0.from,
                                    to: transfer_0.to,
                                    amount: transfer_0.value,
                                    token: transfer_0.token,
                                },
                                t2: Transfer {
                                    from: swap_log.address,
                                    to: transfer_1.to,
                                    amount: transfer_1.value,
                                    token: transfer_1.token,
                                },
                            };

                            return Some(Action::with_logs_and_protocols(
                                action.into(),
                                call.trace_address.clone(),
                                vec![
                                    transfer_0.log_index,
                                    transfer_1.log_index,
                                    swap_log.log_index,
                                ],
                                protos,
                            ));
                        }
                    }
                }
            }
            _ => {}
        }
        None
    }

    fn classify(
        &self,
        call: &InternalCall,
    ) -> Option<(CallClassification, Option<SpecificAction>)> {
        if self
            .router
            .decode::<AddLiquidity, _>("addLiquidity", &call.input)
            .is_ok()
            || self
                .router
                .decode::<AddLiquidityEth, _>("addLiquidityETH", &call.input)
                .is_ok()
        {
            Some((CallClassification::AddLiquidity, None))
        } else if let Ok((_, _, _, bytes)) = self.pair.decode::<PairSwap, _>("swap", &call.input) {
            // we're only interested in the pair's `swap` function
            if bytes.as_ref().is_empty() {
                Some((CallClassification::Swap, None))
            } else {
                // TODO: Get an example tx.
                Some((CallClassification::FlashSwap, None))
            }
        } else if self
            .router
            .decode::<RemoveLiquidity, _>("removeLiquidity", &call.input)
            .is_ok()
        {
            Some((CallClassification::RemoveLiquidity, None))
        } else if let Ok((_token_a, _liquidity, _amount_token_min, _amount_ethmin, _to, _)) = self
            .router
            .decode::<RemoveLiquidityEth, _>("removeLiquidityETH", &call.input)
        {
            Some((CallClassification::RemoveLiquidity, None))
        } else {
            None
        }
    }
}

struct TokenTransfer {
    from: Address,
    token: Address,
    to: Address,
    value: U256,
    log_index: U256,
}

impl Uniswap {
    /// Decodes two `Transfer` events that happen right before the event with the given `log_index`
    fn decode_token_transfers_prior(
        &self,
        call: &InternalCall,
        tx: &TransactionData,
        log_index: U256,
    ) -> Option<(TokenTransfer, TokenTransfer)> {
        let logs_by = tx
            .call_logs(&call.trace_address)
            .map(|(_, l)| l.log_index)
            .collect::<HashSet<_>>();
        let mut transfers = tx
            .logs_prior_decoded::<erc20::TransferFilter>(log_index)
            .filter(|(l, _)| logs_by.contains(&l.log_index));

        if let (Some((log_1, transfer_1)), Some((log_0, transfer_0))) =
            (transfers.next(), transfers.next())
        {
            let transfer_1 = TokenTransfer {
                from: call.to,
                to: transfer_1.to,
                token: log_1.address,
                value: transfer_1.value,
                log_index: log_1.log_index,
            };

            let transfer_0 = TokenTransfer {
                from: call.to,
                to: transfer_0.to,
                token: log_0.address,
                value: transfer_0.value,
                log_index: log_0.log_index,
            };
            Some((transfer_0, transfer_1))
        } else {
            None
        }
    }
}

impl Uniswap {
    pub fn is_swap_call(&self, call: &InternalCall) -> bool {
        if self.pair.decode::<PairSwap, _>("swap", &call.input).is_ok() {
            return true;
        }
        for function in self.router.as_ref().functions() {
            if function.name.starts_with("swapETH") || function.name.starts_with("swapExactETH") {
                if decode_function_data::<SwapEthFor, _>(function, &call.input, true).is_ok() {
                    return true;
                }
            } else if function.name.starts_with("swap")
                && decode_function_data::<SwapTokensFor, _>(function, &call.input, true).is_ok()
            {
                return true;
            }
        }
        false
    }
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
                    let protocol = uniswappy(&call.to, &call.from);
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
                        |t| t.as_transfer(),
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
                            |t| t.as_transfer(),
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
                    let protocol = uniswappy(&call.to, &call.from);
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

fn uniswappy(to: &Address, from: &Address) -> Protocol {
    if let Some(protocol) = PROTOCOLS.get(to) {
        *protocol
    } else if let Some(protocol) = PROTOCOLS.get(from) {
        *protocol
    } else {
        Protocol::Uniswappy
    }
}

impl Uniswap {
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
    use ethers::types::U256;

    use crate::test_helpers::*;
    use crate::{
        addresses::ADDRESSBOOK,
        reducers::{ArbitrageReducer, TradeReducer},
        types::{Protocol, Status},
        Reducer, TxReducer,
    };
    use crate::{inspectors::ERC20, types::Inspection, Inspector};

    use super::*;

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

        fn inspect_tx(&self, tx: &mut TransactionData) {
            self.uni.inspect_tx(tx);
            self.erc20.inspect_tx(tx);

            self.trade.reduce_tx(tx);
            self.arb.reduce_tx(tx);
        }

        fn new() -> Self {
            Self {
                erc20: ERC20::new(),
                uni: Uniswap::default(),
                trade: TradeReducer,
                arb: ArbitrageReducer,
            }
        }
    }

    mod arbitrages {
        use super::*;

        #[test]
        // https://etherscan.io/tx/0xd9306dc8c1230cc0faef22a8442d0994b8fc9a8f4c9faeab94a9a7eac8e59710
        // This trace does not use the Routers, instead it goes directly to the YFI pair contracts
        fn parse_uni_sushi_arb2() {
            let mut tx =
                get_tx("0xd9306dc8c1230cc0faef22a8442d0994b8fc9a8f4c9faeab94a9a7eac8e59710");
            let uni = MyInspector::new();
            uni.inspect_tx(&mut tx);

            let actions = tx.actions().collect::<Vec<_>>();
            assert_eq!(actions.len(), 3);

            let arb = actions[1].as_arbitrage().unwrap();
            assert!(arb.profit == U256::from_dec_str("626678385524850545").unwrap());

            assert_eq!(
                tx.protocols(),
                crate::set![Protocol::Sushiswap, Protocol::UniswapV2]
            );
        }

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

            let arb = known[1].as_ref().as_arbitrage().unwrap();
            assert!(arb.profit == U256::from_dec_str("626678385524850545").unwrap());

            // the initial call and the delegate call
            assert_eq!(inspection.unknown().len(), 7);
            assert_eq!(
                inspection.protocols,
                crate::set![Protocol::Sushiswap, Protocol::UniswapV2]
            );
        }

        // https://etherscan.io/tx/0xdfeae07360e2d7695a498e57e2054c658d1d78bbcd3c763fc8888b5433b6c6d5
        #[test]
        fn xsp_xfi_eth_arb2() {
            let mut tx =
                get_tx("0xdfeae07360e2d7695a498e57e2054c658d1d78bbcd3c763fc8888b5433b6c6d5");
            let uni = MyInspector::new();
            uni.inspect_tx(&mut tx);

            let actions = tx.actions().collect::<Vec<_>>();
            assert!(actions[0].as_deposit().is_some());
            let arb = actions[1].as_arbitrage().unwrap();
            assert_eq!(arb.profit, U256::from_dec_str("23939671034095067").unwrap());
            assert!(actions[2].as_withdrawal().is_some());
        }

        // https://etherscan.io/tx/0xdfeae07360e2d7695a498e57e2054c658d1d78bbcd3c763fc8888b5433b6c6d5
        #[test]
        fn xsp_xfi_eth_arb() {
            let mut inspection =
                get_trace("0xdfeae07360e2d7695a498e57e2054c658d1d78bbcd3c763fc8888b5433b6c6d5");
            let uni = MyInspector::new();
            uni.inspect(&mut inspection);

            let known = inspection.known();

            assert!(known[0].as_ref().as_deposit().is_some());
            let arb = known[1].as_ref().as_arbitrage().unwrap();
            assert_eq!(arb.profit, U256::from_dec_str("23939671034095067").unwrap());
            assert!(known[2].as_ref().as_withdrawal().is_some());
            assert_eq!(inspection.unknown().len(), 10);
        }

        // https://etherscan.io/tx/0xddbf97f758bd0958487e18d9e307cd1256b1ad6763cd34090f4c9720ba1b4acc
        #[test]
        fn triangular_router_arb2() {
            let mut tx = read_tx("triangular_arb.data.json");
            let uni = MyInspector::new();

            uni.inspect_tx(&mut tx);

            let actions = tx.actions().collect::<Vec<_>>();

            let arb = actions[0].as_arbitrage().unwrap();
            assert_eq!(arb.profit, U256::from_dec_str("9196963592118237").unwrap());
        }

        // https://etherscan.io/tx/0xddbf97f758bd0958487e18d9e307cd1256b1ad6763cd34090f4c9720ba1b4acc
        #[test]
        fn triangular_router_arb() {
            let mut inspection = read_trace("triangular_arb.json");
            let uni = MyInspector::new();

            uni.inspect(&mut inspection);

            let known = inspection.known();

            let arb = known[0].as_ref().as_arbitrage().unwrap();
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
        let transfer = known[0].as_ref().as_transfer().unwrap();
        assert_eq!(ADDRESSBOOK.get(&transfer.token).unwrap(), "ETH");
    }

    #[test]
    // Traces which either reverted or returned early on purpose, after checking
    // for an arb opportunity and seeing that it won't work.
    fn checked() {
        let both = crate::set![Protocol::UniswapV2, Protocol::Sushiswap];
        let uni = crate::set![Protocol::UniswapV2];
        for (trace, protocols) in &[
            (
                "0x2f85ce5bb5f7833e052897fa4a070615a4e21a247e1ccc2347a3882f0e73943d",
                &both,
            ),
            (
                // sakeswap is uniswappy
                "0xd9df5ae2e9e18099913559f71473866758df3fd25919be605c71c300e64165fd",
                &crate::set![Protocol::Uniswappy, Protocol::UniswapV2],
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
    fn bot_trade2() {
        let mut tx = read_tx("bot_trade.data.json");
        let uni = MyInspector::new();
        uni.inspect_tx(&mut tx);

        let actions = tx.actions().collect::<Vec<_>>();

        let t1 = actions[0].as_transfer().unwrap();
        assert_eq!(
            t1.amount,
            U256::from_dec_str("155025667786800022191").unwrap()
        );
        let trade = actions[1].as_trade().unwrap();
        assert_eq!(
            trade.t1.amount,
            U256::from_dec_str("28831175112148480867").unwrap()
        );
        let _ = actions[2].as_transfer().unwrap();
        let _ = actions[3].as_transfer().unwrap();
    }

    #[test]
    // https://etherscan.io/tx/0xb9d415abb21007d6d947949113b91b2bf33c82d291d510e23a08e64ce80bf5bf
    fn bot_trade() {
        let mut inspection = read_trace("bot_trade.json");
        let uni = MyInspector::new();
        uni.inspect(&mut inspection);

        let known = inspection.known();

        assert_eq!(known.len(), 4);
        let t1 = known[0].as_ref().as_transfer().unwrap();
        assert_eq!(
            t1.amount,
            U256::from_dec_str("155025667786800022191").unwrap()
        );
        let trade = known[1].as_ref().as_trade().unwrap();
        assert_eq!(
            trade.t1.amount,
            U256::from_dec_str("28831175112148480867").unwrap()
        );
        let _t2 = known[2].as_ref().as_transfer().unwrap();
        let _t3 = known[3].as_ref().as_transfer().unwrap();
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

            let transfer = known[0].as_ref().as_transfer().unwrap();
            assert_eq!(ADDRESSBOOK.get(&transfer.token).unwrap(), "ETH");

            let deposit = known[1].as_ref().as_deposit();
            assert!(deposit.is_some());

            // Second is the trade
            let trade = known[2].as_ref().as_trade().unwrap();
            assert_eq!(ADDRESSBOOK.get(&trade.t1.token).unwrap(), "WETH");
            assert_eq!(trade.t1.amount, 664510977762648404u64.into());
            assert_eq!(
                trade.t2.amount,
                U256::from_dec_str("499000000000000000000").unwrap()
            );

            // Third is the ETH refund
            let transfer = known[3].as_ref().as_transfer().unwrap();
            assert_eq!(ADDRESSBOOK.get(&transfer.token).unwrap(), "ETH");

            assert_eq!(inspection.status, Status::Success);
            assert_eq!(inspection.known().len(), 4);
            assert_eq!(inspection.unknown().len(), 2);
        }

        #[test]
        // https://etherscan.io/tx/0xeef0edcc4ce9aa85db5bc6a788b5a770dcc0d13eb7df4e7c008c1ac6666cd989
        fn parse_exact_tokens_for_eth2() {
            let mut tx = read_tx("exact_tokens_for_eth.data.json");
            let uni = MyInspector::new();
            uni.inspect_tx(&mut tx);

            let actions = tx.actions().collect::<Vec<_>>();

            // The router makes the first action by transferFrom'ing the tokens we're
            // sending in
            let trade = actions[0].as_trade().unwrap();
            assert_eq!(ADDRESSBOOK.get(&trade.t2.token).unwrap(), "WETH");

            let withdrawal = actions[1].as_withdrawal();
            assert!(withdrawal.is_some());

            // send the eth to the buyer
            let transfer = actions[2].as_transfer().unwrap();
            assert_eq!(ADDRESSBOOK.get(&transfer.token).unwrap(), "ETH");
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
            let trade = known[0].as_ref().as_trade().unwrap();
            assert_eq!(ADDRESSBOOK.get(&trade.t2.token).unwrap(), "WETH");

            assert_eq!(inspection.known().len(), 3);
            assert_eq!(inspection.unknown().len(), 2);

            let withdrawal = known[1].as_ref().as_withdrawal();
            assert!(withdrawal.is_some());

            // send the eth to the buyer
            let transfer = known[2].as_ref().as_transfer().unwrap();
            assert_eq!(ADDRESSBOOK.get(&transfer.token).unwrap(), "ETH");

            assert_eq!(inspection.status, Status::Success);
        }

        #[test]
        // https://etherscan.io/tx/0x622519e27d56ea892c6e5e479b68e1eb6278e222ed34d0dc4f8f0fd254723def/advanced#internal
        fn parse_exact_tokens_for_tokens2() {
            let mut tx = get_tx("622519e27d56ea892c6e5e479b68e1eb6278e222ed34d0dc4f8f0fd254723def");
            let uni = MyInspector::new();
            uni.inspect_tx(&mut tx);

            let actions = tx.actions().collect::<Vec<_>>();
            let trade = actions[0].as_trade().unwrap();
            assert_eq!(ADDRESSBOOK.get(&trade.t1.token).unwrap(), "YFI");
            assert_eq!(ADDRESSBOOK.get(&trade.t2.token).unwrap(), "WETH");
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
            let trade = known[0].as_ref().as_trade().unwrap();
            assert_eq!(ADDRESSBOOK.get(&trade.t1.token).unwrap(), "YFI");
            assert_eq!(ADDRESSBOOK.get(&trade.t2.token).unwrap(), "WETH");
            assert_eq!(inspection.known().len(), 1);
            assert_eq!(inspection.unknown().len(), 2);
        }

        #[test]
        // https://etherscan.io/tx/0x72493a035de37b73d3fcda2aa20852f4196165f3ce593244e51fa8e7c80bc13f/advanced#internal
        fn parse_exact_eth_for_tokens2() {
            let mut tx = get_tx("72493a035de37b73d3fcda2aa20852f4196165f3ce593244e51fa8e7c80bc13f");
            let uni = MyInspector::new();
            uni.inspect_tx(&mut tx);

            let actions = tx.actions().collect::<Vec<_>>();

            let transfer = actions[0].as_transfer().unwrap();
            assert_eq!(ADDRESSBOOK.get(&transfer.token).unwrap(), "ETH");

            let deposit = actions[1].as_deposit();
            assert!(deposit.is_some());

            let trade = actions[2].as_trade().unwrap();
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

            let transfer = known[0].as_ref().as_transfer().unwrap();
            assert_eq!(ADDRESSBOOK.get(&transfer.token).unwrap(), "ETH");

            let deposit = known[1].as_ref().as_deposit();
            assert!(deposit.is_some());

            let trade = known[2].as_ref().as_trade().unwrap();
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
