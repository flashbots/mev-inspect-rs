use crate::{types::Inspection, Inspector};
use ethers::types::Trace;
use itertools::Itertools;

/// Classifies traces according to the provided inspectors
pub struct BatchInspector {
    inspectors: Vec<Box<dyn Inspector>>,
}

impl BatchInspector {
    /// Constructor
    pub fn new(inspectors: Vec<Box<dyn Inspector>>) -> Self {
        Self { inspectors }
    }

    /// Given a trace iterator, it groups all traces for the same tx hash
    /// and then inspects them and all of their subtraces
    pub fn inspect_many<'a>(&'a self, traces: impl IntoIterator<Item = Trace>) -> Vec<Inspection> {
        // group traces in a block by tx hash
        let traces = traces.into_iter().group_by(|t| t.transaction_hash);

        let inspections = traces
            .into_iter()
            // Convert the traces to inspections
            .map(|(_, traces)| Inspection::from(traces))
            // Make an unclassified inspection per tx_hash containing a tree of traces
            .map(|mut i| {
                self.inspect(&mut i);
                i
            })
            .collect();

        // TODO: Convert these inspections to known/unknown Evaluations
        // containing profit-related data.
        inspections
    }

    // pub fn reduce(&self, inspection: Inspection) {}

    /// Decodes the inspection's actions
    pub fn inspect(&self, inspection: &mut Inspection) {
        for inspector in self.inspectors.iter() {
            inspector.inspect(inspection);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        inspectors::{Aave, Uniswap},
        test_helpers::*,
    };

    #[test]
    #[ignore]
    // call that starts from a bot but has a uniswap sub-trace
    // https://etherscan.io/tx/0x93690c02fc4d58734225d898ea4091df104040450c0f204b6bf6f6850ac4602f
    // 99k USDC -> 281 ETH -> 5.7 YFI trade
    // Liquidator Repay -> 5.7 YFI
    // Liquidation -> 292 ETH
    // Profit: 11 ETH
    fn subtrace_parse() {
        let mut inspection =
            get_trace("0x93690c02fc4d58734225d898ea4091df104040450c0f204b6bf6f6850ac4602f");

        let inspector = BatchInspector::new(vec![Box::new(Uniswap::new()), Box::new(Aave::new())]);

        inspector.inspect(&mut inspection);

        dbg!(&inspection);
        // dbg!(&inspection.known());
        // dbg!(inspection.known());
        // dbg!(inspection.unknown().len());
        // dbg!(inspection.known().len());
        // dbg!(inspection.protocols);
    }
}
