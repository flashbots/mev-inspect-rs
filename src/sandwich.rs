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
use crate::types::Protocol;
use ethers::types::Address;

pub struct Detector {
    earliest_block: Option<u64>,
    latest_block: Option<u64>,
    contracts: Vec<Address>,
    protocols: Vec<Protocol>,
    eoa: Vec<Address>,
    min_revenue: Option<u64>,
    max_revenue: Option<u64>,
}

pub struct SandwichDetector {}

pub enum SandwichAttack {}

pub struct LiquidityTakerAttacker {
    /// The attackers ETH address that executed that transactions
    pub address: Address,
    /// All the executed transactions
    pub transactions: (),
}

pub struct DexTransaction {
    input: (),
    output: (),
    ty: (),
}

pub enum AssetSwapTransaction {
    XforY,
    YforX,
}

pub enum LiquidityTransaction {
    Add,
    Remove,
}
