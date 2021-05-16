use crate::{
    addresses::{ETH, WETH},
    inspectors::find_matching,
    types::{
        actions::{ProfitableLiquidation, Transfer},
        Classification, Inspection,
    },
    Reducer,
};

pub struct LiquidationReducer;

impl Reducer for LiquidationReducer {
    fn reduce(&self, inspection: &mut Inspection) {
        let actions = inspection.actions.clone();
        let mut prune = Vec::new();

        // 1. find all the liquidations and populate their received amount with
        // the transfer that was their subtrace
        // 2. find the tx right before the liquidation which has a matching token
        // as the received token and use that to calculate the profit
        inspection
            .actions
            .iter_mut()
            .enumerate()
            .for_each(|(i, ref mut action)| {
                let opt = action
                    .as_action_mut()
                    .map(|x| x.as_liquidation_mut())
                    .flatten();
                let liquidation = if let Some(liquidation) = opt {
                    liquidation
                } else {
                    return;
                };

                // find the transfer which corresponds to this liquidation
                let mut liq = liquidation.clone();
                let check_fn = |t: &Transfer| {
                    t.to == liq.from && (t.token == liq.received_token || t.token == *ETH)
                };

                // found the transfer after, which is the one that pays us
                let res = find_matching(
                    actions.iter().enumerate().skip(i + 1),
                    |t| t.as_transfer(),
                    check_fn,
                    true,
                );
                if let Some((idx, received)) = res {
                    // prune the repayment subcall
                    prune.push(idx);

                    // there may be a DEX trade before the liquidation, allowing
                    // us to instantly determine if it's a profitable liquidation
                    // or not
                    let res = find_matching(
                        actions.iter().enumerate(),
                        |t| t.as_trade(),
                        |t| t.t2.token == liq.sent_token,
                        true,
                    );

                    if let Some((_, paid)) = res {
                        // prune.push(idx2);
                        let tokens_match = (received.token == paid.t1.token)
                            || ((received.token == *ETH && paid.t1.token == *WETH)
                                || (received.token == *WETH && paid.t1.token == *ETH));
                        if received.amount > paid.t1.amount && tokens_match {
                            liq.received_amount = received.amount;
                            let profitable_liq = ProfitableLiquidation {
                                token: paid.t1.token,
                                liquidation: liq.clone(),
                                profit: received.amount - paid.t1.amount,
                            };
                            **action = Classification::new(profitable_liq, Vec::new());
                            return;
                        }
                    }

                    liquidation.received_amount = received.amount;
                }
            });
        for i in prune {
            inspection.actions[i] = Classification::Prune;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;
    use crate::types::actions::{Liquidation, Trade, Transfer};

    fn test_profitable_liquidation(input: Vec<Classification>, expected: Vec<Classification>) {
        let aave = LiquidationReducer::new();
        let mut inspection = mk_inspection(input);
        aave.reduce(&mut inspection);
        assert_eq!(inspection.actions, expected);
    }

    #[test]
    fn to_profitable_liquidation() {
        let addrs = addrs();

        let token = addrs[0];
        let token1 = addrs[1];
        let token2 = addrs[6];
        let usr = addrs[2];
        let liquidated = addrs[3];
        let vault = addrs[4];
        let dex = addrs[5];

        // trade USDC for ETH
        let trade1 = Trade {
            t1: Transfer {
                token,
                from: usr,
                amount: 100.into(),
                to: dex,
            },
            t2: Transfer {
                token: token1,
                from: dex,
                amount: 1.into(),
                to: dex,
            },
        };

        // trade ETH for YFI
        let trade2 = Trade {
            t1: Transfer {
                token: token1,
                from: dex,
                amount: 1.into(),
                to: dex,
            },
            t2: Transfer {
                token: token2,
                from: dex,
                amount: 5.into(),
                to: dex,
            },
        };

        // sends YFI
        let repayment = Transfer {
            token: token2,
            from: dex,
            amount: 5.into(),
            to: vault, // goes to the user's underwater vault
        };

        // repays YFI, gets ETH collateral
        let mut liquidation = Liquidation {
            sent_token: token2,
            sent_amount: 5.into(),

            received_token: token1,
            // This is 0 and will get populated via the transfer subtrace
            received_amount: 0.into(),

            from: usr,
            liquidated_user: liquidated,
        };

        // gets paid out in ETH
        let payout = Transfer {
            token: token1,
            to: usr,
            from: vault,
            amount: 3.into(),
        };

        liquidation.received_amount = payout.amount;
        let res = ProfitableLiquidation {
            liquidation: liquidation.clone(),
            profit: 2.into(),
            token: token1,
        };

        // we expect that we are left with a ProfitableLiquidation.
        // the transfer _after_ is culled
        let input = vec![
            Classification::new(trade1.clone(), Vec::new()),
            Classification::new(trade2.clone(), Vec::new()),
            Classification::new(repayment.clone(), Vec::new()),
            // the payout tx is a subtrace of the liquidation tx
            Classification::new(liquidation, vec![0, 5]),
            Classification::new(payout, vec![0, 5, 3]),
        ];
        let expected = vec![
            Classification::new(trade1, Vec::new()),
            Classification::new(trade2, Vec::new()),
            Classification::new(repayment, Vec::new()),
            Classification::new(res, Vec::new()),
            Classification::Prune,
        ];

        test_profitable_liquidation(input, expected);
    }
}
