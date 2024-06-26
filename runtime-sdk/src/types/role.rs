//sifei: added for implementation tryfrom
use std::{convert::TryFrom};
use thiserror::Error;
use strum_macros::EnumIter;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, EnumIter)]
pub enum Role {
    // GB: WARNING!!!, the roles sequence matters, please have an attention while adding new roles.

    // GB: Admin propose all the roles and vote all the roles.
    Admin,

    // GB: Proposers propose some actions only.
    // GB: Voters vote some actions only.
    MintProposer,
    BurnProposer,
    WhitelistProposer,
    BlacklistProposer,

    MintVoter,
    BurnVoter,
    WhitelistVoter,
    BlacklistVoter,

    WhitelistedUser,
    BlacklistedUser,

    User,
}

///Sifei: Error.
#[derive(Error, Debug)]
pub enum Error {
    #[error("malformed role")]
    MalformedRole,
}


impl Role {
    // GB: this size is the roles bytes allowed, however, the roles are within the 8 bits
    // which is in 255 roles.
    //Sifei: change to pub
    pub const ROLE_SIZE: usize = 1;

    //Sifei: change to pub
    pub fn marshal_binary(&self) -> [u8; Self::ROLE_SIZE] {
        let mut data = [0u8; Self::ROLE_SIZE];
        match self {
            Role::Admin => data[0] = 0,
            Role::MintProposer => data[0] = 1,
            Role::MintVoter => data[0] = 2,
            Role::BurnProposer => data[0] = 3,
            Role::BurnVoter => data[0] = 4,
            Role::WhitelistProposer => data[0] = 5,
            Role::WhitelistVoter => data[0] = 6,
            Role::BlacklistProposer => data[0] = 7,
            Role::BlacklistVoter => data[0] = 8,
            Role::WhitelistedUser => data[0] = 9,
            Role::BlacklistedUser => data[0] = 10,
            Role::User => data[0] = 11,
        }
        data
    }

    pub fn to_string(&self) -> String {
        match self {
            Role::Admin => String::from("Admin"),
            Role::MintProposer => String::from("MintProposer"),
            Role::MintVoter => String::from("MintVoter"),
            Role::BurnProposer => String::from("BurnProposer"),
            Role::BurnVoter => String::from("BurnVoter"),
            Role::WhitelistProposer => String::from("WhitelistProposer"),
            Role::WhitelistVoter => String::from("WhitelistVoter"),
            Role::BlacklistProposer => String::from("BlacklistProposer"),
            Role::BlacklistVoter => String::from("BlacklistVoter"),
            Role::WhitelistedUser => String::from("WhitelistedUser"),
            Role::BlacklistedUser => String::from("BlacklistedUser"),
            Role::User => String::from("User"),
        }
    }

    ///Sifei: Tries to create a new role from raw bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, Error> {
        if data.len() != Self::ROLE_SIZE {
            return Err(Error::MalformedRole);
        }

        let mut r = [0; Self::ROLE_SIZE];
        r.copy_from_slice(data);

        let role = match r[0] {
            0 => Ok(Role::Admin),
            1 => Ok(Role::MintProposer),
            2 => Ok(Role::MintVoter),
            3 => Ok(Role::BurnProposer),
            4 => Ok(Role::BurnVoter),
            5 => Ok(Role::WhitelistProposer),
            6 => Ok(Role::WhitelistVoter),
            7 => Ok(Role::BlacklistProposer),
            8 => Ok(Role::BlacklistVoter),
            9 => Ok(Role::WhitelistedUser),
            10 => Ok(Role::BlacklistedUser),
            11 => Ok(Role::User),
            _ => Err(Error::MalformedRole),
        };
        role
    }

}



//Sifei: added for role
impl TryFrom<&[u8]> for Role {
    type Error =  Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        Self::from_bytes(bytes)
    }
}

impl Default for Role {
    fn default() -> Self {
        Role::User
    }
}

impl cbor::Encode for Role {
    fn into_cbor_value(self) -> cbor::Value {
        cbor::Value::ByteString(self.marshal_binary().to_vec())
    }
}

impl cbor::Decode for Role {
    fn try_from_cbor_value(value: cbor::Value) -> Result<Self, cbor::DecodeError> {
        match value {
            cbor::Value::ByteString(bytes) if bytes.len() == Role::ROLE_SIZE => {
                match bytes[0] {
                    0 => Ok(Role::Admin),
                    1 => Ok(Role::MintProposer),
                    2 => Ok(Role::MintVoter),
                    3 => Ok(Role::BurnProposer),
                    4 => Ok(Role::BurnVoter),
                    5 => Ok(Role::WhitelistProposer),
                    6 => Ok(Role::WhitelistVoter),
                    7 => Ok(Role::BlacklistProposer),
                    8 => Ok(Role::BlacklistVoter),
                    9 => Ok(Role::WhitelistedUser),
                    10 => Ok(Role::BlacklistedUser),
                    11 => Ok(Role::User),
                    _ => Err(cbor::DecodeError::UnexpectedType),
                }
            }
            _ => Err(cbor::DecodeError::UnexpectedType),
        }
    }
}


