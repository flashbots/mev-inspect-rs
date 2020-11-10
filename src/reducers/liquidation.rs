use crate::{
    addresses::ETH,
    is_subtrace,
    types::{
        actions::{ProfitableLiquidation, Transfer},
        Classification, Inspection,
    },
    Reducer,
};

pub struct LiquidationReducer;

impl LiquidationReducer {
    pub fn new() -> Self {
        Self
    }
}

impl Reducer for LiquidationReducer {
    fn reduce(&self, inspection: &mut Inspection) {
        let actions = inspection.actions.clone();
        let mut prune = Vec::new();

        // 1. find all the liquidations and populate their received amount with
        // the transfer that was their subtrace
        // 2. find the tx right before the liquidation which has a matching token
        // as the received token and use that to calculate the profit
        let mut profitable_liqs = Vec::new();
        inspection
            .actions
            .iter_mut()
            .enumerate()
            .for_each(|(i, ref mut action)| {
                let t1 = action.trace_address();
                let opt = action
                    .to_action_mut()
                    .map(|x| x.liquidation_mut())
                    .flatten();
                let liquidation = if let Some(liquidation) = opt {
                    liquidation
                } else {
                    return;
                };

                // find the transfer which corresponds to this liquidation
                let liq = liquidation.clone();
                let check_fn = |t: &Transfer| {
                    t.to == liq.from && (t.token == liq.received_token || t.token == *ETH)
                };
                let mut found_known = false;
                actions
                    .iter()
                    .enumerate()
                    .skip(i + 1)
                    .for_each(|(j, action2)| {
                        if let Some(a2) = action2.to_action() {
                            if is_subtrace(&t1, &action2.trace_address()) {
                                if let Some(t) = a2.transfer() {
                                    if !found_known && check_fn(t) {
                                        liquidation.received_amount = t.amount;
                                        prune.push(j);

                                        // If we've found the element below us,
                                        // we gotta find the first transfer _above_
                                        // us with a matching token as the liquidation's `received_token`
                                        // to figure out our profitability.
                                        actions.iter().rev().for_each(|action3| {
                                            let opt =
                                                action3.to_action().map(|x| x.transfer()).flatten();
                                            if let Some(transfer3) = opt {
                                                if transfer3.token == liquidation.received_token {
                                                    profitable_liqs.push((
                                                        i,
                                                        ProfitableLiquidation {
                                                            liquidation: liquidation.clone(),
                                                            token: transfer3.token,
                                                            profit: t.amount - transfer3.amount,
                                                        },
                                                    ));
                                                }
                                            }
                                        });
                                    }
                                }
                            }
                            found_known = true;
                        }
                    });
            });

        for (i, profit) in profitable_liqs {
            inspection.actions[i] = Classification::new(profit, Vec::new());
        }

        for i in prune {
            inspection.actions[i] = Classification::Prune;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;
    use crate::types::actions::{Liquidation, Transfer};

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

        // sends USDC
        let repayment1 = Transfer {
            token,
            from: usr,
            amount: 100.into(),
            to: dex,
        };

        // sends ETH
        let repayment2 = Transfer {
            token: token1,
            from: dex,
            amount: 1.into(),
            to: dex,
        };

        // sends YFI
        let repayment3 = Transfer {
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
            Classification::new(repayment1.clone(), Vec::new()),
            Classification::new(repayment2.clone(), Vec::new()),
            Classification::new(repayment3.clone(), Vec::new()),
            // the payout tx is a subtrace of the liquidation tx
            Classification::new(liquidation, vec![0, 5]),
            Classification::new(payout, vec![0, 5, 3]),
        ];
        let expected = vec![
            Classification::new(repayment1, Vec::new()),
            Classification::new(repayment2, Vec::new()),
            Classification::new(repayment3, Vec::new()),
            Classification::new(res, Vec::new()),
            Classification::Prune,
        ];

        test_profitable_liquidation(input, expected);
    }
}
