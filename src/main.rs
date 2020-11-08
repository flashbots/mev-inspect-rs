use ethers::providers::{Middleware, Provider};
use mev_inspect::{
    inspectors::{Aave, Uniswap},
    BatchInspector,
};
use std::convert::TryFrom;
use std::env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // TODO: Convert these to CLI params
    let url: String = env::var("ETHEREUM_URL").unwrap_or("http://localhost:8545".to_owned());
    let block_start: u64 = env::var("BLOCK_START")
        .unwrap_or("11003919".to_owned())
        .parse()?;
    let block_end: u64 = env::var("BLOCK_START")?.parse()?;

    // Instantiate the provider
    let provider = Provider::try_from(url.as_str())?;
    // Use the Uniswap / Aave inspectors
    let processor = BatchInspector::new(vec![Box::new(Uniswap::new()), Box::new(Aave::new())]);

    for block in block_start..block_end {
        let traces = provider.trace_block(block.into()).await?;
        let inspections = processor.inspect_many(traces);

        // TODO: Do further processing on the inspected data
        // TODO: Publish the data to a database (postgres?)
        dbg!(inspections);
    }

    Ok(())
}
