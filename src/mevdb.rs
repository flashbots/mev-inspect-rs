use crate::types::Evaluation;
use ethers::types::{TxHash, U256};
use rust_decimal::prelude::*;
use thiserror::Error;
use tokio_postgres::{Client, NoTls};

/// Wrapper around PostGres for storing results in the database
pub struct MevDB<'a> {
    client: Client,
    table_name: &'a str,
}

impl<'a> MevDB<'a> {
    /// Connects to the MEV PostGres instance
    pub async fn connect(
        host: &str,
        user: &str,
        password: Option<String>,
        table_name: &'a str,
    ) -> Result<MevDB<'a>, DbError> {
        let mut auth = format!("host={} user={}", host, user);
        if let Some(password) = password {
            auth = format!("{} password={}", auth, password);
        }

        let (client, connection) = tokio_postgres::connect(&auth, NoTls).await?;

        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("connection error: {}", e);
            }
        });

        Ok(Self { client, table_name })
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
                    proxy_impl text
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
                    ",
                    self.table_name
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
                        .unwrap_or("".to_owned()),
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

    #[tokio::test]
    async fn insert_eval() {
        let mut client = MevDB::connect("localhost", "postgres", None, "test_table")
            .await
            .unwrap();
        client.clear().await.unwrap();
        client.create().await.unwrap();

        let inspection = Inspection {
            status: crate::types::Status::Checked,
            actions: Vec::new(),
            protocols: Vec::new(),
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
        assert!(client
            .block_exists(evaluation.as_ref().block_number)
            .await
            .unwrap());
        assert!(!client.block_exists(8).await.unwrap());
    }
}
