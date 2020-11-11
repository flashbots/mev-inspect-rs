use crate::{types::Inspection, Inspector, Reducer};
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

    pub fn inspect_one<'a>(
        &'a self,
        traces: impl IntoIterator<Item = Trace>,
    ) -> Option<Inspection> {
        let mut i = Inspection::from(traces);
        if !i.actions.is_empty() {
            self.inspect(&mut i);
            self.reduce(&mut i);
            Some(i)
        } else {
            None
        }
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
        addresses::ADDRESSBOOK,
        inspectors::{Aave, Uniswap},
        reducers::{ArbitrageReducer, LiquidationReducer, TradeReducer},
        test_helpers::*,
        types::Protocol,
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
            vec![Box::new(Uniswap::new()), Box::new(Aave::new())],
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
    // https://etherscan.io/tx/0x1d9a2c8bfcd9f6e133c490d892fe3869bada484160a81966e645616cfc21652a
    fn balancer_uni_arb() {
        let mut inspection =
            get_trace("0x46f4a4d409b44d85e64b1722b8b0f70e9713eb16d2c89da13cffd91486442627");
        let uni = Uniswap::new();
        uni.inspect(&mut inspection);
    }
}
