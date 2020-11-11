//! MEV-INSPECT
//!
//! Utility for MEV Inspection
//!
//! - Inspectors
//!     - UniswapV2 (and clones)
//! - Processor
//! - Database
//!     - PostGres (maybe Influx) + Grafana?

/// MEV Inspectors
pub mod inspectors;

/// Reducers
pub mod reducers;

/// Batch Inspector which tries to decode traces using
/// multiple inspectors
pub use inspectors::BatchInspector;

/// Types for MEV-INSPECT
pub mod types;

/// Various addresses which are found among protocols
pub mod addresses;

mod cached_provider;
pub use cached_provider::CachedProvider;

#[cfg(test)]
mod test_helpers;

mod traits;
pub use traits::*;

/// PostGres trait implementations
mod mevdb;
pub use mevdb::MevDB;

mod prices;
pub use prices::HistoricalPrice;

/// Checks if `a2` is a subtrace of `a1`
pub fn is_subtrace(a1: &[usize], a2: &[usize]) -> bool {
    if a1.is_empty() {
        return false;
    }

    a1 == &a2[..std::cmp::min(a1.len(), a2.len())]
}

#[cfg(test)]
mod tests {
    use super::is_subtrace;

    #[test]
    fn check() {
        let test_cases = vec![
            (vec![0], vec![0, 1], true),
            (vec![0], vec![0, 0], true),
            (vec![0, 1], vec![0, 1, 0], true),
            (vec![0, 1], vec![0, 1, 1], true),
            (vec![0, 1], vec![0, 2], false),
            (vec![0, 1], vec![0], false),
            (vec![], vec![0, 1], false),
            (vec![15], vec![15, 0, 3, 22, 0, 0], true),
        ];

        for (a1, a2, expected) in test_cases {
            assert_eq!(is_subtrace(&a1, &a2), expected);
        }
    }
}
