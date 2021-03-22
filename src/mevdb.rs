use crate::inspectors::BatchEvaluationError;
use crate::types::Evaluation;
use ethers::prelude::Middleware;
use ethers::types::{TxHash, U256};
use futures::{Future, FutureExt, Stream, StreamExt};
use rust_decimal::prelude::*;
use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};
use thiserror::Error;
use tokio_postgres::{config::Config, Client, NoTls};

/// Wrapper around PostGres for storing results in the database
pub struct MevDB {
    client: Client,
    table_name: String,
    overwrite: String,
}

impl MevDB {
    /// Connects to the MEV PostGres instance
    pub async fn connect(cfg: Config, table_name: impl Into<String>) -> Result<Self, DbError> {
        let (client, connection) = cfg.connect(NoTls).await?;

        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("connection error: {}", e);
            }
        });

        // TODO: Allow overwriting on conflict
        let overwrite = "on conflict do nothing";
        Ok(Self {
            client,
            table_name: table_name.into(),
            overwrite: overwrite.to_owned(),
        })
    }

    /// Creates a new table for the MEV data
    pub async fn create(&mut self) -> Result<(), DbError> {
        self.client
            .batch_execute(&format!(
                "CREATE TABLE IF NOT EXISTS {} (
                    hash text PRIMARY KEY,
                    status text,

                    block_number NUMERIC,
                    gas_price NUMERIC,
                    gas_used NUMERIC,
                    revenue NUMERIC,

                    protocols text[],
                    actions text[],

                    eoa text,
                    contract text,
                    proxy_impl text,

                    inserted_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
                )",
                self.table_name
            ))
            .await?;
        Ok(())
    }

    /// Inserts data from this evaluation to PostGres
    pub async fn insert(&mut self, evaluation: &Evaluation) -> Result<(), DbError> {
        self.client
            .execute(
                format!(
                    "INSERT INTO {} (
                        hash,
                        status,
                        block_number,
                        gas_price,
                        gas_used,
                        revenue,
                        protocols,
                        actions,
                        eoa,
                        contract,
                        proxy_impl
                    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                    {}",
                    self.table_name, self.overwrite,
                )
                .as_str(),
                &[
                    &format!("{:?}", evaluation.inspection.hash),
                    &format!("{:?}", evaluation.inspection.status),
                    &Decimal::from(evaluation.inspection.block_number),
                    &u256_decimal(evaluation.gas_price)?,
                    &u256_decimal(evaluation.gas_used)?,
                    &u256_decimal(evaluation.profit)?,
                    &vec_str(&evaluation.inspection.protocols),
                    &vec_str(&evaluation.actions),
                    &format!("{:?}", evaluation.inspection.from),
                    &format!("{:?}", evaluation.inspection.contract),
                    &evaluation
                        .inspection
                        .proxy_impl
                        .map(|x| format!("{:?}", x))
                        .unwrap_or_else(|| "".to_owned()),
                ],
            )
            .await?;

        Ok(())
    }

    /// Checks if the transaction hash is already inspected
    pub async fn exists(&mut self, hash: TxHash) -> Result<bool, DbError> {
        let rows = self
            .client
            .query(
                format!("SELECT hash FROM {} WHERE hash = $1", self.table_name).as_str(),
                &[&format!("{:?}", hash)],
            )
            .await?;
        if let Some(row) = rows.get(0) {
            let got: String = row.get(0);
            Ok(format!("{:?}", hash) == got)
        } else {
            Ok(false)
        }
    }

    /// Checks if the provided block has been inspected
    pub async fn block_exists(&mut self, block: u64) -> Result<bool, DbError> {
        let rows = self
            .client
            .query(
                format!(
                    "SELECT block_number FROM {} WHERE block_number = $1 LIMIT 1;",
                    self.table_name
                )
                .as_str(),
                &[&Decimal::from_u64(block).ok_or(DbError::InvalidDecimal)?],
            )
            .await?;
        if rows.get(0).is_some() {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn clear(&mut self) -> Result<(), DbError> {
        self.client
            .batch_execute(&format!("DROP TABLE {}", self.table_name))
            .await?;
        Ok(())
    }
}

#[derive(Error, Debug)]
pub enum DbError {
    #[error(transparent)]
    Decimal(#[from] rust_decimal::Error),

    #[error("could not convert u64 to decimal")]
    InvalidDecimal,

    #[error(transparent)]
    TokioPostGres(#[from] tokio_postgres::Error),
}

type EvalInsertion = Pin<Box<dyn Future<Output = Result<(Evaluation, MevDB), (MevDB, DbError)>>>>;

type EvaluationStream<'a, M> =
    Pin<Box<dyn Stream<Item = Result<Evaluation, BatchEvaluationError<M>>> + 'a>>;

/// Takes a stream of `Evaluation`s and puts it in the database
pub struct BatchInserts<'a, M: Middleware + Unpin + 'static> {
    mev_db: Option<MevDB>,
    /// The currently running insert job
    insertion: Option<EvalInsertion>,
    /// `Evaluation`s ready to insert
    insertion_queue: VecDeque<Evaluation>,
    /// All the evaluations to insert
    pending_evaluations: EvaluationStream<'a, M>,
    /// Whether no more evaluations are coming
    evals_done: bool,
}

impl<'a, M: Middleware + Unpin + 'static> BatchInserts<'a, M> {
    pub fn new<S>(mev_db: MevDB, evals: S) -> Self
    where
        S: Stream<Item = Result<Evaluation, BatchEvaluationError<M>>> + 'a,
    {
        Self {
            mev_db: Some(mev_db),
            insertion: None,
            insertion_queue: VecDeque::new(),
            pending_evaluations: Box::pin(evals),
            evals_done: false,
        }
    }

    /// Returns the database again
    ///
    /// If the DB is currently busy, this waits until the last job is completed
    pub async fn get_database(mut self) -> MevDB {
        if let Some(db) = self.mev_db.take() {
            db
        } else {
            match self.insertion.expect("DB is busy when not idle").await {
                Ok((_, db)) => db,
                Err((db, _)) => db,
            }
        }
    }
}

impl<'a, M: Middleware + Unpin> Stream for BatchInserts<'a, M> {
    type Item = Result<Evaluation, InsertEvaluationError<M>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        // start a new insert if ready
        if let Some(db) = this.mev_db.take() {
            if let Some(next) = this.insertion_queue.pop_front() {
                log::trace!(
                    "start next evaluation insert, {} pending",
                    this.insertion_queue.len()
                );
                this.insertion = Some(Box::pin(insert_evaluation(next, db)));
            } else {
                this.mev_db = Some(db);
            }
        }

        // complete the insertion task
        if let Some(mut job) = this.insertion.take() {
            match job.poll_unpin(cx) {
                Poll::Ready(Ok((eval, db))) => {
                    this.mev_db = Some(db);
                    return Poll::Ready(Some(Ok(eval)));
                }
                Poll::Ready(Err((db, err))) => {
                    this.mev_db = Some(db);
                    return Poll::Ready(Some(Err(err.into())));
                }
                Poll::Pending => {
                    this.insertion = Some(job);
                }
            }
        }

        if !this.evals_done {
            // queue in all evaluations that are coming in
            loop {
                match this.pending_evaluations.poll_next_unpin(cx) {
                    Poll::Ready(Some(Ok(eval))) => {
                        log::trace!(
                            "received new evaluation of block {} with tx {}; waiting evaluations: {}",
                            eval.inspection.block_number,
                            eval.inspection.hash,
                            this.insertion_queue.len() + 1
                        );
                        this.insertion_queue.push_back(eval);
                    }
                    Poll::Ready(Some(Err(err))) => return Poll::Ready(Some(Err(err.into()))),
                    Poll::Ready(None) => {
                        log::trace!("evaluations done");
                        this.evals_done = true;
                        break;
                    }
                    Poll::Pending => break,
                }
            }
        }

        // If more evaluations and insertions are processed we're not done yet
        if this.evals_done && this.insertion_queue.is_empty() && this.insertion.is_none() {
            log::trace!("batch insert done");
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let insertions = self.insertion_queue.len() + self.insertion.is_some() as usize;
        let (evals, _) = self.pending_evaluations.size_hint();
        (insertions + evals, None)
    }
}

async fn insert_evaluation(
    eval: Evaluation,
    mut db: MevDB,
) -> Result<(Evaluation, MevDB), (MevDB, DbError)> {
    if let Err(err) = db.insert(&eval).await {
        log::error!("DB insert failed: {:?}", err);
        Err((db, err))
    } else {
        log::debug!(
            "inserted evaluation of block {} with tx {}",
            eval.inspection.block_number,
            eval.inspection.hash
        );
        Ok((eval, db))
    }
}

#[derive(Error, Debug)]
pub enum InsertEvaluationError<M: Middleware + 'static> {
    #[error(transparent)]
    DbError(#[from] DbError),

    #[error(transparent)]
    BatchEvaluationError(#[from] BatchEvaluationError<M>),
}

// helpers
fn vec_str<T: std::fmt::Debug, I: IntoIterator<Item = T>>(t: I) -> Vec<String> {
    t.into_iter()
        .map(|i| format!("{:?}", i).to_lowercase())
        .collect::<Vec<_>>()
}

fn u256_decimal(src: U256) -> Result<Decimal, rust_decimal::Error> {
    Decimal::from_str(&src.to_string())
}

#[cfg(all(test, feature = "postgres-tests"))]
mod tests {
    use super::*;
    use crate::types::evaluation::ActionType;
    use crate::types::Inspection;
    use ethers::types::{Address, TxHash};
    use std::collections::HashSet;

    /// This expects postgres running on localhost:5432 with user `mev_rs_user` and table `mev_inspections_test`
    #[tokio::test]
    async fn insert_eval() {
        let mut config = Config::default();
        config
            .host("localhost")
            .user("mev_rs_user")
            .dbname("mev_inspections_test");
        let mut client = MevDB::connect(config, "mev_inspections").await.unwrap();
        let _ = client.clear().await;
        client.create().await.unwrap();

        let inspection = Inspection {
            status: crate::types::Status::Checked,
            actions: Vec::new(),
            protocols: HashSet::new(),
            from: Address::zero(),
            contract: Address::zero(),
            proxy_impl: None,
            hash: TxHash::zero(),
            block_number: 9,
        };
        let actions = [ActionType::Liquidation, ActionType::Arbitrage]
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let evaluation = Evaluation {
            inspection,
            gas_used: 21000.into(),
            gas_price: (100e9 as u64).into(),
            actions,
            profit: (1e18 as u64).into(),
        };

        client.insert(&evaluation).await.unwrap();

        assert!(client.exists(evaluation.as_ref().hash).await.unwrap());

        // conflicts get ignored
        client.insert(&evaluation).await.unwrap();
    }
}
