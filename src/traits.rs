use crate::types::{Inspection, Status};

pub trait Reducer {
    /// By default the reducer is empty. A consumer may optionally
    /// implement this method to perform additional actions on the classified &
    /// filtered results.
    fn reduce(&self, _: &mut Inspection) {}
}

/// Trait for defining an inspector for a specific DeFi protocol
pub trait Inspector: Reducer {
    /// Classifie an inspection's actions
    fn classify(&self, inspection: &mut Inspection);

    fn inspect(&self, inspection: &mut Inspection) {
        // 1. Classify unknown ones
        self.classify(inspection);

        // 2. Remove pruned ones
        inspection.prune();

        // 3. Reduce / combine actions
        self.reduce(inspection);

        // 4. Prune again after the reduction
        inspection.prune();

        // If there are no classified actions and if there were checked protocols
        // in this transaction, then that means there was an arb check which reverted early
        if !inspection.protocols.is_empty()
            && inspection.actions.iter().any(|x| x.to_action().is_none())
        {
            inspection.status = Status::Checked;
        }
    }
}
