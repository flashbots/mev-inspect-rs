use mev_inspect::{
    inspectors::{Aave, Uniswap},
    reducers::{ArbitrageReducer, LiquidationReducer, TradeReducer},
    types::Evaluation,
    BatchInspector, CachedProvider, Inspector, MevDB, Reducer,
};

use ethers::{
    providers::{Middleware, Provider},
    types::TxHash,
};

use gumdrop::Options;
use std::{convert::TryFrom, path::PathBuf};

#[derive(Debug, Options, Clone)]
struct Opts {
    help: bool,

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
    let cmd = if let Some(cmd) = opts.cmd {
        cmd
    } else {
        eprintln!("No command supplied.");
        eprintln!("Usage: mev-inspect [OPTIONS]");
        eprintln!("{}", Opts::usage());
        return Ok(());
    };

    // Instantiate the provider and read from the cached files if needed
    let provider = CachedProvider::new(Provider::try_from(opts.url.as_str())?, opts.cache);

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

    match cmd {
        Command::Tx(opts) => {
            let traces = provider.trace_transaction(opts.tx).await?;
            if let Some(inspection) = processor.inspect_one(traces) {
                let evaluation = Evaluation::new(inspection, &provider).await?;
                println!("Found: {:?}", evaluation.as_ref().hash);
                println!("Revenue: {:?}", evaluation.profit);
                println!("Cost: {:?}", evaluation.gas_used * evaluation.gas_price);
                println!("Actions: {:?}", evaluation.actions);
                db.insert(&evaluation).await?;
            } else {
                eprintln!("No actions found for tx {:?}", opts.tx);
            }
        }
        Command::Blocks(opts) => {
            for block in opts.from..opts.to {
                let traces = provider.trace_block(block.into()).await?;
                let inspections = processor.inspect_many(traces);

                for inspection in inspections {
                    let evaluation = Evaluation::new(inspection, &provider).await?;
                    db.insert(&evaluation).await?;
                }
            }
        }
    };

    Ok(())
}
