#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Vote {
    VoteYes,
    VoteNo,
    VoteAbstain,
}

impl Vote {
    fn to_u8(&self) -> u8 {
        match self {
            Vote::VoteYes => 0,
            Vote::VoteNo => 1,
            Vote::VoteAbstain => 2,
        }
    }
}

impl Default for Vote {
    fn default() -> Self {
        Vote::VoteAbstain
    }
}

impl cbor::Encode for Vote {
    fn into_cbor_value(self) -> cbor::Value {
        cbor::Value::Unsigned(self.to_u8() as u64)
    }
}


impl cbor::Decode for Vote {
    fn try_from_cbor_value(value: cbor::Value) -> Result<Self, cbor::DecodeError> {
        match value {
            cbor::Value::Unsigned(u) => {
                match u {
                    0 => Ok(Vote::VoteYes),
                    1 => Ok(Vote::VoteNo),
                    2 => Ok(Vote::VoteAbstain),
                    _ => Err(cbor::DecodeError::UnexpectedType),
                }
            }
            _ => Err(cbor::DecodeError::UnexpectedType),
        }
    }
}


#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    NoAction,
    SetRoles,
    Mint,
    Burn,
    Whitelist,
    Blacklist,
    Config,
}

impl Action {
    const ACT_SIZE: usize = 1;

    fn marshal_binary(&self) -> [u8; Self::ACT_SIZE] {
        match self {
            Action::NoAction => [0],
            Action::SetRoles => [1],
            Action::Mint => [2],
            Action::Burn => [3],
            Action::Whitelist => [4],
            Action::Blacklist => [5],
            Action::Config => [6],
        }
    }
}

impl Default for Action {
    fn default() -> Self {
        Action::NoAction
    }
}



impl cbor::Encode for Action {
    fn into_cbor_value(self) -> cbor::Value {
        cbor::Value::ByteString(self.marshal_binary().to_vec())
    }
}

impl cbor::Decode for Action {
    fn try_from_cbor_value(value: cbor::Value) -> Result<Self, cbor::DecodeError> {
        match value {
            cbor::Value::ByteString(bytes) if bytes.len() == Action::ACT_SIZE => {
                match bytes[0] {
                    0 => Ok(Action::NoAction),
                    1 => Ok(Action::SetRoles),
                    2 => Ok(Action::Mint),
                    3 => Ok(Action::Burn),
                    4 => Ok(Action::Whitelist),
                    5 => Ok(Action::Blacklist),
                    6 => Ok(Action::Config),
                    _ => Err(cbor::DecodeError::UnexpectedType),
                }
            }
            _ => Err(cbor::DecodeError::UnexpectedType),
        }
    }
}

