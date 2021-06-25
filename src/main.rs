use mev_inspect::{
    inspectors::{Aave, Balancer, Compound, Curve, Uniswap, ZeroEx, ERC20},
    model::EventLog,
    reducers::{ArbitrageReducer, LiquidationReducer, TradeReducer},
    types::Evaluation,
    BatchInserts, BatchInspector, CachedProvider, DefiProtocol, HistoricalPrice, MevDB, TxReducer,
};

use ethers::{
    providers::{Middleware, Provider, StreamExt},
    types::TxHash,
};

use ethers::types::Filter;
use futures::SinkExt;
use gumdrop::Options;
use mev_inspect::types::TransactionData;
use std::{convert::TryFrom, path::PathBuf, sync::Arc};

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
    let inspectors: Vec<Box<dyn DefiProtocol + Send + Sync>> = vec![
        Box::new(ZeroEx::default()),
        // Classify AMMs
        Box::new(Balancer::default()),
        Box::new(Uniswap::default()),
        Box::new(curve),
        // Classify Liquidations
        Box::new(Aave::new()),
        Box::new(compound),
        // Classify Transfers
        Box::new(ERC20::new()),
    ];

    let reducers: Vec<Box<dyn TxReducer + Send + Sync>> = vec![
        Box::new(LiquidationReducer),
        Box::new(TradeReducer),
        Box::new(ArbitrageReducer),
    ];
    let processor = BatchInspector::new(inspectors, reducers);

    // TODO: Pass overwrite parameter
    let mut db = MevDB::connect(opts.db_cfg)
        .await?
        .with_table_name(&opts.db_table);

    if opts.reset {
        db.redo_migration().await?
    } else {
        db.run_migration().await?;
    }
    log::debug!("created mevdb table");

    db.prepare_statements().await?;
    log::debug!("prepared mevdb statements");

    if let Some(cmd) = opts.cmd {
        match cmd {
            Command::Tx(opts) => {
                let traces = provider.trace_transaction(opts.tx).await?;
                if traces.is_empty() {
                    return Ok(());
                }

                let block = traces[0].block_number;
                let logs: Vec<_> = provider
                    .get_logs(&Filter::new().from_block(block).to_block(block))
                    .await?
                    .into_iter()
                    .filter(|log| log.transaction_hash == Some(opts.tx))
                    .filter_map(|log| EventLog::try_from(log).ok())
                    .collect();

                let mut tx = TransactionData::create(traces, logs)
                    .unwrap_or_else(|_| panic!("Failed to create tx {:?}", opts.tx));

                processor.inspect_tx(&mut tx);
                processor.reduce_tx(&mut tx);
                let gas_used = provider
                    .get_transaction_receipt(tx.hash)
                    .await?
                    .expect("tx not found")
                    .gas_used
                    .unwrap_or_default();
                let gas_price = provider
                    .get_transaction(tx.hash)
                    .await?
                    .expect("tx not found")
                    .gas_price;

                let evaluation = Evaluation::new(tx, &prices, gas_used, gas_price).await?;
                println!("Found: {:?}", evaluation.as_ref().hash);
                println!("Revenue: {:?} WEI", evaluation.profit);
                println!("Cost: {:?} WEI", evaluation.gas_used * evaluation.gas_price);
                println!("Actions: {:?}", evaluation.actions);
                println!("Protocols: {:?}", evaluation.tx.protocols());
                println!("Status: {:?}", evaluation.tx.status);
                db.insert(&evaluation).await?;
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
                                eval.tx.hash,
                                eval.tx.block_number,
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
        let provider = Arc::new(provider);
        let processor = Arc::new(processor);
        let prices = Arc::new(prices);

        let mut watcher = provider.watch_blocks().await?;

        while watcher.next().await.is_some() {
            let block = provider.get_block_number().await?.as_u64();
            println!("Got block: {}", block);
            let processor = Arc::clone(&processor);
            let mut eval_stream = processor.evaluate_blocks(
                Arc::clone(&provider),
                Arc::clone(&prices),
                block..block + 1,
                10,
            );
            while let Some(res) = eval_stream.next().await {
                match res {
                    Ok(eval) => {
                        if let Err(err) = db.insert(&eval).await {
                            log::error!("failed to insert: {:?}", err)
                        } else {
                            log::info!(
                                "Inserted tx 0x{} in block {}",
                                eval.tx.hash,
                                eval.tx.block_number,
                            );
                        }
                    }
                    Err(err) => {
                        log::error!("failed to insert: {:?}", err)
                    }
                }
            }
        }
    }

    Ok(())
}
