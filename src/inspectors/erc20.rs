use crate::{
    addresses::{ETH, WETH},
    types::actions::{Deposit, SpecificAction, Transfer, Withdrawal},
};
use ethers::{
    abi::parse_abi,
    contract::BaseContract,
    types::{Address, Call as TraceCall, CallType, U256},
};

#[derive(Debug, Clone)]
/// Decodes ERC20 calls
pub struct ERC20(BaseContract);

impl ERC20 {
    pub fn new() -> Self {
        Self(BaseContract::from(
            parse_abi(&[
                "function transferFrom(address, address, uint256)",
                "function transfer(address, uint256)",
                "function deposit()",
                "function withdraw(uint256)",
                "function mint(address, uint256)",
                "function burnFrom(address, uint256)",
            ])
            .expect("could not parse erc20 abi"),
        ))
    }

    /// Parse a Call trace to discover a token action
    pub fn parse(&self, trace_call: &TraceCall) -> Option<SpecificAction> {
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
