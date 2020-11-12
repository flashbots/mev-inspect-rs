use crate::addresses::{ETH, WETH};
use ethers::{
    contract::{abigen, ContractError},
    providers::Middleware,
    types::{Address, BlockNumber, U256},
};
use std::sync::Arc;

// Generate type-safe bindings to Uniswap's router
abigen!(Uniswap, "abi/unirouterv2.json");

/// Gets historical prices in ETH for any token via Uniswap.
/// **Requires an archive node to work**
pub struct HistoricalPrice<M> {
    uniswap: Uniswap<M>,
}

impl<M: Middleware> HistoricalPrice<M> {
    /// Instantiates a Unirouter
    pub fn new<T: Into<Arc<M>>>(provider: T) -> Self {
        let unirouter: Address = "7a250d5630b4cf539739df2c5dacb4c659f2488d"
            .parse()
            .expect("cannot parse unirouter");
        Self {
            uniswap: Uniswap::new(unirouter.clone(), provider.into()),
        }
    }

    /// Converts any token amount to ETH by querying historical Uniswap prices
    /// at a specific block
    pub async fn quote<T: Into<BlockNumber>, A: Into<U256>>(
        &self,
        token: Address,
        amount: A,
        block: T,
    ) -> Result<U256, ContractError<M>> {
        let amount = amount.into();

        // assume price parity of WETH / ETH
        if token == *ETH || token == *WETH {
            return Ok(amount);
        }

        // ask uniswap how much we'd get from the TOKEN -> WETH path
        let amounts = self
            .uniswap
            .get_amounts_out(amount, vec![token, *WETH])
            .block(block)
            .call()
            .await?;
        // .map_err(HistoricalPriceError::MiddlewareError)?;
        debug_assert_eq!(amount, amounts[0]);
        debug_assert_eq!(amounts.len(), 2);

        Ok(amounts[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethers::providers::{Http, Provider};
    use std::convert::TryFrom;

    fn to_eth(amt: U256) -> U256 {
        ethers::utils::WEI_IN_ETHER / amt
    }

    const ONE: u64 = 1000000;

    #[tokio::test]
    #[ignore] // This test can only run against an archive node
    async fn check_historical_price() {
        let url: String = std::env::var("ARCHIVE").expect("Archive node URL should be set");
        let provider = Provider::<Http>::try_from(url).unwrap();
        let prices = HistoricalPrice::new(provider);

        // 1 usdc (6 decimals) in ETH
        let usdc = "a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".parse().unwrap();
        let usdc_amount = prices.quote(usdc, ONE, BlockNumber::Latest).await.unwrap();
        assert_eq!(to_eth(usdc_amount), 466.into());

        let usdc_amount = prices.quote(usdc, ONE, 10532013).await.unwrap();
        assert_eq!(to_eth(usdc_amount), 302.into());
    }
}
