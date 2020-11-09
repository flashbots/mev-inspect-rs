use ethers::providers::{Middleware, Provider};
use mev_inspect::{
    inspectors::{Aave, Uniswap},
    types::Evaluation,
    BatchInspector, Inspector,
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
    let block_end: u64 = env::var("BLOCK_END")?.parse()?;

    // Instantiate the provider
    let provider = Provider::try_from(url.as_str())?;

    // Use the Uniswap / Aave inspectors
    let inspectors: Vec<Box<dyn Inspector>> = vec![Box::new(Uniswap::new()), Box::new(Aave::new())];
    let processor = BatchInspector::new(inspectors);

    for block in block_start..block_end {
        // TODO: Cache! Load from a cache if the block.json exists. Once the trace
        // gets downloaded, save it to a cache dir.
        let traces = provider.trace_block(block.into()).await?;
        let inspections = processor.inspect_many(traces);

        let mut evaluations = Vec::new();
        for inspection in inspections {
            let evaluation = Evaluation::new(inspection, &provider).await?;
            evaluations.push(evaluation);
        }

        // TODO: Publish the data to a database (postgres?)
        dbg!(evaluations);
    }

    Ok(())
}
