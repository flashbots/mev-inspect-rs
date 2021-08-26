use ethers::{prelude::JsonRpcClient, providers::Middleware};
use ethers::providers::Http as HttpProvider;
use url::{ParseError, Url};
use std::convert::TryFrom;

#[derive(Debug)]
pub struct Provider<P> {
    address: P,
}

// become enum
pub struct ProviderError;

impl<P: JsonRpcClient> Provider<P> {
    pub fn new(provider:P) -> Option<Self> {
        Some(Provider { address: provider })
    }
}

impl<P: JsonRpcClient> Middleware for Provider<P> {
    type Error = ProviderError;
    type Provider = P;
    type Inner = Self;

    fn inner(&self) -> &Self::Inner {
        unreachable!("no inner provider here")
    }

    fn provider(&self) -> &ethers::providers::Provider<Self::Provider> {
        self
    }

}

impl TryFrom<&str> for Provider<HttpProvider> {
    type Error = ParseError;

    fn try_from(src: &str) -> Result<Self, Self::Error> {
        Ok(Provider {
            address: HttpProvider::new(Url::parse(src)?),
        })
    }
}