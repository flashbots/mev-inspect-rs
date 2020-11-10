use crate::{
    inspectors::Uniswap,
    types::{
        actions::{SpecificAction, Trade},
        Classification,
    },
};
use itertools::{EitherOrBoth::*, Itertools};

impl Uniswap {
    pub fn combine_transfers(&self, classifications: &mut [Classification]) {
        use Classification::Known;
        let actions = classifications.to_vec();
        let mut prune = 0;
        classifications
            .iter_mut()
            .enumerate()
            .zip_longest(actions.iter().skip(1))
            .for_each(|mut pair| match pair {
                Both((i, ref mut a1), ref mut a2) => {
                    // prune elements which have been reduced already
                    // TODO: Figure out how to do this properly.
                    if i > 0 && i == prune {
                        **a1 = Classification::Prune;
                        return;
                    }

                    match (&a1, &a2) {
                        (Known(ref b1), Known(ref b2)) => match (&b1.as_ref(), &b2.as_ref()) {
                            (SpecificAction::Transfer(t1), SpecificAction::Transfer(t2)) => {
                                // Hack to filter out weird dups
                                if t1.token == t2.token {
                                    **a1 = Classification::Prune;
                                    return;
                                }

                                if t1.to == t2.from {
                                    let action = Trade {
                                        t1: t1.clone(),
                                        t2: t2.clone(),
                                    };

                                    // TODO: Figure out what the trace should be here
                                    **a1 = Classification::new(action, Vec::new());

                                    // Since we paired with the next element,
                                    // we should prune it, if the element after the
                                    // next does not also execute a matching trade
                                    if let Some(actions) = actions.get(i + 2) {
                                        match &actions {
                                            Known(ref inner) => match &inner.as_ref() {
                                                SpecificAction::Transfer(t3) => {
                                                    if t2.to == t3.from {
                                                        prune = 0;
                                                        return;
                                                    }
                                                }
                                                _ => (),
                                            },
                                            _ => (),
                                        };
                                    }

                                    prune = i + 1;
                                }
                            }
                            // TODO: Is there a way to avoid doing these _ => () branches?
                            _ => (),
                        },
                        _ => (),
                    };
                }
                Left((i, ref mut a1)) => {
                    if i > 0 && i == prune {
                        **a1 = Classification::Prune;
                        return;
                    }
                }
                Right(_) => (),
            });
    }
}

#[cfg(test)]
mod reducer {
    use super::*;
    use crate::types::{actions::Transfer, Inspection, Status};
    use ethers::types::{Address, TxHash};

    fn mk_inspection(actions: Vec<Classification>) -> Inspection {
        Inspection {
            status: Status::Success,
            actions,
            protocols: vec![],
            from: Address::zero(),
            contract: Address::zero(),
            proxy_impl: None,
            hash: TxHash::zero(),
            block_number: 0,
        }
    }

    fn test_combine(input: Vec<Classification>, expected: Vec<Classification>) {
        let uniswap = Uniswap::new();
        let mut inspection = mk_inspection(input);
        uniswap.combine_transfers(&mut inspection.actions);
        assert_eq!(inspection.actions, expected);
    }

    fn addrs() -> Vec<Address> {
        use ethers::core::rand::thread_rng;
        (0..10)
            .into_iter()
            .map(|_| ethers::signers::LocalWallet::new(&mut thread_rng()).address())
            .collect()
    }

    #[test]
    // based on https://etherscan.io/tx/0xddbf97f758bd0958487e18d9e307cd1256b1ad6763cd34090f4c9720ba1b4acc
    fn transfer_loop_ok() {
        let addrs = addrs();
        let token1 = addrs[0];
        let token2 = addrs[1];
        let token3 = addrs[2];
        let token4 = addrs[3];

        let usr = addrs[4];
        let uni1 = addrs[5];
        let uni2 = addrs[6];
        let uni3 = addrs[7];
        let uni4 = addrs[8];

        let t1 = Transfer {
            from: usr,
            to: uni1,
            amount: 120.into(),
            token: token1,
        };

        let t2 = Transfer {
            from: uni1,
            to: uni2,
            amount: 194.into(),
            token: token2,
        };

        let t3 = Transfer {
            from: uni2,
            to: uni3,
            amount: 98.into(),
            token: token3,
        };

        let t4 = Transfer {
            from: uni3,
            to: uni4,
            amount: 164.into(),
            token: token4,
        };

        let t5 = Transfer {
            from: uni4,
            to: usr,
            amount: 121.into(),
            token: token1,
        };

        // the 5 transfers get condensed down to 4 trades
        let input = vec![
            Classification::new(t1.clone(), Vec::new()),
            Classification::new(t2.clone(), Vec::new()),
            Classification::new(t3.clone(), Vec::new()),
            Classification::new(t4.clone(), Vec::new()),
            Classification::new(t5.clone(), Vec::new()),
        ];
        let expected = vec![
            Classification::new(Trade { t1, t2: t2.clone() }, Vec::new()),
            Classification::new(
                Trade {
                    t1: t2,
                    t2: t3.clone(),
                },
                Vec::new(),
            ),
            Classification::new(
                Trade {
                    t1: t3.clone(),
                    t2: t4.clone(),
                },
                Vec::new(),
            ),
            Classification::new(
                Trade {
                    t1: t4,
                    t2: t5.clone(),
                },
                Vec::new(),
            ),
            Classification::Prune,
        ];

        test_combine(input, expected);
    }

    #[test]
    fn two_continuous_transfers_ok() {
        let addrs = addrs();
        let token1 = addrs[0];
        let token2 = addrs[1];
        let addr1 = addrs[2];
        let addr2 = addrs[3];

        let t1 = Transfer {
            from: addr1,
            to: addr2,
            amount: 1.into(),
            token: token1,
        };

        let t2 = Transfer {
            from: addr2,
            to: addr1,
            amount: 5.into(),
            token: token2,
        };

        let input = vec![
            Classification::new(t1.clone(), Vec::new()),
            Classification::new(t2.clone(), Vec::new()),
        ];
        let expected = vec![
            Classification::new(Trade { t1, t2 }, Vec::new()),
            Classification::Prune,
        ];

        test_combine(input, expected);
    }

    // #[test]
    fn non_continuous_transfers_ok() {
        let addrs = addrs();
        let token1 = addrs[0];
        let token2 = addrs[1];
        let addr1 = addrs[2];
        let addr2 = addrs[3];

        let t1 = Transfer {
            from: addr1,
            to: addr2,
            amount: 1.into(),
            token: token1,
        };

        let t2 = Transfer {
            from: addr2,
            to: addr1,
            amount: 5.into(),
            token: token2,
        };

        // There's some junk between our two transfers
        let input = vec![
            Classification::new(t1.clone(), Vec::new()),
            Classification::Prune,
            Classification::new(t2.clone(), Vec::new()),
        ];
        // but it still understand that it's a trade
        let expected = vec![
            Classification::new(Trade { t1, t2 }, Vec::new()),
            Classification::Prune,
            Classification::Prune,
        ];

        test_combine(input, expected);
    }
}
