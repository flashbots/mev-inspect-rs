use crate::model::EventLog;
use crate::types::{inspection::TraceWrapper, Classification, Inspection, Status, TransactionData};
use ethers::types::{Address, Log, Trace, TxHash};
use once_cell::sync::Lazy;
use serde::__private::TryFrom;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, convert::TryInto};

pub const TRACE: &str = include_str!("../../res/11017338.trace.json");
pub static TRACES: Lazy<Vec<Trace>> = Lazy::new(|| serde_json::from_str(TRACE).unwrap());

pub const TXINFO: &str = include_str!("../../res/11017338.data.json");
pub static TXINFOS: Lazy<Vec<TxInfo>> = Lazy::new(|| serde_json::from_str(TXINFO).unwrap());

#[derive(Clone, Serialize, Deserialize)]
pub struct TxInfo {
    pub traces: Vec<Trace>,
    pub logs: Vec<Log>,
}

pub fn addrs() -> Vec<Address> {
    use ethers::core::rand::thread_rng;
    (0..10)
        .into_iter()
        .map(|_| ethers::signers::LocalWallet::new(&mut thread_rng()).address())
        .collect()
}

pub fn mk_inspection(actions: Vec<Classification>) -> Inspection {
    Inspection {
        status: Status::Success,
        actions,
        protocols: HashSet::new(),
        from: Address::zero(),
        contract: Address::zero(),
        proxy_impl: None,
        hash: TxHash::zero(),
        block_number: 0,
        transaction_position: 0,
        internal_calls: vec![],
        logs: vec![],
    }
}

pub fn read_trace(path: &str) -> Inspection {
    let input = std::fs::read_to_string(format!("res/{}", path)).unwrap();
    let traces: Vec<Trace> = serde_json::from_str(&input).unwrap();
    TraceWrapper(traces).try_into().unwrap()
}

pub fn read_tx(path: &str) -> TransactionData {
    let input = std::fs::read_to_string(format!("res/{}", path)).unwrap();
    let TxInfo { traces, logs } = serde_json::from_str(&input).unwrap();
    let logs = logs
        .into_iter()
        .filter_map(|log| EventLog::try_from(log).ok())
        .collect();
    TransactionData::create(traces.into_iter(), logs).unwrap()
}

pub fn get_tx(hash: &str) -> TransactionData {
    let hash = if hash.starts_with("0x") {
        &hash[2..]
    } else {
        hash
    };

    TXINFOS
        .iter()
        .filter(|t| t.traces[0].transaction_hash == Some(hash.parse::<TxHash>().unwrap()))
        .cloned()
        .map(|t| {
            (
                t.traces,
                t.logs
                    .into_iter()
                    .filter_map(|log| EventLog::try_from(log).ok())
                    .collect::<Vec<_>>(),
            )
        })
        .filter_map(|(traces, logs)| TransactionData::create(traces.into_iter(), logs).ok())
        .next()
        .unwrap()
}

pub fn get_trace(hash: &str) -> Inspection {
    let hash = if hash.starts_with("0x") {
        &hash[2..]
    } else {
        hash
    };

    TraceWrapper(
        TRACES
            .iter()
            .filter(|t| t.transaction_hash == Some(hash.parse::<TxHash>().unwrap()))
            .cloned()
            .collect::<Vec<_>>(),
    )
    .try_into()
    .unwrap()
}

#[macro_export]
macro_rules! set {
    ( $( $x:expr ),* ) => {  // Match zero or more comma delimited items
        {
            let mut temp_set = std::collections::HashSet::new();  // Create a mutable HashSet
            $(
                temp_set.insert($x); // Insert each item matched into the HashSet
             )*
                temp_set // Return the populated HashSet
        }
    };
}
