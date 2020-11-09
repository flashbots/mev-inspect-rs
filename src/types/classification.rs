use crate::{addresses::lookup, is_subtrace, types::actions::SpecificAction};
use ethers::types::Call;
use rustc_hex::ToHex;
use std::fmt;

#[derive(Clone, PartialEq)]
pub enum Classification {
    Known(ActionTrace),
    Unknown(CallTrace),
    Prune,
}

#[derive(Debug, Clone, PartialOrd, PartialEq)]
pub struct ActionTrace {
    pub action: SpecificAction,
    pub trace_address: Vec<usize>,
}

impl AsRef<SpecificAction> for ActionTrace {
    fn as_ref(&self) -> &SpecificAction {
        &self.action
    }
}

impl From<ActionTrace> for Classification {
    fn from(action: ActionTrace) -> Self {
        Classification::Known(action)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallTrace {
    pub call: Call,
    pub trace_address: Vec<usize>,
}

impl AsRef<Call> for CallTrace {
    fn as_ref(&self) -> &Call {
        &self.call
    }
}

impl From<CallTrace> for Classification {
    fn from(call: CallTrace) -> Self {
        Classification::Unknown(call)
    }
}

impl Classification {
    pub fn new<T: Into<SpecificAction>>(action: T, trace_address: Vec<usize>) -> Self {
        Classification::Known(ActionTrace {
            action: action.into(),
            trace_address,
        })
    }

    /// Gets the trace address in this call (Empty if Prune)
    pub fn trace_address(&self) -> Vec<usize> {
        match &self {
            Classification::Known(inner) => inner.trace_address.clone(),
            Classification::Unknown(inner) => inner.trace_address.clone(),
            Classification::Prune => vec![],
        }
    }

    pub fn prune_subcalls(&self, classifications: &mut [Classification]) {
        let t1 = self.trace_address();

        for c in classifications.iter_mut() {
            let t2 = c.trace_address();
            if t2 == t1 {
                continue;
            }

            if is_subtrace(&t1, &t2) {
                *c = Classification::Prune;
            }
        }
    }

    pub fn subcalls(&self, classifications: &[Classification]) -> Vec<Classification> {
        let t1 = self.trace_address();

        let mut v = Vec::new();
        for c in classifications.iter() {
            let t2 = c.trace_address();

            if is_subtrace(&t1, &t2) {
                v.push(c.clone());
            }
        }
        v
    }

    pub fn to_action(&self) -> Option<&SpecificAction> {
        match self {
            Classification::Known(ref inner) => Some(&inner.action),
            _ => None,
        }
    }
}

impl fmt::Debug for Classification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self {
            Classification::Known(action) => write!(f, "{:#?}", action),
            Classification::Unknown(CallTrace {
                call,
                trace_address,
            }) => f
                .debug_struct("TraceCall")
                .field("from", &lookup(call.from))
                .field("to", &lookup(call.to))
                .field("value", &call.value)
                .field("gas", &call.gas)
                .field("input", &call.input.as_ref().to_hex::<String>())
                .field("call_type", &call.call_type)
                .field("trace", trace_address)
                .finish(),
            Classification::Prune => f.debug_tuple("Pruned").finish(),
        }
    }
}
