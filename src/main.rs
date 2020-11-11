use ethers::providers::{Middleware, Provider};
use mev_inspect::{
    inspectors::{Aave, Uniswap},
    reducers::{ArbitrageReducer, LiquidationReducer, TradeReducer},
    types::Evaluation,
    BatchInspector, CachedProvider, Inspector, MevDB, Reducer,
};
use std::convert::TryFrom;
use std::env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cache: String = env::var("CACHE").unwrap_or("res".to_owned());

    // TODO: Convert these to CLI params
    let url: String = env::var("ETHEREUM_URL").unwrap_or("http://localhost:8545".to_owned());
    let block_start: u64 = env::var("BLOCK_START")
        .unwrap_or("11003919".to_owned())
        .parse()?;
    let block_end: u64 = env::var("BLOCK_END").unwrap_or("0".to_owned()).parse()?;

    // Instantiate the provider and read from the cached files if needed
    let provider = CachedProvider::new(Provider::try_from(url.as_str())?, cache);

    // Use the Uniswap / Aave inspectors
    let inspectors: Vec<Box<dyn Inspector>> = vec![Box::new(Uniswap::new()), Box::new(Aave::new())];

    let reducers: Vec<Box<dyn Reducer>> = vec![
        Box::new(LiquidationReducer::new()),
        Box::new(TradeReducer::new()),
        Box::new(ArbitrageReducer::new()),
    ];
    let processor = BatchInspector::new(inspectors, reducers);

    let mut db = MevDB::connect("127.0.0.1", "postgres", "mev_inspections").await?;
    // TODO: Remove these so that the db isn't reset every time
    db.clear().await?;
    db.create().await?;

    for block in block_start..block_end {
        let traces = provider.trace_block(block.into()).await?;
        let inspections = processor.inspect_many(traces);

        for inspection in inspections {
            let evaluation = Evaluation::new(inspection, &provider).await?;
            db.insert(&evaluation).await?;
        }
    }

    Ok(())
}
