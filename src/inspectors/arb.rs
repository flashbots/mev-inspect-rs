use super::types::Inspection;
use super::{
    prune,
    types::{Arbitrage, Classification, SpecificAction},
    Reducer,
};

#[derive(Clone, Debug)]
pub struct ArbitrageReducer;

impl Reducer for ArbitrageReducer {
    fn reduce(&self, inspection: &mut Inspection) {
        let actions = &mut inspection.actions;

        for i in 0..actions.len() {
            // If a trade is found,
            // 1. prune the previous element in teh array if it was a weth deposit
            // 2. find the first trade where `to` is the same as the 1st trade's `from`
            // and the tokens match. replace the first call's point with the arb, and
            // prune the rest
            if let Some(specific_action) = actions[i].to_action() {
                match specific_action {
                    SpecificAction::Trade(trade1) => {
                        let mut arb = None;
                        let actions2 = actions.clone();
                        let index = actions2.iter().position(|action| {
                            let mut res = false;
                            if let Some(spec) = action.to_action() {
                                match spec {
                                    SpecificAction::Trade(trade2) => {
                                        if trade1.t1.token == trade2.t2.token
                                        // TODO: Can we simply omit this?
                                        // && trade1.t1.from == trade2.t2.to
                                        {
                                            arb = Some(trade2);
                                            res = true
                                        }
                                    }
                                    _ => (),
                                }
                            }

                            res
                        });

                        if let Some(index) = index {
                            let t1amount = trade1.t1.amount.clone();
                            if i > 0 {
                                if let Some(ref action) =
                                    actions.get(i - 1).map(|x| x.to_action()).flatten()
                                {
                                    if action.deposit().is_some() {
                                        actions[i - 1] = Classification::Prune;
                                    }
                                }
                            }

                            // replace with an arb classification
                            let arb = arb.unwrap();
                            actions[i] = Classification::new(
                                Arbitrage {
                                    profit: arb.t2.amount - t1amount,
                                    token: arb.t2.token,
                                    to: arb.t2.to,
                                },
                                // TODO!
                                Vec::new(),
                            );

                            // prune the rest in-place
                            for k in i + 1..=index {
                                actions[k] = Classification::Prune;
                            }

                            if let Some(action) =
                                actions.get(index + 2).map(|x| x.to_action()).flatten()
                            {
                                if action.withdrawal().is_some() {
                                    actions[index + 2] = Classification::Prune;
                                }
                            }
                        }
                    }
                    _ => (),
                }
            }
        }

        prune(inspection);
    }
}
