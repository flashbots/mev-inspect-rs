#![allow(unused)]
#![allow(dead_code)]
use mev_inspect::types::evaluation::ActionType;
use mev_inspect::types::Protocol;
use mev_inspect::MevDB;
use std::str::FromStr;
use tokio_postgres::Config;

use ethers::providers::Http;
use ethers::types::{Filter, U64};
use ethers::{
    providers::{Middleware, Provider},
    types::{BlockNumber, TxHash, U256},
};
use std::convert::TryFrom;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::init();

    let db = MevDB::connect(
        Config::from_str("postgres://mev_rs_user@localhost/mev_inspections")?,
        "mev_inspections",
    )
    .await?;

    let provider = Provider::<Http>::try_from(
        "https://mainnet.infura.io/v3/e49df99733844499bc5d19fa97bc9367",
    )?;

    let logs = provider
        .get_logs(&Filter::new().from_block(12155939u64).to_block(12155939u64))
        .await?;

    dbg!(logs.len());

    // let evals = db
    //     .select_where_eoa("0x74dec05e5b894b0efec69cdf6316971802a2f9a1".parse()?)
    //     .await?;
    // let evals = db.select_where_actions(&[ActionType::Arbitrage]).await?;

    // dbg!(evals);

    Ok(())
}
