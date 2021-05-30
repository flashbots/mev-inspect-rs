use std::collections::{BTreeMap, VecDeque};
use std::pin::Pin;
use std::task::{Context, Poll};

use ethers::prelude::Middleware;
use ethers::types::{Address, TxHash, U256};
use futures::{future, Future, FutureExt, Stream, StreamExt};
use rust_decimal::prelude::*;
use thiserror::Error;
use tokio_postgres::{config::Config, Client, NoTls, Statement};

use crate::inspectors::BatchEvaluationError;
use crate::model::{EventLog, InternalCall, SqlCallType, SqlRowExt};
use crate::types::evaluation::ActionType;
use crate::types::{Evaluation, Protocol};
use itertools::Itertools;

/// The SQL script to setup the database schema
pub const DATABASE_MIGRATION_UP: &str =
    include_str!("../migrations/00000000000000_initial_setup/up.sql");

/// The SQL script to drop all the database infra
pub const DATABASE_MIGRATION_DOWN: &str =
    include_str!("../migrations/00000000000000_initial_setup/down.sql");

// default table name for inspections
const DEFAULT_MEV_INSPECTIONS_TABLE: &'static str = "mev_inspections";

// default table name for internal calls
const DEFAULT_INTERNAL_CALLS_TABLE: &'static str = "internal_calls";

// default table name for event logs
const DEFAULT_LOGS_TABLE: &'static str = "event_logs";

/// Wrapper around PostGres for storing results in the database
pub struct MevDB {
    client: Client,
    on_conflict: String,
    table_name: String,
    /// prepared statements for inserting entries
    prepared_statements: Option<PreparedInsertStatements>,
    /// What to insert
    insert_filter: InsertFilter,
}

struct PreparedInsertStatements {
    /// The prepared statement to insert an `Evaluation`
    insert_evaluation_stmt: Statement,
    /// The prepared statement to insert an `InternalCall`
    insert_call_stmt: Statement,
    /// The prepared statement to insert an `EventLog`
    insert_event_log_stmt: Statement,
}

