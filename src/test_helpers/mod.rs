use crate::types::{inspection::TraceWrapper, Classification, Inspection, Status};
use ethers::signers::Signer;
use ethers::types::{Address, Trace, TxHash};
use once_cell::sync::Lazy;
use std::{collections::HashSet, convert::TryInto};

pub const TRACE: &str = include_str!("../../res/11017338.trace.json");
pub static TRACES: Lazy<Vec<Trace>> = Lazy::new(|| serde_json::from_str(TRACE).unwrap());

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
    }
}

pub fn read_trace(path: &str) -> Inspection {
    let input = std::fs::read_to_string(format!("res/{}", path)).unwrap();
    let traces: Vec<Trace> = serde_json::from_str(&input).unwrap();
    TraceWrapper(traces).try_into().unwrap()
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
