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

/// Batch Inspector which tries to decode traces using
/// multiple inspectors
pub use inspectors::BatchInspector;

#[cfg(test)]
mod test_helpers;
