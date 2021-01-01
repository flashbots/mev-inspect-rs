use crate::{
    types::inspection::{Inspection, TraceWrapper},
    Inspector, Reducer,
};
use ethers::types::Trace;
use itertools::Itertools;

/// Classifies traces according to the provided inspectors
pub struct BatchInspector {
    inspectors: Vec<Box<dyn Inspector>>,
    reducers: Vec<Box<dyn Reducer>>,
}

impl BatchInspector {
    /// Constructor
    pub fn new(inspectors: Vec<Box<dyn Inspector>>, reducers: Vec<Box<dyn Reducer>>) -> Self {
        Self {
            inspectors,
            reducers,
        }
    }

    /// Given a trace iterator, it groups all traces for the same tx hash
    /// and then inspects them and all of their subtraces
    pub fn inspect_many<'a>(&'a self, traces: impl IntoIterator<Item = Trace>) -> Vec<Inspection> {
        // group traces in a block by tx hash
        let traces = traces.into_iter().group_by(|t| t.transaction_hash);

        // inspects everything
        let inspections = traces
            .into_iter()
            // Convert the traces to inspections
            .filter_map(|(_, traces)| self.inspect_one(traces))
            .collect::<Vec<_>>();

        inspections
    }

    pub fn inspect_one<'a, T>(&'a self, traces: T) -> Option<Inspection>
    where
        T: IntoIterator<Item = Trace>,
    {
        use std::convert::TryFrom;
        let mut res = None;
        if let Some(mut i) = Inspection::try_from(TraceWrapper(traces)).ok() {
            if !i.actions.is_empty() {
                self.inspect(&mut i);
                self.reduce(&mut i);
                i.prune();
                res = Some(i);
            }
        }
        res
    }

    /// Decodes the inspection's actions
    pub fn inspect(&self, inspection: &mut Inspection) {
        for inspector in self.inspectors.iter() {
            inspector.inspect(inspection);
        }
    }

    pub fn reduce(&self, inspection: &mut Inspection) {
        for reducer in self.reducers.iter() {
            reducer.reduce(inspection);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        addresses::{ADDRESSBOOK, WETH},
        inspectors::*,
        reducers::*,
        test_helpers::*,
        types::{Protocol, Status},
    };
    use ethers::types::U256;

    #[test]
    // call that starts from a bot but has a uniswap sub-trace
    // https://etherscan.io/tx/0x93690c02fc4d58734225d898ea4091df104040450c0f204b6bf6f6850ac4602f
    // 99k USDC -> 281 ETH -> 5.7 YFI trade
    // Liquidator Repay -> 5.7 YFI
    // Liquidation -> 292 ETH
    // Profit: 11 ETH
    fn aave_uni_liquidation() {
        let mut inspection =
            get_trace("0x93690c02fc4d58734225d898ea4091df104040450c0f204b6bf6f6850ac4602f");

        let inspector = BatchInspector::new(
            vec![
                Box::new(ERC20::new()),
                Box::new(Uniswap::new()),
                Box::new(Aave::new()),
                Box::new(Curve::new()), // even though the Curve inspector is on, there's no Curve in the found protocols
            ],
            vec![
                // Classify liquidations first
                Box::new(LiquidationReducer::new()),
                Box::new(TradeReducer::new()),
                Box::new(ArbitrageReducer::new()),
            ],
        );
        inspector.inspect(&mut inspection);
        inspector.reduce(&mut inspection);
        inspection.prune();

        let known = inspection.known();

        dbg!(&known);

        let liquidation = known
            .iter()
            .find_map(|action| action.as_ref().profitable_liquidation())
            .unwrap();
        assert_eq!(
            liquidation.profit,
            U256::from_dec_str("11050220339336811520").unwrap()
        );

        assert_eq!(
            inspection.protocols,
            // SushiSwap is touched in a static call. The bot probably
            // checked whether it was more profitable to trade the
            // ETH for YFI on Sushi or Uni
            vec![Protocol::Uniswap, Protocol::Sushiswap, Protocol::Aave]
        );

        assert_eq!(ADDRESSBOOK.get(&liquidation.token).unwrap(), "WETH");
        assert_eq!(
            ADDRESSBOOK.get(&liquidation.as_ref().sent_token).unwrap(),
            "YFI"
        );
    }

    #[test]
    // https://etherscan.io/tx/0x46f4a4d409b44d85e64b1722b8b0f70e9713eb16d2c89da13cffd91486442627
    fn balancer_uni_arb() {
        let mut inspection =
            get_trace("0x46f4a4d409b44d85e64b1722b8b0f70e9713eb16d2c89da13cffd91486442627");

        let inspector = BatchInspector::new(
            vec![
                Box::new(ERC20::new()),
                Box::new(Uniswap::new()),
                Box::new(Curve::new()),
                Box::new(Balancer::new()),
            ],
            vec![
                Box::new(TradeReducer::new()),
                Box::new(ArbitrageReducer::new()),
            ],
        );
        inspector.inspect(&mut inspection);
        inspector.reduce(&mut inspection);
        inspection.prune();

        let known = inspection.known();
        let arb = known
            .iter()
            .find_map(|action| action.as_ref().arbitrage())
            .unwrap();
        assert_eq!(arb.profit, U256::from_dec_str("41108016724856778").unwrap());
        assert_eq!(arb.token, *WETH);
        assert_eq!(
            inspection.protocols,
            vec![Protocol::Uniswap, Protocol::Balancer]
        );
    }

    #[test]
    // https://etherscan.io/tx/0x1d9a2c8bfcd9f6e133c490d892fe3869bada484160a81966e645616cfc21652a
    fn balancer_uni_arb2() {
        let mut inspection =
            get_trace("0x1d9a2c8bfcd9f6e133c490d892fe3869bada484160a81966e645616cfc21652a");

        let inspector = BatchInspector::new(
            vec![
                Box::new(ERC20::new()),
                Box::new(Uniswap::new()),
                Box::new(Curve::new()),
                Box::new(Balancer::new()),
            ],
            vec![
                Box::new(TradeReducer::new()),
                Box::new(ArbitrageReducer::new()),
            ],
        );
        inspector.inspect(&mut inspection);
        inspector.reduce(&mut inspection);
        inspection.prune();

        let known = inspection.known();
        let arb = known
            .iter()
            .find_map(|action| action.as_ref().arbitrage())
            .unwrap();
        assert_eq!(arb.profit, U256::from_dec_str("47597234528640869").unwrap());
        assert_eq!(arb.token, *WETH);
        assert_eq!(
            inspection.protocols,
            vec![Protocol::Uniswap, Protocol::Balancer]
        );
    }

    #[test]
    fn curve_arb() {
        let mut inspection = read_trace("curve_arb.json");

        let inspector = BatchInspector::new(
            vec![
                Box::new(ERC20::new()),
                Box::new(Uniswap::new()),
                Box::new(Curve::new()),
            ],
            vec![
                Box::new(TradeReducer::new()),
                Box::new(ArbitrageReducer::new()),
            ],
        );
        inspector.inspect(&mut inspection);
        inspector.reduce(&mut inspection);
        inspection.prune();

        let known = inspection.known();

        let arb = known
            .iter()
            .find_map(|action| action.as_ref().arbitrage())
            .unwrap();
        assert_eq!(arb.profit, U256::from_dec_str("14397525374450478").unwrap());
        assert_eq!(arb.token, *WETH);
        assert_eq!(
            inspection.protocols,
            vec![Protocol::Sushiswap, Protocol::Curve]
        );
    }

    #[test]
    // https://etherscan.io/tx/0x1c85df1fa4c2e9fe7acc7bf204681aa0072b5df05e06bbc8e593777c0dfa5c1c
    fn bot_selfdestruct() {
        let mut inspection = read_trace("bot_selfdestruct.json");

        let inspector = BatchInspector::new(
            vec![
                Box::new(ERC20::new()),
                Box::new(Aave::new()),
                Box::new(Uniswap::new()),
                Box::new(Balancer::new()),
                Box::new(ZeroEx::new()),
                Box::new(Curve::new()),
            ],
            vec![
                Box::new(LiquidationReducer::new()),
                Box::new(TradeReducer::new()),
                Box::new(ArbitrageReducer::new()),
            ],
        );
        inspector.inspect(&mut inspection);
        inspector.reduce(&mut inspection);
        inspection.prune();

        let known = inspection.known();
        dbg!(&known);
        assert_eq!(inspection.status, Status::Reverted);
        assert_eq!(inspection.protocols, vec![Protocol::Uniswap])
    }

    #[test]
    // http://etherscan.io/tx/0x0e0e7c690589d9b94c3fbc4bae8abb4c5cac5c965abbb5bf1533e9f546b10b92
    fn dydx_aave_liquidation() {
        let mut inspection = read_trace("dydx_loan.json");

        let inspector = BatchInspector::new(
            vec![
                Box::new(ERC20::new()),
                Box::new(Aave::new()),
                Box::new(ZeroEx::new()),
                Box::new(Balancer::new()),
                Box::new(Uniswap::new()),
                Box::new(Curve::new()),
            ],
            vec![
                Box::new(LiquidationReducer::new()),
                Box::new(TradeReducer::new()),
                Box::new(ArbitrageReducer::new()),
            ],
        );
        inspector.inspect(&mut inspection);
        inspector.reduce(&mut inspection);
        inspection.prune();

        let known = inspection.known();
        assert_eq!(inspection.status, Status::Success);
        assert_eq!(
            inspection.protocols,
            vec![Protocol::Aave, Protocol::DyDx, Protocol::Uniswap]
        );
        let liquidation = known
            .iter()
            .find_map(|action| action.as_ref().profitable_liquidation())
            .unwrap();
        assert_eq!(
            liquidation.profit,
            U256::from_dec_str("18789801420638046861").unwrap()
        );
    }

    #[test]
    // http://etherscan.io/tx/0x97afae49a25201dbb34502d36a7903b51754362ceb231ff775c07db540f4a3d6
    // here the trader keeps the received asset (different than the one he used to repay)
    fn liquidation1() {
        let mut inspection = read_trace("liquidation_1.json");

        let inspector = BatchInspector::new(
            vec![
                Box::new(ERC20::new()),
                Box::new(Aave::new()),
                Box::new(ZeroEx::new()),
                Box::new(Balancer::new()),
                Box::new(Uniswap::new()),
                Box::new(Curve::new()),
            ],
            vec![
                Box::new(LiquidationReducer::new()),
                Box::new(TradeReducer::new()),
                Box::new(ArbitrageReducer::new()),
            ],
        );
        inspector.inspect(&mut inspection);
        inspector.reduce(&mut inspection);
        inspection.prune();

        let known = inspection.known();
        assert_eq!(inspection.status, Status::Success);
        assert_eq!(
            inspection.protocols,
            vec![Protocol::Aave, Protocol::Uniswappy]
        );
        let liquidation = known
            .iter()
            .find_map(|action| action.as_ref().liquidation())
            .unwrap();
        assert_eq!(ADDRESSBOOK.get(&liquidation.sent_token).unwrap(), "BAT");
        assert_eq!(ADDRESSBOOK.get(&liquidation.received_token).unwrap(), "DAI");
    }

    #[tokio::test]
    // This was a failed attempt at a triangular arb between zHEGIC/WETH, zHEGIC/HEGIC
    // and the HEGIC/WETH pools. The arb, if successful, would've yielded 0.1 ETH:
    // 1. Known bot sends 115 WETH to 0xa084 (their proxy)
    // 2. 0xa084 trades 3.583 WETH for zHEGIC
    // 3. trades zHEGIC for HEGIC
    // 4. trades HEGIC for 3.685 WETH whcih stays at 0xa084
    // 5. send the remaining 111 WETH back to known bot
    async fn reverted_arb_positive_revenue() {
        let mut inspection = read_trace("reverted_arb.json");

        let inspector = BatchInspector::new(
            vec![Box::new(ERC20::new()), Box::new(Uniswap::new())],
            vec![
                Box::new(TradeReducer::new()),
                Box::new(ArbitrageReducer::new()),
            ],
        );
        inspector.inspect(&mut inspection);
        inspector.reduce(&mut inspection);
        inspection.prune();

        let arb = inspection
            .known()
            .iter()
            .find_map(|x| x.as_ref().arbitrage())
            .cloned()
            .unwrap();
        assert_eq!(arb.profit.to_string(), "101664758086906735");
        assert_eq!(inspection.status, Status::Reverted);
    }
}