impl MevDB {
    /// Connects to the MEV PostGres instance
    pub async fn connect(cfg: Config) -> Result<Self, DbError> {
        let (client, connection) = cfg.connect(NoTls).await?;

        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("connection error: {}", e);
            }
        });

        Ok(Self {
            client,
            // TODO: Allow overwriting on conflict
            table_name: DEFAULT_MEV_INSPECTIONS_TABLE.to_string(),
            on_conflict: "on conflict do nothing".to_string(),
            prepared_statements: None,
            insert_filter: Default::default(),
        })
    }

    /// Prepares all the statement that can be reused when inserting new rows
    pub async fn prepare_statements(&mut self) -> Result<(), DbError> {
        self.prepared_statements = Some(self.get_prepared_stmts().await?);
        Ok(())
    }

    /// Sets the `InsertFilter` to apply when inserting `Evaluation`s
    pub fn with_insert_filter(mut self, filter: InsertFilter) -> Self {
        self.insert_filter = filter;
        self
    }
    /// Sets the `InsertFilter` to apply when inserting `Evaluation`s
    pub fn with_table_name(mut self, table_name: impl Into<String>) -> Self {
        self.table_name = table_name.into();
        self
    }

    /// Runs the database migration
    pub async fn run_migration(&self) -> Result<(), DbError> {
        if self.table_name == DEFAULT_MEV_INSPECTIONS_TABLE {
            Ok(self.client.batch_execute(DATABASE_MIGRATION_UP).await?)
        } else {
            Ok(self
                .client
                .batch_execute(
                    &DATABASE_MIGRATION_UP.replace(DEFAULT_MEV_INSPECTIONS_TABLE, &self.table_name),
                )
                .await?)
        }
    }

    /// Reverts the database migration
    pub async fn revert_migration(&self) -> Result<(), DbError> {
        if self.table_name == DEFAULT_MEV_INSPECTIONS_TABLE {
            Ok(self.client.batch_execute(DATABASE_MIGRATION_DOWN).await?)
        } else {
            Ok(self
                .client
                .batch_execute(
                    &DATABASE_MIGRATION_DOWN
                        .replace(DEFAULT_MEV_INSPECTIONS_TABLE, &self.table_name),
                )
                .await?)
        }
    }

    /// First runs the down.sql script and then up.sql
    pub async fn redo_migration(&self) -> Result<(), DbError> {
        self.revert_migration().await?;
        self.run_migration().await
    }

    /// The statement to insert `Evaluation`s
    fn insert_into_table_name_stmt(&self) -> String {
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
                        proxy_impl,
                        transaction_position
                    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
                    {}",
            self.table_name, self.on_conflict,
        )
    }

    /// The statement to insert `InternalCall`s
    fn insert_into_internal_call_stmt(&self) -> String {
        format!(
            "INSERT INTO internal_calls (
                        transaction_hash,
                        trace_address,
                        call_type,
                        value,
                        gas_used,
                        caller,
                        callee,
                        protocol,
                        input,
                        classification
                    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                    {}",
            self.on_conflict,
        )
    }

    /// The statement to insert `EventLog`s
    fn insert_into_event_logs_stmt(&self) -> String {
        format!(
            "INSERT INTO event_logs (
                        address,
                        transaction_hash,
                        signature,
                        topics,
                        data,
                        transaction_index,
                        log_index,
                        block_number
                    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                    {}",
            self.on_conflict,
        )
    }

    async fn get_prepared_call_stmt(&self) -> Result<Statement, DbError> {
        let insert_call = self.insert_into_internal_call_stmt();
        Ok(self.client.prepare(&insert_call).await?)
    }

    async fn get_prepared_log_stmt(&self) -> Result<Statement, DbError> {
        let insert_log = self.insert_into_event_logs_stmt();
        Ok(self.client.prepare(&insert_log).await?)
    }
    async fn get_prepared_eval_stmt(&self) -> Result<Statement, DbError> {
        let insert_eval = self.insert_into_table_name_stmt();
        Ok(self.client.prepare(&insert_eval).await?)
    }

    async fn get_prepared_stmts(&self) -> Result<PreparedInsertStatements, DbError> {
        let (insert_evaluation_stmt, insert_call_stmt, insert_event_log_stmt) = futures::try_join!(
            self.get_prepared_eval_stmt(),
            self.get_prepared_call_stmt(),
            self.get_prepared_log_stmt()
        )?;
        Ok(PreparedInsertStatements {
            insert_evaluation_stmt,
            insert_call_stmt,
            insert_event_log_stmt,
        })
    }

    /// Creates a new table for the MEV data
    pub async fn create(&self) -> Result<(), DbError> {
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

                    transaction_position NUMERIC,

                    inserted_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
                )",
                self.table_name
            ))
            .await?;
        Ok(())
    }

    /// Returns all database `Evaluation` entries where the `eoa` column matches the address
    pub async fn select_where_eoa(&self, address: Address) -> Result<Vec<Evaluation>, DbError> {
        self.select_where(&format!("eoa = '{:?}'", address)).await
    }

    /// Returns all database `Evaluation` entries where the `protocol` column contains one of
    /// the provided protocols
    pub async fn select_where_protocols(
        &self,
        protocols: &[Protocol],
    ) -> Result<Vec<Evaluation>, DbError> {
        let values = protocols
            .iter()
            .map(|p| format!("'{}'", p.to_string()))
            .collect::<Vec<_>>()
            .join(",");
        self.select_where(&format!("ARRAY[{}]::text[] <@ protocols", values))
            .await
    }

    /// Returns all database `Evaluation` entries where the `actions` column contains one of
    /// the provided action
    pub async fn select_where_actions(
        &self,
        actions: &[ActionType],
    ) -> Result<Vec<Evaluation>, DbError> {
        let values = actions
            .iter()
            .map(|p| format!("'{}'", p.to_string()))
            .collect::<Vec<_>>()
            .join(",");
        self.select_where(&format!("ARRAY[{}]::text[] <@ actions", values))
            .await
    }

    /// Returns all `Evaluations` group by their block number
    pub async fn select_blocks(
        &self,
        blocks: impl IntoIterator<Item = u64>,
    ) -> Result<BTreeMap<u64, Vec<Evaluation>>, DbError> {
        let blocks = blocks
            .into_iter()
            .map(|block| block.to_string())
            .collect::<Vec<_>>();
        let clause = format!("block_number in ({})", blocks.join(","));
        Ok(self
            .select_where(&clause)
            .await?
            .into_iter()
            .group_by(|eval| eval.tx.block_number)
            .into_iter()
            .map(|(num, evals)| {
                let mut evals = evals.collect::<Vec<_>>();
                evals.sort_by_key(|eval| eval.tx.block_number);
                (num, evals)
            })
            .collect())
    }

    /// Returns the latest block number stored in the database
    pub async fn latest_block(&self) -> Result<u64, DbError> {
        Ok(self
            .client
            .query_one(
                format!("SELECT MAX(block_number) FROM {}", self.table_name).as_str(),
                &[],
            )
            .await?
            .get::<_, Decimal>(0)
            .to_u64()
            .expect("block number stored as u64; qed"))
    }

    /// Returns the earliest block number stored in the database
    pub async fn earliest_block(&self) -> Result<u64, DbError> {
        Ok(self
            .client
            .query_one(
                format!("SELECT MIN(block_number) FROM {}", self.table_name).as_str(),
                &[],
            )
            .await?
            .get::<_, Decimal>(0)
            .to_u64()
            .expect("block number stored as u64; qed"))
    }

    /// Returns all database `Evaluation` entries where the `block_number` column matches the block_number
    pub async fn select_where_block(&self, block_number: u64) -> Result<Vec<Evaluation>, DbError> {
        self.select_where(&format!("block_number = {}", block_number))
            .await
    }

    /// Returns all database `Evaluation` entries where the `block_number` is [lower..upper]
    pub async fn select_where_block_in_range(
        &self,
        lower: u64,
        upper: u64,
    ) -> Result<Vec<Evaluation>, DbError> {
        self.select_where(&format!(
            "block_number >= {} AND block_number <= {}",
            lower, upper
        ))
        .await
    }

    /// Returns the `Evaluation` entry with the transaction `hash` primary key.
    pub async fn select_transaction(&self, tx: TxHash) -> Result<Evaluation, DbError> {
        let row = self
            .client
            .query_one(
                format!("SELECT * FROM {} WHERE hash = '{:?}'", self.table_name, tx).as_str(),
                &[],
            )
            .await?;
        Evaluation::from_row(&row)
    }

    /// Returns all database `Evaluation` entries where the `proxy_impl` column matches the address
    pub async fn select_where_proxy(&self, address: Address) -> Result<Vec<Evaluation>, DbError> {
        self.select_where(&format!("proxy_impl = '{:?}'", address))
            .await
    }

    /// Returns all database `Evaluation` entries where the `contract` column matches the address
    pub async fn select_where_contract(
        &self,
        address: Address,
    ) -> Result<Vec<Evaluation>, DbError> {
        self.select_where(&format!("contract = '{:?}'", address))
            .await
    }

    /// Returns all internal calls within a transaction
    pub async fn select_internal_calls_in_tx(
        &self,
        tx: TxHash,
    ) -> Result<Vec<InternalCall>, DbError> {
        self.select_internal_calls_where(&format!("transaction_hash = '{:?}'", tx))
            .await
    }

    /// Returns all internal calls within a transaction
    pub async fn select_logs_in_tx(&self, tx: TxHash) -> Result<Vec<EventLog>, DbError> {
        self.select_logs_where(&format!("transaction_hash = '{:?}'", tx))
            .await
    }

    /// Expects the `WHERE` clause as input: `eoa = '0x2363423..'`
    ///
    /// *NOTE*: this returns only a bare `Evaluation` _without_ inner `InternalCall`s and `EventLog`s
    pub async fn select_where(&self, stmt: &str) -> Result<Vec<Evaluation>, DbError> {
        self.client
            .query(
                format!(
                    "SELECT * FROM {} WHERE {}",
                    self.table_name,
                    stmt.trim_start_matches("WHERE ")
                )
                .as_str(),
                &[],
            )
            .await?
            .iter()
            .map(SqlRowExt::from_row)
            .collect()
    }

    /// Expects the `WHERE` clause as input: `hash = '0x2363423..'`
    pub async fn select_internal_calls_where(
        &self,
        stmt: &str,
    ) -> Result<Vec<InternalCall>, DbError> {
        let mut calls = self
            .query::<InternalCall>(
                format!(
                    "SELECT * FROM {} WHERE {}",
                    DEFAULT_INTERNAL_CALLS_TABLE,
                    stmt.trim_start_matches("WHERE ")
                )
                .as_str(),
            )
            .await?;
        calls.sort();
        Ok(calls)
    }

    /// Expects the `WHERE` clause as input: `hash = '0x2363423..'`
    pub async fn select_logs_where(&self, stmt: &str) -> Result<Vec<EventLog>, DbError> {
        self.query(
            format!(
                "SELECT * FROM {} WHERE {} ORDER BY log_index ASC",
                DEFAULT_LOGS_TABLE,
                stmt.trim_start_matches("WHERE ")
            )
            .as_str(),
        )
        .await
    }

    async fn query<T: SqlRowExt>(&self, stmt: &str) -> Result<Vec<T>, DbError> {
        self.client
            .query(stmt, &[])
            .await?
            .iter()
            .map(SqlRowExt::from_row)
            .collect()
    }

    /// Insert a single `InternalCall`
    pub async fn insert_call(&self, call: &InternalCall) -> Result<(), DbError> {
        if let Some(ref stmts) = self.prepared_statements {
            Ok(self
                .insert_call_with_statement(&stmts.insert_call_stmt, call)
                .await?)
        } else {
            let stmt = self.get_prepared_call_stmt().await?;
            Ok(self.insert_call_with_statement(&stmt, call).await?)
        }
    }

    async fn insert_call_with_statement(
        &self,
        stmt: &Statement,
        call: &InternalCall,
    ) -> Result<(), DbError> {
        let call_type: SqlCallType = call.call_type.clone().into();
        self.client
            .execute(
                stmt,
                &[
                    &format!("{:?}", call.transaction_hash),
                    &call
                        .trace_address
                        .iter()
                        .cloned()
                        .map(Decimal::from)
                        .collect::<Vec<_>>(),
                    &call_type,
                    // &call_type_to_str(&call.call_type),
                    &u256_decimal(call.value)?,
                    &u256_decimal(call.gas_used)?,
                    &format!("{:?}", call.from),
                    &format!("{:?}", call.to),
                    &call
                        .protocol
                        .as_ref()
                        .map(|proto| proto.to_string())
                        .unwrap_or_default(),
                    &call.input,
                    &call.classification,
                ],
            )
            .await?;
        Ok(())
    }

    /// Insert a single `EventLog`
    pub async fn insert_log(&self, log: &EventLog) -> Result<(), DbError> {
        if let Some(ref stmts) = self.prepared_statements {
            Ok(self
                .insert_log_with_statement(&stmts.insert_event_log_stmt, log)
                .await?)
        } else {
            let stmt = self.get_prepared_log_stmt().await?;
            Ok(self.insert_log_with_statement(&stmt, log).await?)
        }
    }

    async fn insert_log_with_statement(
        &self,
        stmt: &Statement,
        log: &EventLog,
    ) -> Result<(), DbError> {
        self.client
            .execute(
                stmt,
                &[
                    &format!("{:?}", log.address),
                    &format!("{:?}", log.transaction_hash),
                    &format!("{:?}", log.signature),
                    &vec_str(&log.raw_log.topics),
                    &log.raw_log.data,
                    &Decimal::from(log.transaction_index),
                    &u256_decimal(log.log_index)?,
                    &Decimal::from(log.block_number),
                ],
            )
            .await?;
        Ok(())
    }

    async fn insert_with_statements(
        &self,
        evaluation: &Evaluation,
        stmts: &PreparedInsertStatements,
    ) -> Result<(), DbError> {
        let PreparedInsertStatements {
            insert_evaluation_stmt,
            insert_call_stmt,
            insert_event_log_stmt,
        } = stmts;

        self.client
            .execute(
                insert_evaluation_stmt,
                &[
                    &format!("{:?}", evaluation.tx.hash),
                    &format!("{:?}", evaluation.tx.status),
                    &Decimal::from(evaluation.tx.block_number),
                    &u256_decimal(evaluation.gas_price)?,
                    &u256_decimal(evaluation.gas_used)?,
                    &u256_decimal(evaluation.profit)?,
                    &vec_str(&evaluation.tx.protocols()),
                    &vec_str(&evaluation.actions),
                    &format!("{:?}", evaluation.tx.from),
                    &format!("{:?}", evaluation.tx.contract),
                    &evaluation
                        .tx
                        .proxy_impl
                        .map(|x| format!("{:?}", x))
                        .unwrap_or_else(|| "".to_owned()),
                    &Decimal::from(evaluation.tx.transaction_position),
                ],
            )
            .await?;

        let (calls_fut, logs_fut) =
            match self.insert_filter {
                InsertFilter::EvaluationOnly => return Ok(()),
                InsertFilter::Essential => {
                    // insert only calls and logs used during classification
                    let calls_fut = future::try_join_all(
                        evaluation
                            .tx
                            .assigned_calls()
                            .map(|call| self.insert_call_with_statement(insert_call_stmt, call)),
                    );

                    let logs_fut =
                        future::try_join_all(evaluation.tx.assigned_logs().map(|(_, log)| {
                            self.insert_log_with_statement(insert_event_log_stmt, log)
                        }));

                    (calls_fut, logs_fut)
                }
                InsertFilter::InsertAll => {
                    // insert all internal calls and logs
                    let calls_fut = future::try_join_all(
                        evaluation
                            .tx
                            .all_calls()
                            .map(|call| self.insert_call_with_statement(insert_call_stmt, call)),
                    );

                    let logs_fut =
                        future::try_join_all(evaluation.tx.all_logs().map(|log| {
                            self.insert_log_with_statement(insert_event_log_stmt, &*log)
                        }));

                    (calls_fut, logs_fut)
                }
            };

        future::try_join(calls_fut, logs_fut).await?;

        Ok(())
    }

    /// Inserts data from this evaluation to PostGres
    pub async fn insert(&self, evaluation: &Evaluation) -> Result<(), DbError> {
        if let Some(ref stmts) = self.prepared_statements {
            Ok(self.insert_with_statements(evaluation, stmts).await?)
        } else {
            let stmts = self.get_prepared_stmts().await?;
            Ok(self.insert_with_statements(evaluation, &stmts).await?)
        }
    }

    /// Checks if the transaction hash is already inspected
    pub async fn exists(&self, hash: TxHash) -> Result<bool, DbError> {
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
    pub async fn block_exists(&self, block: u64) -> Result<bool, DbError> {
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

    pub async fn clear(&self) -> Result<(), DbError> {
        self.client
            .batch_execute(&format!("DROP TABLE {}", self.table_name))
            .await?;
        Ok(())
    }
}

///
#[derive(Debug, Copy, Clone)]
pub enum InsertFilter {
    /// Insert the `Evaluation` only without any additional `TransactionData`
    EvaluationOnly,
    /// Insert data (internal call, logs) that was used when analyzing the Tx
    Essential,
    /// Insert all internal calls and logs
    InsertAll,
}

impl Default for InsertFilter {
    fn default() -> Self {
        InsertFilter::Essential
    }
}

#[derive(Error, Debug)]
pub enum DbError {
    #[error(transparent)]
    Decimal(#[from] rust_decimal::Error),

    /// Occurs when converting a `FromSql` type failed
    #[error("{0}")]
    FromSqlError(String),

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
                            eval.tx.block_number,
                            eval.tx.hash,
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
    db: MevDB,
) -> Result<(Evaluation, MevDB), (MevDB, DbError)> {
    if let Err(err) = db.insert(&eval).await {
        log::error!("DB insert failed: {:?}", err);
        Err((db, err))
    } else {
        log::debug!(
            "inserted evaluation of block {} with tx {}",
            eval.tx.block_number,
            eval.tx.hash
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
    use std::collections::HashSet;

    use crate::test_helpers::{get_tx, test_inspector};
    use crate::types::evaluation::ActionType;

    use super::*;

    /// This expects postgres running on localhost:5432 with user `postgres` and table `mev_inspections_test`
    async fn mock_mevdb() -> MevDB {
        let mut config = Config::default();
        config
            .host("localhost")
            .user("postgres")
            .dbname("mev_inspections_test");
        MevDB::connect(config).await.unwrap()
    }

    fn mock_evaluation() -> Evaluation {
        let mut tx = get_tx("0x93690c02fc4d58734225d898ea4091df104040450c0f204b6bf6f6850ac4602f");
        let inspector = test_inspector();
        inspector.inspect_tx(&mut tx);
        inspector.reduce_tx(&mut tx);

        let actions = [ActionType::Liquidation, ActionType::Arbitrage]
            .iter()
            .cloned()
            .collect::<HashSet<_>>();

        Evaluation {
            protocols: tx.protocols(),
            tx,
            gas_used: 21000.into(),
            gas_price: (100e9 as u64).into(),
            actions,
            profit: (1e18 as u64).into(),
        }
    }

    #[tokio::test]
    async fn insert_all() {
        let client = mock_mevdb()
            .await
            .with_insert_filter(InsertFilter::InsertAll);
        let _ = client.redo_migration().await;

        let evaluation = mock_evaluation();
        client.insert(&evaluation).await.unwrap();
        let evals = client
            .select_blocks(evaluation.tx.block_number..=evaluation.tx.block_number)
            .await
            .unwrap();
        assert_eq!(evals.len(), 1);
        assert_eq!(evals[&evaluation.tx.block_number].len(), 1);

        client.revert_migration().await.unwrap();
    }

    #[tokio::test]
    async fn insert_eval_only() {
        let client = mock_mevdb()
            .await
            .with_insert_filter(InsertFilter::EvaluationOnly);

        let _ = client.redo_migration().await;

        let evaluation = mock_evaluation();
        client.insert(&evaluation).await.unwrap();

        assert_eq!(
            client
                .select_transaction(evaluation.tx.hash)
                .await
                .unwrap()
                .tx
                .hash,
            evaluation.tx.hash
        );
        assert!(client.exists(evaluation.as_ref().hash).await.unwrap());

        assert_eq!(
            client.latest_block().await.unwrap(),
            evaluation.tx.block_number
        );

        let selected = client.select_where_eoa(evaluation.tx.from).await.unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].tx.hash, evaluation.tx.hash);
        assert_eq!(&selected[0].actions, &evaluation.actions);

        for proto in evaluation.protocols.iter().cloned() {
            let selected = client.select_where_protocols(&[proto]).await.unwrap();
            assert_eq!(selected.len(), 1);
        }

        for action in evaluation.actions.iter().cloned() {
            let selected = client.select_where_actions(&[action]).await.unwrap();
            assert_eq!(selected.len(), 1);
        }

        client.revert_migration().await.unwrap();
    }
}
