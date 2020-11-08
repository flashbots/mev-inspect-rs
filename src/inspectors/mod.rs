//! # Inspectors
//!
//! All inspectors go here. An inspector is an implementer of the `Inspector`
//! trait and is responsible for decoding a `Trace` in isolation. No sub-trace
//! specific logic needs to be written.
use ethers::types::Trace;
use itertools::Itertools;

/// A Uniswap inspector
mod uniswap;
pub use uniswap::Uniswap;

/// An Aave inspector
mod aave;
pub use aave::Aave;

mod arb;
pub use arb::ArbitrageReducer;

/// ERC20 Inspector, to be used for parsing subtraces involving transfer/transferFrom
mod erc20;

pub mod types;
use types::{Classification, Inspection};

/// All the protocol addresses
mod addresses;

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

    /// Decodes the inspection's actions
    pub fn inspect(&self, inspection: &mut Inspection) {
        for inspector in self.inspectors.iter() {
            inspector.inspect(inspection);
        }
    }
}

/// Trait for defining an inspector for a specific DeFi protocol
pub trait Inspector: Reducer {
    /// Classifie an inspection's actions
    fn classify(&self, inspection: &mut Inspection);

    fn inspect(&self, inspection: &mut Inspection) {
        // 1. Classify unknown ones
        self.classify(inspection);

        // 2. Remove pruned ones
        prune(inspection);

        // 3. Reduce / combine actions
        self.reduce(inspection);

        // 4. Prune again after the reduction
        prune(inspection);
    }
}

// TODO: Is there a better way to do this without re-allocating?
// Maybe this? https://doc.rust-lang.org/std/vec/struct.DrainFilter.html
pub fn prune(inspection: &mut Inspection) {
    inspection.actions = inspection
        .actions
        .iter()
        .filter(|action| match action {
            // Remove any of the pruned calls
            Classification::Prune => false,
            // Remove calls with 2300 gas as they are probably due to
            // the gas stipend for low level calls, which we've already
            // taken into account.
            Classification::Unknown(call) => call.as_ref().gas != 2300.into(),
            Classification::Known(_) => true,
        })
        .cloned()
        .collect_vec();
}

pub trait Reducer {
    /// By default the reducer is empty. A consumer may optionally
    /// implement this method to perform additional actions on the classified &
    /// filtered results.
    fn reduce(&self, _: &mut Inspection) {}
}

// TODO: Add tests
