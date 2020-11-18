use crate::{
    inspectors::find_matching,
    types::{actions::Trade, Classification, Inspection},
    Reducer,
};

pub struct TradeReducer;

impl TradeReducer {
    /// Instantiates the reducer
    pub fn new() -> Self {
        Self
    }
}

impl Reducer for TradeReducer {
    fn reduce(&self, inspection: &mut Inspection) {
        let actions = inspection.actions.to_vec();
        let mut prune = Vec::new();
        inspection
            .actions
            .iter_mut()
            .enumerate()
            .for_each(|(i, action)| {
                // check if we got a transfer
                let transfer =
                    if let Some(transfer) = action.to_action().map(|x| x.transfer()).flatten() {
                        transfer
                    } else {
                        return;
                    };

                // find the first transfer after it
                let res = find_matching(
                    actions.iter().enumerate().skip(i + 1),
                    |t| t.transfer(),
                    |t| t.to == transfer.from && t.from == transfer.to,
                    false,
                );

                if let Some((j, transfer2)) = res {
                    *action = Classification::new(
                        Trade {
                            t1: transfer.clone(),
                            t2: transfer2.clone(),
                        },
                        Vec::new(),
                    );

                    // If there is no follow-up transfer that uses `transfer2`, prune it:
                    let res = find_matching(
                        actions.iter().enumerate().skip(j + 1),
                        |t| t.transfer(),
                        |t| t.to == transfer2.from && t.from == transfer2.to,
                        false,
                    );
                    if res.is_none() {
                        prune.push(j);
                    }
                }
            });

        prune
            .iter()
            .for_each(|p| inspection.actions[*p] = Classification::Prune);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;
    use crate::types::actions::Transfer;

    fn test_transfer_to_trade(input: Vec<Classification>, expected: Vec<Classification>) {
        let uniswap = TradeReducer::new();
        let mut inspection = mk_inspection(input);
        uniswap.reduce(&mut inspection);
        assert_eq!(inspection.actions, expected);
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

        test_transfer_to_trade(input, expected);
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

        test_transfer_to_trade(input, expected);
    }

    #[test]
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

        test_transfer_to_trade(input, expected);
    }
}
