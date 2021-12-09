use mev_inspect::{
    inspectors::{
        gas_price_txs_from_block, Aave, Balancer, Compound, Curve, Uniswap, ZeroEx, ERC20,
    },
    reducers::{ArbitrageReducer, LiquidationReducer, TradeReducer},
    types::Evaluation,
    BatchInserts, BatchInspector, CachedProvider, HistoricalPrice, Inspector, MevDB, Reducer,
};

use ethers::{
    providers::{Middleware, Provider, StreamExt},
    types::{BlockNumber, TxHash, U256},
};

use futures::SinkExt;
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
    #[options(default = "4", help = "How many separate tasks to use")]
    tasks: u64,
    #[options(
        default = "10",
        help = "Maximum of requests each task is allowed to execute concurrently"
    )]
    max_requests: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::init();
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
    let inspectors: Vec<Box<dyn Inspector + Send + Sync>> = vec![
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

    let reducers: Vec<Box<dyn Reducer + Send + Sync>> = vec![
        Box::new(LiquidationReducer::new()),
        Box::new(TradeReducer::new()),
        Box::new(ArbitrageReducer::new()),
    ];
    let processor = BatchInspector::new(inspectors, reducers);

    // TODO: Pass overwrite parameter
    let mut db = MevDB::connect(opts.db_cfg, &opts.db_table).await?;
    db.create().await?;
    if opts.reset {
        db.clear().await?;
        db.create().await?;
    }
    log::debug!("created mevdb table");

    if let Some(cmd) = opts.cmd {
        match cmd {
            Command::Tx(opts) => {
                let traces = provider.trace_transaction(opts.tx).await?;
                if let Some(inspection) = processor.inspect_one(traces) {
                    let transaction_receipt = provider
                        .get_transaction_receipt(inspection.hash)
                        .await?
                        .expect("tx not found");

                    let gas_used = transaction_receipt.gas_used.unwrap_or_default();

                    let legacy_gas_price = provider
                        .get_transaction(inspection.hash)
                        .await?
                        .expect("tx not found")
                        .gas_price;

                    let effective_gas_price = transaction_receipt.effective_gas_price;

                    let evaluation = match (legacy_gas_price, effective_gas_price) {
                        (Some(gas_price), _) => {
                            Some(Evaluation::new(inspection, &prices, gas_used, gas_price).await?)
                        }
                        (None, Some(gas_price)) => {
                            Some(Evaluation::new(inspection, &prices, gas_used, gas_price).await?)
                        }
                        _ => None,
                    }
                    .unwrap();
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
                log::debug!("command blocks {:?}", inner);
                let provider = Arc::new(provider);
                let processor = Arc::new(processor);
                let prices = Arc::new(prices);

                let (tx, rx) = futures::channel::mpsc::unbounded();

                // divide the bloccs to process equally onto all the tasks
                assert!(inner.from < inner.to);
                let mut num_tasks = inner.tasks as usize;
                let block_num = inner.to - inner.from;
                let blocks_per_task = block_num.max(inner.tasks) / inner.tasks;
                let rem = block_num % inner.tasks;

                if rem > 0 {
                    let rem_start = inner.to - rem - blocks_per_task;
                    let processor = Arc::clone(&processor);
                    let eval_stream = processor.evaluate_blocks(
                        Arc::clone(&provider),
                        Arc::clone(&prices),
                        rem_start..inner.to,
                        inner.max_requests,
                    );
                    let mut tx = tx.clone();
                    log::debug!("spawning batch for blocks: [{}..{})", rem_start, inner.to);
                    tokio::task::spawn(async move {
                        // wrap in an ok because send_all only sends Result::Ok
                        let mut iter = eval_stream.map(Ok);
                        let _ = tx.send_all(&mut iter).await;
                    });

                    num_tasks -= 1;
                };

                for from in (inner.from..inner.to)
                    .into_iter()
                    .step_by(blocks_per_task as usize)
                    .take(num_tasks as usize)
                {
                    let processor = Arc::clone(&processor);
                    let eval_stream = processor.evaluate_blocks(
                        Arc::clone(&provider),
                        Arc::clone(&prices),
                        from..from + blocks_per_task,
                        inner.max_requests,
                    );
                    let mut tx = tx.clone();
                    log::debug!(
                        "spawning batch for blocks: [{}..{})",
                        from,
                        from + blocks_per_task
                    );
                    tokio::task::spawn(async move {
                        // wrap in an ok because send_all only sends Result::Ok
                        let mut iter = eval_stream.map(Ok);
                        let _ = tx.send_all(&mut iter).await;
                    });
                }
                // drop the sender so that the channel gets closed
                drop(tx);

                // all the evaluations arrive at the receiver and are inserted into the DB
                let mut inserts = BatchInserts::new(db, rx);
                let mut insert_ctn = 0usize;
                let mut error_ctn = 0usize;
                while let Some(res) = inserts.next().await {
                    match res {
                        Ok(eval) => {
                            insert_ctn += 1;
                            log::info!(
                                "Inserted tx 0x{} in block {}",
                                eval.inspection.hash,
                                eval.inspection.block_number,
                            );
                        }
                        Err(err) => {
                            error_ctn += 1;
                            log::error!("failed to insert: {:?}", err)
                        }
                    }
                }
                println!(
                    "inserted evaluations: {}, errors: {}, block range [{}..{}) using {} tasks",
                    insert_ctn, error_ctn, inner.from, inner.to, inner.tasks
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
    db: &mut MevDB,
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

    let receipts = provider.get_block_receipts(block_number).await?;

    let gas_price_txs = gas_price_txs_from_block(&block, receipts);

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
