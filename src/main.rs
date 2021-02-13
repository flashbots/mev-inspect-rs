use mev_inspect::{
    inspectors::{Aave, Balancer, Compound, Curve, Uniswap, ZeroEx, ERC20},
    reducers::{ArbitrageReducer, LiquidationReducer, TradeReducer},
    types::Evaluation,
    BatchInspector, CachedProvider, HistoricalPrice, Inspector, MevDB, Reducer,
};

use ethers::{
    providers::{Middleware, Provider, StreamExt},
    types::{BlockNumber, TxHash, U256},
};

use gumdrop::Options;
use std::io::Write;
use std::{collections::HashMap, convert::TryFrom, path::PathBuf, sync::Arc};

#[derive(Debug, Options, Clone)]
struct Opts {
    help: bool,

    #[options(help = "clear and re-build the database")]
    reset: bool,

    #[options(help = "do not skip blocks which already exist")]
    overwrite: bool,

    #[options(
        default = "http://localhost:8545",
        help = "The tracing / archival node's URL"
    )]
    url: String,

    #[options(help = "Path to where traces will be cached")]
    cache: Option<PathBuf>,

    #[options(help = "Database config")]
    db_cfg: tokio_postgres::Config,
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
    #[options(help = "inspect a vector of txs")]
    Txs(TxsOpts),
    #[options(help = "inspect a range of blocks")]
    Blocks(BlockOpts),
}

#[derive(Debug, Options, Clone)]
struct TxsOpts {
    help: bool,
    #[options(free, help = "path to the csv file with all your tx hashes")]
    path: PathBuf,
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
    if let Some(ref cache) = opts.cache {
        let provider = CachedProvider::new(Provider::try_from(opts.url.as_str())?, cache);
        run(provider, opts).await
    } else {
        let provider = Provider::try_from(opts.url.as_str())?;
        run(provider, opts).await
    }
}

async fn run<M: Middleware + Clone + 'static>(provider: M, opts: Opts) -> anyhow::Result<()> {
    let provider = Arc::new(provider);
    // Instantiate the thing which will query historical prices
    let prices = HistoricalPrice::new(provider.clone());

    let compound = Compound::create(provider.clone()).await?;
    let curve = Curve::create(provider.clone()).await?;
    let inspectors: Vec<Box<dyn Inspector>> = vec![
        // Classify Transfers
        Box::new(ZeroEx::new()),
        Box::new(ERC20::new()),
        // Classify AMMs
        Box::new(Balancer::new()),
        Box::new(Uniswap::new()),
        Box::new(curve),
        // Classify Liquidations
        Box::new(Aave::new()),
        Box::new(compound),
    ];

    let reducers: Vec<Box<dyn Reducer>> = vec![
        Box::new(LiquidationReducer::new()),
        Box::new(TradeReducer::new()),
        Box::new(ArbitrageReducer::new()),
    ];
    let processor = BatchInspector::new(inspectors, reducers);

    let mut db = MevDB::connect(opts.db_cfg, &opts.db_table).await?;
    db.create().await?;
    if opts.reset {
        db.clear().await?;
        db.create().await?;
    }

    if let Some(cmd) = opts.cmd {
        match cmd {
            Command::Txs(opts) => {
                use std::io::BufRead;
                let file = std::fs::File::open(opts.path).unwrap();
                let lines = std::io::BufReader::new(file).lines();
                let tx_hashes: Vec<_> = lines
                    .map(|line| line.unwrap().parse::<TxHash>().unwrap())
                    .collect();
                for tx_hash in tx_hashes {
                    let traces = provider.trace_transaction(tx_hash).await?;
                    let inspection = processor.inspect_one(traces).unwrap();
                    let gas_used = provider
                        .get_transaction_receipt(inspection.hash)
                        .await?
                        .expect("tx not found")
                        .gas_used
                        .unwrap_or_default();

                    let gas_price = provider
                        .get_transaction(inspection.hash)
                        .await?
                        .expect("tx not found")
                        .gas_price;

                    let evaluation =
                        Evaluation::new(inspection, &prices, gas_used, gas_price).await?;

                    db.delete(tx_hash).await?;
                    db.insert(&evaluation).await?;
                    println!("Corrected {:?}", tx_hash);
                }
            }
            Command::Tx(opts) => {
                let traces = provider.trace_transaction(opts.tx).await?;
                if let Some(inspection) = processor.inspect_one(traces) {
                    let gas_used = provider
                        .get_transaction_receipt(inspection.hash)
                        .await?
                        .expect("tx not found")
                        .gas_used
                        .unwrap_or_default();

                    let gas_price = provider
                        .get_transaction(inspection.hash)
                        .await?
                        .expect("tx not found")
                        .gas_price;

                    let evaluation =
                        Evaluation::new(inspection, &prices, gas_used, gas_price).await?;
                    println!("Found: {:?}", evaluation.as_ref().hash);
                    println!("Revenue: {:?} WEI", evaluation.profit);
                    println!("Cost: {:?} WEI", evaluation.gas_used * evaluation.gas_price);
                    println!("Actions: {:?}", evaluation.actions);
                    println!("Protocols: {:?}", evaluation.inspection.protocols);
                    println!("Status: {:?}", evaluation.inspection.status);
                    db.insert(&evaluation).await?;
                } else {
                    eprintln!("No actions found for tx {:?}", opts.tx);
                }
            }
            Command::Blocks(inner) => {
                let t1 = std::time::Instant::now();
                let stdout = std::io::stdout();
                let mut lock = stdout.lock();
                for block in inner.from..inner.to {
                    // TODO: Can we do the block processing in parallel? Theoretically
                    // it should be possible
                    process_block(&mut lock, block, &provider, &processor, &mut db, &prices)
                        .await?;
                }
                drop(lock);

                println!(
                    "Processed {} blocks in {:?}",
                    inner.to - inner.from,
                    std::time::Instant::now().duration_since(t1)
                );
            }
        };
    } else {
        let mut watcher = provider.watch_blocks().await?;
        while watcher.next().await.is_some() {
            let block = provider.get_block_number().await?;
            let stdout = std::io::stdout();
            let mut lock = stdout.lock();
            writeln!(lock, "Got block: {}", block.as_u64())?;
            process_block(
                &mut lock,
                block.as_u64(),
                &provider,
                &processor,
                &mut db,
                &prices,
            )
            .await?;
        }
    }

    Ok(())
}

