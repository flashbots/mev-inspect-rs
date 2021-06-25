//! A module to detect sandwich attacks
//!
//! Contains detection schemes for two kinds of sandwich attacks on AMM DEX.
//! Scenario descriptions taken from [https://arxiv.org/pdf/2009.14021.pdf]
//!
//! # Attack scenario 1: Liquidity Taker Attacks Taker
//!
//! In this scenario an adversarial liquidity taker tries to exploit the
//! victim's `TransactXforY` transaction (`Txy_v`) after it was emitted on the
//! Ethereum network by front- (`Txy_a`) and then back-running (`Tyx_a`) it.
//!
//! The timeline of the transactions looks as follows:
//!
//! ```text
//!   |                                        transaction order
//!   |                                                |
//!   |                Txy_a-------------------------->| front running
//!   |      Txy_v------------------------------------>|
//!   |                Tyx_a-------------------------->| back running
//!   |                                                V
//! Block N                                      Block N+x
//! ---------time appearance on Ethereum network------->
//! ```
//!
//! The attacker's goal is to make sure transactions `Txy_a`, `Txy_v` and
//! `Tyx_a` appear in the same block in that order, so that the attacker profits
//! from the victim's slippage. The attacker can influence the position of their
//! adversarial transactions, relative to the victim's transaction (`Txy_v`), by
//! paying a higher (`Txy_a`), or lower (`Tyx_a`) gas price.
//!
//!
//! # Attack scenario 2: Liquidity Provider Attacks Taker
//!
//! In this scenario an attacker targets the victims `TransactXforY` transaction
//! (`Txy_v`) by emitting 3 transactions:
//!
//! 1) `RemoveLiquidity` `Tout_a` (increases victim’s slippage)
//!         - attacker withdraws δx of asset X, and δy of asset Y
//! 2) `AddLiquidity` `Tin_a` (restores pool liquidity)
//!         - attacker deposits δx of asset X, and δy of asset Y
//! 3) `TransactYforX` `Tyx_a` (restores asset balance of X)
//!         - attacker trades δy of asset Y, increasing the available liquidity
//!           of asset Y, in exchange for δx
//!
//! 1)  The front-running `RemoveLiquidity` transaction (`Tout_a`) reduces the
//! market liquidity of the AMM DEX and increases the victim’s unexpected
//! slippage. 2)  The back-running `AddLiquidity` transaction (`Tin_a`) restores
//! the percentage of liquidity A holds before the attack 3)  The back-running
//! transaction `TransactYforX` (`Tyx_a`) equilibrates the adversary’s balance
//! of asset X to the state before the attack.
//!
//! Note: because the attacker withdraws all the liquidity 1), and only restores
//! it in 2), the attacker misses out on a potential commission fee of the
//! victim's transaction `Txy_v`
//!
//! The timeline of the transactions looks as follows:
//!
//! ```text
//!   |                                        transaction order
//!   |                Tout_a------------------------->| front running
//!   |      Txy_v------------------------------------>|
//!   |                Tin_a-------------------------->| back running
//!   |                Tyx_a-------------------------->| back running
//!   |                                                V
//! Block N                                      Block N+x
//! ---------time appearance on Ethereum network------->
//! ```
//!
//! To detect such transaction patterns, you can select a eoa that is suspected to be sandwich trader or search by blocks and contract.
//! To narrow down potential matches some parameters are necessary:

#![allow(unused)]
#![allow(dead_code)]
use crate::mevdb::DbError;
use crate::types::evaluation::ActionType;
use crate::types::{Evaluation, Protocol};
use crate::MevDB;
use ethers::types::Address;
use std::collections::BTreeSet;
use std::ops::Range;

#[derive(Debug, Clone)]
pub struct SandwichDetector {
    config: DetectorConfig,
    /// All the blocks and their `Evaluation`
    blocks: Vec<(u64, Vec<Evaluation>)>,
}

impl SandwichDetector {
    /// Initialise a new detector
    pub async fn new(mevdb: &MevDB, config: impl Into<DetectorConfig>) -> Result<Self, DbError> {
        let config = config.into();
        let blocks = mevdb.select_blocks(config.blocks.iter().cloned()).await?;
        Ok(Self {
            config,
            blocks: blocks.into_iter().collect(),
        })
    }

    /// Returns an iterator over all blocks
    pub fn blocks(&self) -> impl Iterator<Item = &(u64, Vec<Evaluation>)> {
        self.blocks.iter()
    }

    /// Returns an iterator over all the adversaries found in the selected blocks
    pub fn adversaries(&self, filter: AdversaryFilter) -> impl Iterator<Item = (u64, Adversary)> {
        self.blocks.iter().flat_map(move |(block, evals)| {
            let block = *block;
            evals
                .iter()
                .enumerate()
                .filter_map(move |(idx, e)| filter.find_adversary(e, evals.iter().skip(idx)))
                .map(move |a| (block, a))
        })
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum AdversaryFilter {
    SandwichTrade,
    LiquidityProvider,
}

#[derive(Debug, Clone)]
pub enum Adversary<'a> {
    SandwichTrade {
        /// The front running trade
        front: &'a Evaluation,
        /// The back running trade
        back: &'a Evaluation,
    },
    LiquidityProvider {
        /// The front running remove liquidity
        remove: &'a Evaluation,
        /// The back running add liquidity
        add: &'a Evaluation,
        /// The back running trade
        trade: &'a Evaluation,
    },
}

impl AdversaryFilter {
    /// Determines whether this
    pub fn find_adversary<'a>(
        &self,
        eval: &'a Evaluation,
        mut remaining: impl Iterator<Item = &'a Evaluation>,
    ) -> Option<Adversary<'a>> {
        match self {
            AdversaryFilter::SandwichTrade => {
                if eval.actions.contains(&ActionType::Trade) {
                    if let Some(back) = remaining.find(|e| {
                        eval.tx.from == e.tx.from && e.actions.contains(&ActionType::Trade)
                    }) {
                        return Some(Adversary::SandwichTrade { front: eval, back });
                    }
                }
            }
            AdversaryFilter::LiquidityProvider => {
                if eval.actions.contains(&ActionType::RemoveLiquidity) {
                    let mut add = None;
                    for e in remaining {
                        if eval.tx.from == e.tx.from {
                            if e.actions.contains(&ActionType::AddLiquidity) {
                                add = Some(e);
                                continue;
                            }
                            if e.actions.contains(&ActionType::Trade) {
                                if let Some(add) = add.take() {
                                    return Some(Adversary::LiquidityProvider {
                                        remove: eval,
                                        add,
                                        trade: e,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct DetectorConfig {
    blocks: BTreeSet<u64>,
    contracts: Vec<Address>,
    protocols: Vec<Protocol>,
}

impl DetectorConfig {
    pub fn new(blocks: impl IntoIterator<Item = u64>) -> Self {
        Self {
            blocks: blocks.into_iter().collect(),
            contracts: vec![],
            protocols: vec![],
        }
    }
}
