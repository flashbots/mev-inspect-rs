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

        let liquidation = known
            .iter()
            .find_map(|action| action.as_ref().profitable_liquidation())
            .unwrap();
        assert_eq!(
            liquidation.profit,
            U256::from_dec_str("11050220339336343871").unwrap()
        );

        assert_eq!(
            inspection.protocols,
            // SushiSwap is touched in a static call. The bot probably
            // checked whether it was more profitable to trade the
            // ETH for YFI on Sushi or Uni
            vec![Protocol::Uniswap, Protocol::Sushiswap, Protocol::Aave]
        );

        assert_eq!(ADDRESSBOOK.get(&liquidation.token).unwrap(), "ETH");
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
                Box::new(ZeroEx::new()),
                Box::new(ERC20::new()),
                Box::new(Uniswap::new()),
                Box::new(Balancer::new()),
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
}
