use crate::{
    addresses::PROTOCOLS,
    traits::Inspector,
    types::{actions::Transfer, Classification, Inspection, Protocol},
    DefiProtocol, ProtocolContracts,
};

use crate::model::{CallClassification, EventLog, InternalCall};
use crate::types::actions::{SpecificAction, Trade};
use crate::types::{Action, TransactionData};
use ethers::{
    contract::{abigen, BaseContract, EthLogDecode},
    types::{Address, Bytes, U256},
};

abigen!(ZeroXUniswapBridge, "abi/0x-uniswap-bridge.json");
abigen!(ZeroXExchange, "abi/0x-exchange-v3.json");

#[derive(Debug, Clone)]
/// An inspector for ZeroEx Exchange Proxy transfers
pub struct ZeroEx {
    exchange: BaseContract,
    bridge: BaseContract,
}

type BridgeTransfer = (Address, Address, Address, U256, Bytes);

impl Default for ZeroEx {
    fn default() -> Self {
        let bridge = BaseContract::from(ZEROXUNISWAPBRIDGE_ABI.clone());
        let exchange = BaseContract::from(ZEROXEXCHANGE_ABI.clone());
        Self { exchange, bridge }
    }
}

impl DefiProtocol for ZeroEx {
    fn base_contracts(&self) -> ProtocolContracts {
        ProtocolContracts::Dual(&self.exchange, &self.bridge)
    }

    fn protocol(&self) -> Protocol {
        Protocol::ZeroEx
    }

    fn is_protocol_event(&self, log: &EventLog) -> bool {
        Erc20BridgeTransferFilter::decode_log(&log.raw_log).is_ok()
            || ZeroXExchangeEvents::decode_log(&log.raw_log).is_ok()
    }

    fn decode_call_action(&self, call: &InternalCall, tx: &TransactionData) -> Option<Action> {
        match call.classification {
            CallClassification::Transfer => {
                // https://github.com/0xProject/0x-monorepo/blob/development/contracts/asset-proxy/contracts/src/interfaces/IERC20Bridge.sol#L34
                println!("try bridge transfer decoding");
                if let Some((_, log, swap)) = tx
                    .call_logs_decoded::<Erc20BridgeTransferFilter>(&call.trace_address)
                    .next()
                {
                    println!("decoded zerox transfer");
                    let action = Trade {
                        t1: Transfer {
                            from: swap.from,
                            to: swap.to,
                            amount: swap.input_token_amount,
                            token: swap.input_token,
                        },
                        t2: Transfer {
                            from: swap.to,
                            to: swap.from,
                            amount: swap.output_token_amount,
                            token: swap.output_token,
                        },
                    };

                    // the bridge call will tell us which sub-protocol was used
                    let additional_protocols = PROTOCOLS
                        .get(&swap.from)
                        .cloned()
                        .map(|p| vec![p])
                        .unwrap_or_default();

                    return Some(Action::with_logs_and_protocols(
                        action.into(),
                        call.trace_address.clone(),
                        vec![log.log_index],
                        additional_protocols,
                    ));
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
        // https://github.com/0xProject/0x-monorepo/blob/development/contracts/asset-proxy/contracts/src/bridges/UniswapBridge.sol#L150
        self.bridge
            .decode::<BridgeTransfer, _>("bridgeTransferFrom", &call.input)
            .ok()
            .map(|_| (CallClassification::Transfer, None))
    }
}

impl Inspector for ZeroEx {
    fn inspect(&self, inspection: &mut Inspection) {
        let actions = inspection.actions.to_vec();
        let mut prune = Vec::new();
        for i in 0..inspection.actions.len() {
            let action = &mut inspection.actions[i];

            if let Some(calltrace) = action.as_call() {
                let call = calltrace.as_ref();

                if let Ok(transfer) = self
                    .bridge
                    .decode::<BridgeTransfer, _>("bridgeTransferFrom", &call.input)
                {
                    // we found a 0x transaction
                    inspection.protocols.insert(Protocol::ZeroEx);

                    // the bridge call will tell us which sub-protocol was used
                    if let Some(protocol) = PROTOCOLS.get(&transfer.1) {
                        inspection.protocols.insert(*protocol);
                    }

                    // change this to a transfer
                    *action = Classification::new(
                        Transfer {
                            token: transfer.0,
                            from: transfer.1,
                            to: transfer.2,
                            amount: transfer.3,
                        },
                        calltrace.trace_address.clone(),
                    );

                    // keep the index to prune all the subcalls
                    prune.push(i);
                }
            }
        }

        // remove the subcalls from any of the classified calls
        prune
            .into_iter()
            .for_each(|idx| actions[idx].prune_subcalls(&mut inspection.actions));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        inspectors::ERC20,
        reducers::{ArbitrageReducer, TradeReducer},
        test_helpers::*,
        types::Status,
        Reducer, TxReducer,
    };

    struct MyInspector {
        zeroex: ZeroEx,
        erc20: ERC20,
        trade: TradeReducer,
        arbitrage: ArbitrageReducer,
    }

    impl MyInspector {
        fn inspect(&self, inspection: &mut Inspection) {
            self.zeroex.inspect(inspection);
            self.erc20.inspect(inspection);
            self.trade.reduce(inspection);
            self.arbitrage.reduce(inspection);
            inspection.prune();
        }

        fn inspect_tx(&self, tx: &mut TransactionData) {
            self.zeroex.inspect_tx(tx);
            self.erc20.inspect_tx(tx);
            self.trade.reduce_tx(tx);
            self.arbitrage.reduce_tx(tx);
        }

        fn new() -> Self {
            Self {
                zeroex: ZeroEx::default(),
                erc20: ERC20::new(),
                trade: TradeReducer,
                arbitrage: ArbitrageReducer,
            }
        }
    }

    #[test]
    // Split trade between balancer and uniswap via the 0x exchange proxy
    fn balancer_uni_zeroex2() {
        let mut tx = read_tx("exchange_proxy.data.json");
        let zeroex = MyInspector::new();
        zeroex.inspect_tx(&mut tx);
        assert_eq!(tx.status, Status::Reverted);
    }

    #[test]
    // Split trade between balancer and uniswap via the 0x exchange proxy
    fn balancer_uni_zeroex() {
        let mut inspection = read_trace("exchange_proxy.json");
        let zeroex = MyInspector::new();
        zeroex.inspect(&mut inspection);
        assert_eq!(
            inspection.protocols,
            crate::set![Protocol::ZeroEx, Protocol::Balancer, Protocol::UniswapV2]
        );
        assert_eq!(inspection.status, Status::Reverted);
        let known = inspection.known();

        assert_eq!(known.len(), 3);
        // transfer in
        let t1 = known[0].as_ref().as_transfer().unwrap();
        assert_eq!(
            t1.amount,
            U256::from_dec_str("1000000000000000000000").unwrap()
        );

        // balancer trade out
        let balancer = known[1].as_ref().as_trade().unwrap();
        assert_eq!(
            balancer.t1.amount,
            U256::from_dec_str("384007192433857968681").unwrap()
        );

        let uniswap = known[2].as_ref().as_trade().unwrap();
        assert_eq!(
            uniswap.t1.amount,
            U256::from_dec_str("622513125832506272941").unwrap()
        );

        // the trade required more than we put in (TODO: is this correct?)
        assert_ne!(t1.amount, balancer.t1.amount + uniswap.t1.amount);
    }
}
