use crate::types::TransactionData;
use crate::{
    inspectors::find_matching,
    types::{actions::Trade, Classification, Inspection},
    Reducer, TxReducer,
};
use itertools::Itertools;
use std::collections::HashSet;

pub struct TradeReducer;

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
                    if let Some(transfer) = action.as_action().map(|x| x.as_transfer()).flatten() {
                        transfer
                    } else {
                        return;
                    };

                // find the first transfer after it
                let res = find_matching(
                    actions.iter().enumerate().skip(i + 1),
                    |t| t.as_transfer(),
                    |t| t.to == transfer.from && t.from == transfer.to && t.token != transfer.token,
                    true,
                );

                if let Some((j, transfer2)) = res {
                    // only match transfers which were on the same rank of the trace
                    // trades across multiple trace levels are handled by their individual
                    // inspectors
                    if actions[i].trace_address().len() != actions[j].trace_address().len() {
                        return;
                    }
                    *action = Classification::new(
                        Trade {
                            t1: transfer.clone(),
                            t2: transfer2.clone(),
                        },
                        actions[i].trace_address(),
                    );

                    // If there is no follow-up transfer that uses `transfer2`, prune it:
                    let res = find_matching(
                        actions.iter().enumerate().skip(j + 1),
                        |t| t.as_transfer(),
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

impl TxReducer for TradeReducer {
    fn reduce_tx(&self, tx: &mut TransactionData) {
        let mut trades = Vec::new();
        let mut prune = HashSet::new();

        let actions: Vec<_> = tx.actions().enumerate().collect();

        for (idx, action, transfer, call) in actions
            .iter()
            .filter_map(|(idx, action)| {
                action
                    .as_transfer()
                    .map(|transfer| (*idx, action, transfer))
            })
            .filter_map(|(idx, action, transfer)| {
                tx.get_call(&action.call)
                    .map(|call| (idx, action, transfer, call))
            })
        {
            // handle uniswap transfers/swaps differently, where we're only interested in continuous swaps
            if call.protocol.map(|p| p.is_uniswap()).unwrap_or_default() {
                if call.classification.is_swap() {
                    // find the transfer prior to this call that forms a trade
                    if let Some((i, t1)) = actions
                        .iter()
                        .rev()
                        .skip(actions.len() - idx)
                        .filter_map(|(i, a)| a.as_transfer().map(|t| (i, t)))
                        .next()
                    {
                        // we make the previous transfer a trade and remove the current
                        prune.remove(i);
                        prune.insert(idx);
                        trades.push((
                            *i,
                            Trade {
                                t1: t1.clone(),
                                t2: transfer.clone(),
                            },
                        ));
                        continue;
                    }
                }
            }

            // find the first transfer after it
            if let Some((transfer2_idx, action2, transfer2)) = tx
                .actions()
                .enumerate()
                .skip(idx + 1)
                .filter_map(|(i, a)| {
                    a.as_transfer()
                        .filter(|t| {
                            t.to == transfer.from
                                && t.from == transfer.to
                                && t.token != transfer.token
                        })
                        .map(|t| (i, a, t))
                })
                .next()
            {
                // only match transfers which were on the same rank of the trace
                // trades across multiple trace levels are handled by their individual
                // inspectors
                if action.call.len() != action2.call.len() {
                    continue;
                }
                trades.push((
                    idx,
                    Trade {
                        t1: transfer.clone(),
                        t2: transfer2.clone(),
                    },
                ));

                // If there is no follow-up transfer that uses `transfer2`, prune it:
                if let Some((_, action)) = tx.actions().enumerate().skip(transfer2_idx + 1).next() {
                    if action
                        .as_transfer()
                        .filter(|t| t.to == transfer2.from && t.from == transfer2.to)
                        .is_some()
                    {
                        continue;
                    }
                }
                prune.insert(transfer2_idx);
            }
        }
        // replace with profitable liquidation
        for (idx, trade) in trades {
            if let Some(action) = tx.get_action_mut(idx) {
                action.inner = trade.into();
            }
        }

        // loop over all actions to prune starting with the highest index
        for idx in prune.into_iter().sorted_by(|a, b| b.cmp(a)) {
            tx.remove_action(idx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;
    use crate::types::actions::Transfer;

    fn test_transfer_to_trade(input: Vec<Classification>, expected: Vec<Classification>) {
        let uniswap = TradeReducer;
        let mut inspection = mk_inspection(input);
        uniswap.reduce(&mut inspection);
        assert_eq!(inspection.actions, expected);
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
