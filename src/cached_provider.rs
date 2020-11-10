use async_trait::async_trait;
use ethers::{
    providers::{FromErr, Middleware},
    types::{BlockNumber, Trace},
};
use serde::{de::DeserializeOwned, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct CachedProvider<M> {
    inner: M,
    cache: PathBuf,
}

use thiserror::Error;

impl<M: Middleware> CachedProvider<M> {
    /// Creates a new provider with the cache located at the provided path
    pub fn new<P: Into<PathBuf>>(inner: M, cache: P) -> Self {
        Self {
            inner,
            cache: cache.into(),
        }
    }

    fn read<T: DeserializeOwned, K: AsRef<Path>>(
        &self,
        fname: K,
    ) -> Result<T, CachedProviderError<M>> {
        let path = self.cache.join(fname);
        let json = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str::<T>(&json)?)
    }

    fn write<T: Serialize, K: AsRef<Path>>(
        &self,
        fname: K,
        data: T,
    ) -> Result<(), CachedProviderError<M>> {
        let path = self.cache.join(fname);
        let writer = std::fs::File::create(path)?;
        Ok(serde_json::to_writer(writer, &data)?)
    }
}

#[async_trait]
impl<M: Middleware> Middleware for CachedProvider<M> {
    type Error = CachedProviderError<M>;
    type Provider = M::Provider;
    type Inner = M;

    fn inner(&self) -> &Self::Inner {
        &self.inner
    }

    async fn trace_block(&self, block: BlockNumber) -> Result<Vec<Trace>, Self::Error> {
        // check if it exists, else get from the provider
        let mut traces = None;
        if let BlockNumber::Number(block_number) = block {
            traces = self
                .read(format!("{}.trace.json", block_number.as_u64()))
                .ok();
        };

        if let Some(traces) = traces {
            Ok(traces)
        } else {
            let traces: Vec<Trace> = self
                .inner()
                .trace_block(block)
                .await
                .map_err(CachedProviderError::MiddlewareError)?;

            let block_number = if let BlockNumber::Number(block_number) = block {
                block_number.as_u64()
            } else {
                self.get_block_number().await?.as_u64()
            };

            self.write(format!("{}.trace.json", block_number), &traces)?;

            Ok(traces)
        }
    }
}

#[derive(Error, Debug)]
pub enum CachedProviderError<M: Middleware> {
    /// Thrown when the internal middleware errors
    #[error("{0}")]
    MiddlewareError(M::Error),
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    DeserializationError(#[from] serde_json::Error),
}

impl<M: Middleware> FromErr<M::Error> for CachedProviderError<M> {
    fn from(src: M::Error) -> Self {
        CachedProviderError::MiddlewareError(src)
    }
}
