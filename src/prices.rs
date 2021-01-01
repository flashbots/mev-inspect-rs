#![allow(clippy::clippy::too_many_arguments)]
use crate::addresses::{parse_address, ETH, WETH};
use ethers::{
    contract::{abigen, ContractError},
    providers::Middleware,
    types::{Address, BlockNumber, U256},
    utils::WEI_IN_ETHER,
};
use once_cell::sync::Lazy;
use std::{collections::HashMap, sync::Arc};

// Generate type-safe bindings to Uniswap's router
abigen!(Uniswap, "abi/unirouterv2.json");

/// Gets historical prices in ETH for any token via Uniswap.
/// **Requires an archive node to work**
pub struct HistoricalPrice<M> {
    uniswap: Uniswap<M>,
}

static DECIMALS: Lazy<HashMap<Address, usize>> = Lazy::new(|| {
    [("a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", 6)]
        .iter()
        .map(|(addr, decimals)| (parse_address(addr), *decimals))
        .collect::<HashMap<_, _>>()
});

impl<M: Middleware> HistoricalPrice<M> {
    /// Instantiates a Unirouter
    pub fn new<T: Into<Arc<M>>>(provider: T) -> Self {
        let unirouter: Address = "7a250d5630b4cf539739df2c5dacb4c659f2488d"
            .parse()
            .expect("cannot parse unirouter");
        Self {
            uniswap: Uniswap::new(unirouter, provider.into()),
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

        // get a marginal price for a 1 ETH buy order
        let one = DECIMALS
            .get(&token)
            .map(|decimals| U256::from(10u64.pow(*decimals as u32)))
            .unwrap_or(WEI_IN_ETHER);

        // ask uniswap how much we'd get from the TOKEN -> WETH path
        let amounts = self
            .uniswap
            .get_amounts_out(one, vec![token, *WETH])
            .block(block)
            .call()
            .await?;

        debug_assert_eq!(one, amounts[0]);
        debug_assert_eq!(amounts.len(), 2);
        Ok(amounts[1] * amount / one)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethers::{
        providers::{Http, Provider},
        utils::WEI_IN_ETHER as WEI,
    };
    use std::convert::TryFrom;

    fn to_eth(amt: U256) -> U256 {
        ethers::utils::WEI_IN_ETHER / amt
    }

    static PROVIDER: Lazy<Provider<Http>> = Lazy::new(|| {
        let url: String = std::env::var("ARCHIVE").expect("Archive node URL should be set");
        let provider = Provider::<Http>::try_from(url).unwrap();
        provider
    });

    #[tokio::test]
    #[ignore] // This test can only run against an archive node
    async fn check_historical_price() {
        let prices = HistoricalPrice::new(PROVIDER.clone());
        let one = U256::from(1e6 as u64);

        for (token, amount, block, expected) in [
            (
                "a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
                one,
                11248959u64,
                465u64,
            ),
            (
                "a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
                one,
                10532013,
                302,
            ),
            (
                "e41d2489571d322189246dafa5ebde1f4699f498",
                WEI,
                11248959,
                1277,
            ),
        ]
        .iter()
        {
            let amount = prices
                .quote(parse_address(token), *amount, *block)
                .await
                .unwrap();
            assert_eq!(to_eth(amount), (*expected).into());
        }
    }

    #[tokio::test]
    #[ignore] // This test can only run against an archive node
    async fn old_block_fail() {
        let prices = HistoricalPrice::new(PROVIDER.clone());
        prices
            .quote(
                parse_address("e41d2489571d322189246dafa5ebde1f4699f498"),
                WEI,
                9082920,
            )
            .await
            .unwrap_err();
    }
}
