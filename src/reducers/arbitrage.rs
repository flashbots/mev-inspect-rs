use crate::{
    inspectors::find_matching_trade_after,
    types::{actions::Arbitrage, Classification, Inspection},
    Reducer,
};

#[derive(Clone, Debug)]
pub struct ArbitrageReducer;

impl ArbitrageReducer {
    pub fn new() -> Self {
        Self
    }
}

impl Reducer for ArbitrageReducer {
    fn reduce(&self, inspection: &mut Inspection) {
        let actions = inspection.actions.to_vec();
        let mut prune = Vec::new();
        inspection
            .actions
            .iter_mut()
            .enumerate()
            .for_each(|(i, action)| {
                // check if we got a trade
                let trade = if let Some(trade) = action.to_action().map(|x| x.trade()).flatten() {
                    trade
                } else {
                    return;
                };

                if let Some((j, trade2)) = find_matching_trade_after(&actions, i, trade.t1.token) {
                    *action = Classification::new(
                        Arbitrage {
                            profit: trade2.t2.amount.saturating_sub(trade.t1.amount),
                            token: trade2.t2.token,
                            to: trade2.t2.to,
                        },
                        // TODO!
                        Vec::new(),
                    );
                    // prune evrything in that range
                    prune.push((i + 1, j + 1));
                }
            });

        for range in prune {
            inspection.actions[range.0..range.1]
                .iter_mut()
                .for_each(|a| *a = Classification::Prune);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;
    use crate::types::actions::{Arbitrage, Trade, Transfer};

    fn test_trade_to_arbitrage(input: Vec<Classification>, expected: Vec<Classification>) {
        let uniswap = ArbitrageReducer::new();
        let mut inspection = mk_inspection(input);
        uniswap.reduce(&mut inspection);
        assert_eq!(inspection.actions, expected);
    }

    #[test]
    fn simple_arb() {
        let addrs = addrs();
        let token1 = addrs[0];
        let token2 = addrs[1];

        let usr = addrs[4];
        let uni1 = addrs[5];
        let uni2 = addrs[6];

        let t1 = Trade::merge(
            Transfer {
                from: usr,
                to: uni1,
                amount: 100.into(),
                token: token1,
            },
            Transfer {
                from: uni1,
                to: usr,
                amount: 200.into(),
                token: token2,
            },
        );

        let t2 = Trade::merge(
            Transfer {
                from: usr,
                to: uni2,
                amount: 200.into(),
                token: token2,
            },
            Transfer {
                from: uni2,
                to: usr,
                amount: 110.into(),
                token: token1,
            },
        );

        // the 2 trades get condensed down to 1 arb
        let input = vec![
            Classification::new(t1.clone(), Vec::new()),
            Classification::new(t2.clone(), Vec::new()),
        ];
        let expected = vec![
            Classification::new(
                Arbitrage {
                    profit: 10.into(),
                    token: token1,
                    to: usr,
                },
                Vec::new(),
            ),
            Classification::Prune,
        ];

        test_trade_to_arbitrage(input, expected);
    }
}