async fn process_block<M: Middleware + 'static>(
    lock: &mut std::io::StdoutLock<'_>,
    block_number: u64,
    provider: &M,
    processor: &BatchInspector,
    db: &mut MevDB<'_>,
    prices: &HistoricalPrice<M>,
) -> anyhow::Result<()> {
    let block_number = block_number.into();

    // get all the traces
    let traces = provider
        .trace_block(BlockNumber::Number(block_number))
        .await?;
    // get all the block txs
    let block = provider
        .get_block_with_txs(block_number)
        .await?
        .expect("block should exist");
    let gas_price_txs = block
        .transactions
        .iter()
        .map(|tx| (tx.hash, tx.gas_price))
        .collect::<HashMap<TxHash, U256>>();

    // get all the receipts
    let receipts = provider.parity_block_receipts(block_number).await?;
    let gas_used_txs = receipts
        .into_iter()
        .map(|receipt| {
            (
                receipt.transaction_hash,
                receipt.gas_used.unwrap_or_default(),
            )
        })
        .collect::<HashMap<TxHash, U256>>();

    let inspections = processor.inspect_many(traces);

    let t1 = std::time::Instant::now();

    let eval_futs = inspections.into_iter().map(|inspection| {
        let gas_used = gas_used_txs
            .get(&inspection.hash)
            .cloned()
            .unwrap_or_default();
        let gas_price = gas_price_txs
            .get(&inspection.hash)
            .cloned()
            .unwrap_or_default();
        Evaluation::new(inspection, &prices, gas_used, gas_price)
    });
    for evaluation in futures::future::join_all(eval_futs).await {
        if let Ok(evaluation) = evaluation {
            db.insert(&evaluation).await?;
        }
    }

    writeln!(
        lock,
        "Processed {:?} in {:?}",
        block_number,
        std::time::Instant::now().duration_since(t1)
    )?;
    Ok(())
}
