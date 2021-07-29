use crate::model::{CallClassification, EventLog, InternalCall};
use crate::types::{Action, Protocol, TransactionData};
use crate::{
    addresses::{ETH, WETH},
    inspect_tx,
    types::{
        actions::{Deposit, SpecificAction, Transfer, Withdrawal},
        Classification, Inspection,
    },
    DefiProtocol, Inspector, ProtocolContracts,
};
use ethers::{
    contract::{abigen, BaseContract, EthLogDecode},
    types::{Address, Call as TraceCall, CallType, U256},
};

abigen!(
    Erc20Contract,
    r#"[
                function transferFrom(address, address, uint256)
                function transfer(address, uint256)
                function deposit()
                function withdraw(uint256)
                function mint(address, uint256)
                function burnFrom(address, uint256)
                event Transfer(address indexed _from, address indexed _to, uint256 _value)
                event Approval(address indexed _owner, address indexed _spender, uint256 _value)
            ]"#,
    event_derives(serde::Deserialize, serde::Serialize)
);

#[derive(Debug, Clone)]
/// Decodes ERC20 calls
pub struct ERC20(BaseContract);

impl Inspector for ERC20 {
    fn inspect(&self, inspection: &mut Inspection) {
        inspection.actions.iter_mut().for_each(|classification| {
            if let Some(calltrace) = classification.as_call() {
                if let Some(transfer) = self.try_parse(calltrace.as_ref()) {
                    *classification = Classification::new(transfer, calltrace.trace_address.clone())
                }
            }
        })
    }
}

impl DefiProtocol for ERC20 {
    fn base_contracts(&self) -> ProtocolContracts {
        ProtocolContracts::Single(&self.0)
    }

    fn protocol(&self) -> Protocol {
        Protocol::Erc20
    }

    fn is_protocol_event(&self, log: &EventLog) -> bool {
        Erc20ContractEvents::decode_log(&log.raw_log).is_ok()
    }

    fn decode_call_action(&self, call: &InternalCall, tx: &TransactionData) -> Option<Action> {
        match call.classification {
            CallClassification::Transfer => {
                if let Some((_, log, transfer)) = tx
                    .call_logs_decoded::<TransferFilter>(&call.trace_address)
                    .next()
                {
                    let action = Transfer {
                        from: transfer.from,
                        to: transfer.to,
                        amount: transfer.value,
                        token: log.address,
                    };
                    return Some(Action::with_logs(
                        action.into(),
                        call.trace_address.clone(),
                        vec![log.log_index],
                    ));
                }
            }
            _ => {}
        }
        None
    }

    #[allow(clippy::if_same_then_else)]
    fn classify(
        &self,
        call: &InternalCall,
    ) -> Option<(CallClassification, Option<SpecificAction>)> {
        if self
            .0
            .decode::<(Address, Address, U256), _>("transferFrom", &call.input)
            .is_ok()
        {
            Some((CallClassification::Transfer, None))
        } else if self
            .0
            .decode::<(Address, U256), _>("burnFrom", &call.input)
            .is_ok()
        {
            // emits a transfer event that will be caught `decode_call_action`
            Some((CallClassification::Transfer, None))
        } else if self
            .0
            .decode::<(Address, U256), _>("mint", &call.input)
            .is_ok()
        {
            // emits a transfer event that will be caught `decode_call_action`
            Some((CallClassification::Transfer, None))
        } else if self
            .0
            .decode::<(Address, U256), _>("transfer", &call.input)
            .is_ok()
        {
            Some((CallClassification::Transfer, None))
        } else if let Ok(amount) = self.0.decode::<U256, _>("withdraw", &call.input) {
            Some((
                CallClassification::Withdrawal,
                Some(SpecificAction::WethWithdrawal(Withdrawal {
                    to: call.from,
                    amount,
                })),
            ))
        } else if call.input.starts_with(&ethers::utils::id("deposit()")) {
            Some((
                CallClassification::Deposit,
                Some(SpecificAction::WethDeposit(Deposit {
                    from: call.from,
                    amount: call.value,
                })),
            ))
        } else if call.value > 0.into() && call.from != *WETH {
            // ETH transfer
            Some((
                CallClassification::Transfer,
                Some(SpecificAction::Transfer(Transfer {
                    from: call.from,
                    to: call.to,
                    amount: call.value,
                    token: *ETH,
                })),
            ))
        } else {
            None
        }
    }

    fn inspect_tx(&self, tx: &mut TransactionData) {
        inspect_tx(self, tx);
        // get rid of erc20 duplicate events
        tx.remove_duplicate_transfers();
    }
}

impl ERC20 {
    pub fn new() -> Self {
        Self(BaseContract::from(ERC20CONTRACT_ABI.clone()))
    }

    /// Parse a Call trace to discover a token action
    pub fn try_parse(&self, trace_call: &TraceCall) -> Option<SpecificAction> {
        if trace_call.gas == 2300.into() {
            return None;
        }

        // do not parse delegatecall's
        if trace_call.call_type != CallType::Call {
            return None;
        }

        let token = trace_call.to;
        if let Ok((from, to, amount)) = self
            .0
            .decode::<(Address, Address, U256), _>("transferFrom", &trace_call.input)
        {
            Some(SpecificAction::Transfer(Transfer {
                from,
                to,
                amount,
                token,
            }))
        } else if let Ok((from, amount)) = self
            .0
            .decode::<(Address, U256), _>("burnFrom", &trace_call.input)
        {
            Some(SpecificAction::Transfer(Transfer {
                from,
                // Burns send to `0x0`
                to: Address::zero(),
                amount,
                token,
            }))
        } else if let Ok((to, amount)) = self
            .0
            .decode::<(Address, U256), _>("mint", &trace_call.input)
        {
            Some(SpecificAction::Transfer(Transfer {
                // Mints create from `0x0`
                from: Address::zero(),
                to,
                amount,
                token,
            }))
        } else if let Ok((to, amount)) = self
            .0
            .decode::<(Address, U256), _>("transfer", &trace_call.input)
        {
            Some(SpecificAction::Transfer(Transfer {
                from: trace_call.from,
                to,
                amount,
                token,
            }))
        } else if let Ok(amount) = self.0.decode::<U256, _>("withdraw", &trace_call.input) {
            Some(SpecificAction::WethWithdrawal(Withdrawal {
                to: trace_call.from,
                amount,
            }))
        } else if trace_call
            .input
            .as_ref()
            .starts_with(&ethers::utils::id("deposit()"))
        {
            Some(SpecificAction::WethDeposit(Deposit {
                from: trace_call.from,
                amount: trace_call.value,
            }))
        } else if trace_call.value > 0.into() && trace_call.from != *WETH {
            // ETH transfer
            Some(SpecificAction::Transfer(Transfer {
                from: trace_call.from,
                to: trace_call.to,
                amount: trace_call.value,
                token: *ETH,
            }))
        } else {
            None
        }
    }
}
