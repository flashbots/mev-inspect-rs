use crate::{
    inspectors::ERC20,
    traits::Inspector,
    types::{Inspection, Status},
};

use ethers::{abi::Abi, contract::BaseContract};

mod inspector;

#[derive(Debug, Clone)]
/// An inspector for Uniswap
pub struct Uniswap {
    erc20: ERC20,
    router: BaseContract,
    pair: BaseContract,
}

impl Uniswap {
    /// Constructor
    pub fn new() -> Self {
        Self {
            erc20: ERC20::new(),
            router: BaseContract::from({
                serde_json::from_str::<Abi>(include_str!("../../../abi/unirouterv2.json"))
                    .expect("could not parse uniswap abi")
            }),
            pair: BaseContract::from({
                serde_json::from_str::<Abi>(include_str!("../../../abi/unipair.json"))
                    .expect("could not parse uniswap abi")
            }),
        }
    }
}

impl Inspector for Uniswap {
    fn inspect(&self, inspection: &mut Inspection) {
        let actions = &mut inspection.actions;

        let protocols = actions
            .iter_mut()
            .filter_map(|action| self.inspect_one(action))
            .collect::<Vec<_>>();

        inspection.protocols.extend(&protocols[..]);
        inspection.protocols.sort_unstable();
        inspection.protocols.dedup();

        // If there are less than 2 classified actions (i.e. we didn't execute more
        // than 1 trade attempt, and if there were checked protocols
        // in this transaction, then that means there was an arb check which reverted early
        if !inspection.protocols.is_empty()
            && inspection
                .actions
                .iter()
                .filter_map(|x| x.to_action())
                .count()
                < 2
        {
            inspection.status = Status::Checked;
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
    use ethers::types::U256;

    // inspector that does all 3 transfer/trade/arb combos
    struct MyInspector {
        uni: Uniswap,
        trade: TradeReducer,
        arb: ArbitrageReducer,
    }

    impl MyInspector {
        fn inspect(&self, inspection: &mut Inspection) {
            self.uni.inspect(inspection);
            self.trade.reduce(inspection);
            self.arb.reduce(inspection);
            inspection.prune();
        }

        fn new() -> Self {
            Self {
                uni: Uniswap::new(),
                trade: TradeReducer::new(),
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
            assert_eq!(inspection.unknown().len(), 2);
            assert_eq!(
                inspection.protocols,
                vec![Protocol::Uniswap, Protocol::Sushiswap]
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

            let arb = to_arb(&inspection.actions[0]);
            assert_eq!(arb.profit, U256::from_dec_str("9196963592118237").unwrap());

            // 4 swaps loop and the withdrawal
            assert_eq!(inspection.known().len(), 1);
            assert_eq!(inspection.unknown().len(), 0);
        }
    }

    #[test]
    // https://etherscan.io/tx/0x123d03cef9ccd4230d111d01cf1785aed4242eb2e1e542bd792d025eb7e3cc84/advanced#internal
    fn router_insufficient_amount() {
        let mut inspection =
            get_trace("123d03cef9ccd4230d111d01cf1785aed4242eb2e1e542bd792d025eb7e3cc84");
        let uni = MyInspector::new();
        uni.inspect(&mut inspection);
        assert_eq!(inspection.status, Status::Reverted);
        assert_eq!(
            ADDRESSBOOK
                .get(&to_transfer(&inspection.actions[0]).token)
                .unwrap(),
            "ETH"
        );
    }

    #[test]
    // Traces which either reverted or returned early on purpose, after checking
    // for an arb opportunity and seeing that it won't work.
    fn checked() {
        let both = &[Protocol::Uniswap, Protocol::Sushiswap][..];
        let uni = &[Protocol::Uniswap][..];
        for (trace, protocols) in &[
            (
                "0x2f85ce5bb5f7833e052897fa4a070615a4e21a247e1ccc2347a3882f0e73943d",
                both,
            ),
            (
                "0xd9df5ae2e9e18099913559f71473866758df3fd25919be605c71c300e64165fd",
                uni,
            ),
            (
                "0xfd24e512dc90bd1ca8a4f7987be6122c1fa3221b261e8728212f2f4d980ee4cd",
                both,
            ),
            (
                "0xf5f0b7e1c1761eff33956965f90b6d291fa2ff3c9907b450d483a58932c54598",
                both,
            ),
            (
                "0x4cf1a912197c2542208f7c1b5624fa5ea75508fa45f41c28f7e6aaa443d14db2",
                both,
            ),
            (
                "0x9b08b7c8efe5cfd40c012b956a6031f60c076bc07d5946888a0d55e5ed78b38a",
                uni,
            ),
            (
                "0xe43734199366c665e341675e0f6ea280745d7d801924815b2c642dc83c8756d6",
                both,
            ),
            (
                "0x243b4b5bf96d345f690f6b17e75031dc634d0e97c47d73cbecf2327250077591",
                both,
            ),
            (
                "0x52311e6ec870f530e84f79bbb08dce05c95d80af5a3cb29ab85d128a15dbea8d",
                uni,
            ),
        ] {
            let mut inspection = get_trace(trace);
            let uni = MyInspector::new();
            uni.inspect(&mut inspection);
            assert_eq!(inspection.status, Status::Checked);
            assert_eq!(inspection.protocols, *protocols,);
        }
    }
}
