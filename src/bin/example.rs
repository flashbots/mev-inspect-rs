#![allow(unused)]
#![allow(dead_code)]
use mev_inspect::model::EventLog;
use mev_inspect::types::evaluation::ActionType;
use mev_inspect::types::Protocol;
use mev_inspect::MevDB;
use std::str::FromStr;
use tokio_postgres::Config;

use ethers::providers::Http;
use ethers::types::{Filter, Log, Trace, U64};
use ethers::{
    providers::{Middleware, Provider},
    types::{BlockNumber, TxHash, U256},
};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::convert::TryFrom;

#[derive(Serialize, Deserialize)]
pub struct TxInfo {
    pub traces: Vec<Trace>,
    pub logs: Vec<Log>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::init();

    // let db = MevDB::connect(
    //     Config::from_str("postgres://mev_rs_user@localhost/mev_inspections")?,
    //     "mev_inspections",
    // )
    // .await?;

    let provider = Provider::<Http>::try_from(
        "https://mainnet.infura.io/v3/e49df99733844499bc5d19fa97bc9367",
    )?;

    let input = std::fs::read_to_string("res/zapper1.json").unwrap();
    let traces: Vec<Trace> = serde_json::from_str(&input).unwrap();

    let block: u64 = 11095494;
    let logs = provider
        .get_logs(&Filter::new().from_block(block).to_block(block))
        .await?;

    println!("fetched logs");

    let mut tx_logs = logs
        .into_iter()
        .into_group_map_by(|log| log.transaction_hash.expect("exists"));

    let mut entries = Vec::with_capacity(tx_logs.len());

    for (tx, traces) in traces
        .into_iter()
        .filter(|t| t.transaction_hash.is_some())
        .group_by(|t| t.transaction_hash.expect("tx hash exists"))
        .into_iter()
    {
        let traces = traces.collect();
        let logs = tx_logs.remove(&tx).unwrap_or_default();
        entries.push(TxInfo { traces, logs })
    }

    let json = serde_json::to_string_pretty(&entries).unwrap();
    std::fs::write("res/zapper1.data.json", &json).unwrap();

    println!("done");
    // let evals = db
    //     .select_where_eoa("0x74dec05e5b894b0efec69cdf6316971802a2f9a1".parse()?)
    //     .await?;
    // let evals = db.select_where_actions(&[ActionType::Arbitrage]).await?;

    // dbg!(evals);

    Ok(())
}
