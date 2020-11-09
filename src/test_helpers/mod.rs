use crate::types::{
    actions::{Arbitrage, SpecificAction, Trade, Transfer},
    Classification, Inspection,
};
use ethers::types::{Trace, TxHash};
use once_cell::sync::Lazy;

pub const TRACE: &str = include_str!("../../res/11017338.trace.json");
pub static TRACES: Lazy<Vec<Trace>> = Lazy::new(|| serde_json::from_str(TRACE).unwrap());

pub fn read_trace(path: &str) -> Inspection {
    let input = std::fs::read_to_string(format!("res/{}", path)).unwrap();
    let traces: Vec<Trace> = serde_json::from_str(&input).unwrap();
    traces.into()
}

pub fn get_trace(hash: &str) -> Inspection {
    let hash = if hash.starts_with("0x") {
        &hash[2..]
    } else {
        hash
    };

    TRACES
        .iter()
        .filter(|t| t.transaction_hash == Some(hash.parse::<TxHash>().unwrap()))
        .cloned()
        .collect::<Vec<_>>()
        .into()
}

pub fn to_transfer(action: &Classification) -> Transfer {
    match action {
        Classification::Known(action) => match action.as_ref() {
            SpecificAction::Transfer(ref t) => t.clone(),
            _ => unreachable!("Non-transfer found"),
        },
        _ => panic!("could not classify"),
    }
}

pub fn to_trade(action: &Classification) -> Trade {
    match action {
        Classification::Known(action) => match action.as_ref() {
            SpecificAction::Trade(ref t) => t.clone(),
            _ => unreachable!("Non-trade found"),
        },
        _ => panic!("could not classify"),
    }
}

pub fn to_arb(action: &Classification) -> Arbitrage {
    match action {
        Classification::Known(action) => match action.as_ref() {
            SpecificAction::Arbitrage(ref t) => t.clone(),
            _ => unreachable!("Non-arb found"),
        },
        _ => panic!("could not classify"),
    }
}

pub fn is_weth(action: &Classification, deposit: bool) -> bool {
    match action {
        Classification::Known(action) => match action.as_ref() {
            SpecificAction::WethDeposit { .. } => deposit,
            SpecificAction::WethWithdrawal { .. } => !deposit,
            _ => false,
        },
        _ => false,
    }
}
