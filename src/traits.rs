use crate::types::{Inspection, Status};

pub trait Reducer {
    /// By default the reducer is empty. A consumer may optionally
    /// implement this method to perform additional actions on the classified &
    /// filtered results.
    fn reduce(&self, _: &mut Inspection);
}

/// Trait for defining an inspector for a specific DeFi protocol
pub trait Inspector {
    /// Classifies an inspection's actions
    fn classify(&self, inspection: &mut Inspection);

    fn inspect(&self, inspection: &mut Inspection) {
        // 1. Classify unknown ones
        self.classify(inspection);

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
