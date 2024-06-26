// use crate::types::{address, token, role};
use thiserror::Error;


/// Error.
#[derive(Debug, Error)]
pub enum Error {
    #[error("unexpected vote/proposal value")]
    UnexpectedValue,
    #[error("unexpected vote/proposal type")]
    UnexpectedType,

    #[error("Overflow while Proposal vote")]
    Overflow,

    #[error("malformed value: {0}")]
    MalformedValue(anyhow::Error),

}



#[derive(Clone, Debug, PartialEq)]
pub enum ProposalState {
    Active,
    Passed,
    Rejected,
    Expired,
    Cancelled,
}

impl ProposalState {
    const PROPOSAL_STATE_SIZE: usize = 1;

    fn marshal_binary(&self) -> [u8; Self::PROPOSAL_STATE_SIZE] {
        let mut data = [0u8; Self::PROPOSAL_STATE_SIZE];
        match self {
            ProposalState::Active => data[0] = 0,
            ProposalState::Passed => data[0] = 1,
            ProposalState::Rejected => data[0] = 2,
            ProposalState::Expired => data[0] = 3,
            ProposalState::Cancelled => data[0] = 4,
        }
        data
    }

    pub fn to_string(&self) -> String {
        match self {
            ProposalState::Active => String::from("Active"),
            ProposalState::Passed => String::from("Passed"),
            ProposalState::Rejected => String::from("Rejected"),
            ProposalState::Expired => String::from("Expired"),
            ProposalState::Cancelled => String::from("Cancelled"),
        }
    }
}


impl Default for ProposalState {
    fn default() -> Self {
        // Choose a reasonable default variant for your use case
        ProposalState::Active
    }
}



impl cbor::Encode for ProposalState {
    fn into_cbor_value(self) -> cbor::Value {
        cbor::Value::ByteString(self.marshal_binary().to_vec())
    }
}

impl cbor::Decode for ProposalState {

    fn try_from_cbor_value(value: cbor::Value) -> Result<Self, cbor::DecodeError> {
        match value {
            cbor::Value::ByteString(bytes) if bytes.len() == ProposalState::PROPOSAL_STATE_SIZE => {
                match bytes[0] {
                    0 => Ok(ProposalState::Active),
                    1 => Ok(ProposalState::Passed),
                    2 => Ok(ProposalState::Rejected),
                    3 => Ok(ProposalState::Expired),
                    4 => Ok(ProposalState::Cancelled),
                    _ => Err(cbor::DecodeError::UnexpectedType),
                }
            }
            _ => Err(cbor::DecodeError::UnexpectedType),
        }

    }
}


/// Maximum length of a Meta data, maybe some transaction sequence no for mint/burn, 
pub const MAX_META: usize = 64;


#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Meta([u8; MAX_META]);

impl Default for Meta {
    fn default() -> Self {
        Meta([0; MAX_META])
    }
}


impl cbor::Encode for Meta {
    fn into_cbor_value(self) -> cbor::Value {
        cbor::Value::ByteString(self.0.to_vec())
    }
}

impl cbor::Decode for Meta {
    fn try_from_cbor_value(value: cbor::Value) -> Result<Self, cbor::DecodeError> {
        match value {
            cbor::Value::ByteString(bytes) => {
                if bytes.len() > MAX_META {
                    return Err(cbor::DecodeError::UnexpectedType);
                }
                let mut buf = [0u8; MAX_META];
                buf.copy_from_slice(&bytes);
                Ok(Self(buf))
            }
            _ => Err(cbor::DecodeError::UnexpectedType),
        }
    }
}


