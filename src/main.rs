use mev_inspect::{
    inspectors::{Aave, Compound, Uniswap},
    reducers::{ArbitrageReducer, LiquidationReducer, TradeReducer},
    types::Evaluation,
    BatchInspector, CachedProvider, HistoricalPrice, Inspector, MevDB, Reducer,
};

use ethers::{
    providers::{Middleware, Provider, StreamExt},
    types::{BlockNumber, TxHash},
};

use gumdrop::Options;
use std::{convert::TryFrom, path::PathBuf, sync::Arc};

#[derive(Debug, Options, Clone)]
struct Opts {
    help: bool,

    #[options(help = "clear and re-build the database")]
    reset: bool,

    #[options(
        default = "http://localhost:8545",
        help = "The tracing / archival node's URL"
    )]
    url: String,

    #[options(default = "res", help = "Path to where traces will be cached")]
    cache: PathBuf,

    // Postgres  Config
    #[options(default = "localhost", help = "the database's url")]
    db_url: String,
    #[options(default = "postgres", help = "the user of the database")]
    db_user: String,
    #[options(default = "mev_inspections", help = "the table of the database")]
    db_table: String,

    // Single tx or many blocks
    #[options(command)]
    cmd: Option<Command>,
}

#[derive(Debug, Options, Clone)]
enum Command {
    #[options(help = "inspect a transaction")]
    Tx(TxOpts),
    #[options(help = "inspect a range of blocks")]
    Blocks(BlockOpts),
}

#[derive(Debug, Options, Clone)]
struct TxOpts {
    help: bool,
    #[options(free, help = "the transaction's hash")]
    tx: TxHash,
}
#[derive(Debug, Options, Clone)]
struct BlockOpts {
    help: bool,
    #[options(help = "the block to start tracing from")]
    from: u64,
    #[options(help = "the block to finish tracing at")]
    to: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Opts::parse_args_default_or_exit();

    // Instantiate the provider and read from the cached files if needed
    let provider = CachedProvider::new(Provider::try_from(opts.url.as_str())?, opts.cache);

    // Instantiate the thing which will query historical prices
    let prices = HistoricalPrice::new(provider.clone());

    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(Uniswap::new()),
        Box::new(Aave::new()),
        Box::new(Compound::create(Arc::new(provider.clone())).await?),
    ];

    let reducers: Vec<Box<dyn Reducer>> = vec![
        Box::new(LiquidationReducer::new()),
        Box::new(TradeReducer::new()),
        Box::new(ArbitrageReducer::new()),
    ];
    let processor = BatchInspector::new(inspectors, reducers);

    let mut db = MevDB::connect("127.0.0.1", "postgres", "mev_inspections").await?;
    if opts.reset {
        db.clear().await?;
        db.create().await?;
    }

    if let Some(cmd) = opts.cmd {
        match cmd {
            Command::Tx(opts) => {
                let traces = provider.trace_transaction(opts.tx).await?;
                if let Some(inspection) = processor.inspect_one(traces) {
                    let evaluation = Evaluation::new(inspection, &provider, &prices).await?;
                    println!("Found: {:?}", evaluation.as_ref().hash);
                    println!("Revenue: {:?}", evaluation.profit);
                    println!("Cost: {:?}", evaluation.gas_used * evaluation.gas_price);
                    println!("Actions: {:?}", evaluation.actions);
                    println!("Protocols: {:?}", evaluation.inspection.protocols);

                    if !db.exists(opts.tx).await? {
                        db.insert(&evaluation).await?;
                    } else {
                        eprintln!("Tx already in the database, skipping insertion.");
                    }
                } else {
                    eprintln!("No actions found for tx {:?}", opts.tx);
                }
            }
            Command::Blocks(opts) => {
                let t1 = std::time::Instant::now();
                for block in opts.from..opts.to {
                    process_block(block, &provider, &processor, &mut db, &prices).await?;
                }

                println!(
                    "Processed {} blocks in {:?}",
                    opts.to - opts.from,
                    std::time::Instant::now().duration_since(t1)
                );
            }
        };
    } else {
        let mut watcher = provider.watch_blocks().await?;
        while let Some(_) = watcher.next().await {
            let block = provider.get_block_number().await?;
            println!("Got block: {}", block.as_u64());
            process_block(block, &provider, &processor, &mut db, &prices).await?;
        }
    }

    Ok(())
}

async fn process_block<T: Into<BlockNumber>, M: Middleware + 'static>(
    block: T,
    provider: &M,
    processor: &BatchInspector,
    db: &mut MevDB<'_>,
    prices: &HistoricalPrice<M>,
) -> anyhow::Result<()> {
    let block = block.into();
    let traces = provider.trace_block(block).await?;
    let inspections = processor.inspect_many(traces);

    let t1 = std::time::Instant::now();

    let eval_futs = inspections
        .into_iter()
        .map(|inspection| Evaluation::new(inspection, provider, &prices));
    for evaluation in futures::future::join_all(eval_futs).await {
        if let Ok(evaluation) = evaluation {
            db.insert(&evaluation).await?;
        }
    }

    println!(
        "Processed {:?} in {:?}",
        block,
        std::time::Instant::now().duration_since(t1)
    );
    Ok(())
}
