use crate::{
    addresses::PROTOCOLS,
    traits::Inspector,
    types::{actions::Transfer, Classification, Inspection, Protocol},
    DefiProtocol, ProtocolContracts,
};

use crate::model::{CallClassification, InternalCall};
use ethers::{
    contract::abigen,
    contract::BaseContract,
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
        Self { bridge, exchange }
    }
}

impl DefiProtocol for ZeroEx {
    fn base_contracts(&self) -> ProtocolContracts {
        ProtocolContracts::Dual(&self.exchange, &self.bridge)
    }

    fn protocol() -> Protocol {
        Protocol::ZeroEx
    }

    fn classify_call(&self, call: &InternalCall) -> Option<CallClassification> {
        todo!()
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
        Reducer,
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

        fn new() -> Self {
            Self {
                zeroex: ZeroEx::default(),
                erc20: ERC20::new(),
                trade: TradeReducer,
                arbitrage: ArbitrageReducer::new(),
            }
        }
    }

    #[test]
    // Split trade between balancer and uniswap via the 0x exchange proxy
    fn balancer_uni_zeroex() {
        let mut inspection = read_trace("exchange_proxy.json");
        let zeroex = MyInspector::new();
        zeroex.inspect(&mut inspection);
        assert_eq!(
            inspection.protocols,
            crate::set![Protocol::ZeroEx, Protocol::Balancer, Protocol::Uniswap]
        );
        assert_eq!(inspection.status, Status::Reverted);
        let known = inspection.known();

        assert_eq!(known.len(), 3);
        // transfer in
        let t1 = known[0].as_ref().transfer().unwrap();
        assert_eq!(
            t1.amount,
            U256::from_dec_str("1000000000000000000000").unwrap()
        );

        // balancer trade out
        let balancer = known[1].as_ref().trade().unwrap();
        assert_eq!(
            balancer.t1.amount,
            U256::from_dec_str("384007192433857968681").unwrap()
        );

        let uniswap = known[2].as_ref().trade().unwrap();
        assert_eq!(
            uniswap.t1.amount,
            U256::from_dec_str("622513125832506272941").unwrap()
        );

        // the trade required more than we put in (TODO: is this correct?)
        assert_ne!(t1.amount, balancer.t1.amount + uniswap.t1.amount);
    }
}
