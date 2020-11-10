use crate::{
    inspectors::{ArbitrageReducer, ERC20},
    traits::{Inspector, Reducer},
    types::Inspection,
};

use ethers::{abi::Abi, contract::BaseContract};

mod inspector;
mod reducer;

#[derive(Debug, Clone)]
/// An inspector for Uniswap
pub struct Uniswap {
    erc20: ERC20,
    router: BaseContract,
    pair: BaseContract,
    arb: ArbitrageReducer,
}

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
        // Transfers to trades
        self.combine_transfers(&mut inspection.actions);
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
                serde_json::from_str::<Abi>(include_str!("../../../abi/unirouterv2.json"))
                    .expect("could not parse uniswap abi")
            }),
            pair: BaseContract::from({
                serde_json::from_str::<Abi>(include_str!("../../../abi/unipair.json"))
                    .expect("could not parse uniswap abi")
            }),
            arb: ArbitrageReducer,
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::test_helpers::*;
    use crate::{
        addresses::ADDRESSBOOK,
        types::{Protocol, Status},
    };
    use ethers::types::U256;

    mod arbitrages {
        use super::*;

        #[test]
        // https://etherscan.io/tx/0xd9306dc8c1230cc0faef22a8442d0994b8fc9a8f4c9faeab94a9a7eac8e59710
        // This trace does not use the Routers, instead it goes directly to the YFI pair contracts
        fn parse_uni_sushi_arb() {
            let mut inspection =
                get_trace("0xd9306dc8c1230cc0faef22a8442d0994b8fc9a8f4c9faeab94a9a7eac8e59710");
            let uni = Uniswap::new();
            uni.inspect(&mut inspection);

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

    #[test]
    // https://etherscan.io/tx/0x123d03cef9ccd4230d111d01cf1785aed4242eb2e1e542bd792d025eb7e3cc84/advanced#internal
    fn router_insufficient_amount() {
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
            let uni = Uniswap::new();
            uni.inspect(&mut inspection);
            assert_eq!(inspection.status, Status::Checked);
            assert_eq!(inspection.protocols, *protocols,);
        }
    }
}
