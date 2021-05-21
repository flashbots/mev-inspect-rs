use crate::types::TransactionData;
use crate::{
    inspectors::find_matching,
    types::{actions::Trade, Classification, Inspection},
    Reducer, TxReducer,
};

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
        let mut prune = Vec::new();

        for (idx, action, transfer) in tx.actions().enumerate().filter_map(|(idx, action)| {
            action
                .inner
                .as_transfer()
                .map(|transfer| (idx, action, transfer))
        }) {
            // find the first transfer after it
            if let Some((transfer2_idx, action2, transfer2)) = tx
                .actions()
                .enumerate()
                .skip(idx + 1)
                .filter_map(|(i, a)| {
                    action
                        .inner
                        .as_transfer()
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
                        .inner
                        .as_transfer()
                        .filter(|t| t.to == transfer2.from && t.from == transfer2.to)
                        .is_some()
                    {
                        continue;
                    }
                }
                prune.push(transfer2_idx);
            }
        }
        // replace with profitable liquidation
        for (idx, trade) in trades {
            if let Some(action) = tx.get_action_mut(idx) {
                action.inner = trade.into();
            }
        }

        for idx in prune {
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
