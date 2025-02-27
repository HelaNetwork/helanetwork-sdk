//! A minimal representation of an Oasis Runtime SDK address.
use std::convert::TryFrom;

use bech32::{self, FromBase32, ToBase32, Variant};
use thiserror::Error;

const ADDRESS_VERSION_SIZE: usize = 1;
const ADDRESS_DATA_SIZE: usize = 20;
const ADDRESS_SIZE: usize = ADDRESS_VERSION_SIZE + ADDRESS_DATA_SIZE;

// MZ, change it to hela
const ADDRESS_BECH32_HRP: &str = "hela0";
// const ADDRESS_BECH32_HRP: &str = "oasis";

/// Error.
#[derive(Error, Debug)]
pub enum Error {
    #[error("malformed address")]
    MalformedAddress,
}

/// An account address.
#[derive(
    Copy, Clone, Default, Debug, PartialEq, Eq, PartialOrd, Ord, cbor::Encode, cbor::Decode,
)]
#[cbor(transparent)]
pub struct Address([u8; ADDRESS_SIZE]);

impl Address {
    /// Size of an address in bytes.
    pub const SIZE: usize = ADDRESS_SIZE;

    /// Tries to create a new address from raw bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, Error> {
        if data.len() != ADDRESS_SIZE {
            return Err(Error::MalformedAddress);
        }

        let mut a = [0; ADDRESS_SIZE];
        a.copy_from_slice(data);

        Ok(Self(a))
    }

    /// Tries to create a new address from Bech32-encoded string.
    pub fn from_bech32(data: &str) -> Result<Self, Error> {
        let (hrp, data, variant) = bech32::decode(data).map_err(|_| Error::MalformedAddress)?;
        if hrp != ADDRESS_BECH32_HRP {
            return Err(Error::MalformedAddress);
        }
        if variant != Variant::Bech32 {
            return Err(Error::MalformedAddress);
        }
        let data: Vec<u8> = FromBase32::from_base32(&data).map_err(|_| Error::MalformedAddress)?;

        Address::from_bytes(&data)
    }

    /// Converts an address to Bech32 representation.
    pub fn to_bech32(self) -> String {
        bech32::encode(ADDRESS_BECH32_HRP, self.0.to_base32(), Variant::Bech32).unwrap()
    }
}

impl AsRef<[u8]> for Address {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl TryFrom<&[u8]> for Address {
    type Error = Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        Self::from_bytes(bytes)
    }
}

#[cfg(feature = "oasis-runtime-sdk")]
impl From<oasis_runtime_sdk::types::address::Address> for Address {
    fn from(a: oasis_runtime_sdk::types::address::Address) -> Self {
        Self(a.into_bytes())
    }
}

#[cfg(feature = "oasis-runtime-sdk")]
impl From<Address> for oasis_runtime_sdk::types::address::Address {
    fn from(a: Address) -> Self {
        oasis_runtime_sdk::types::address::Address::from_bytes(&a.0).unwrap()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_address_try_from_bytes() {
        let bytes_fixture = vec![42u8; ADDRESS_SIZE + 1];
        assert_eq!(
            Address::try_from(&bytes_fixture[0..ADDRESS_SIZE]).unwrap(),
            Address::from_bytes(&bytes_fixture[0..ADDRESS_SIZE]).unwrap()
        );
        assert!(matches!(
            Address::try_from(bytes_fixture.as_slice()).unwrap_err(),
            Error::MalformedAddress
        ));
    }

    #[test]
    fn test_address_from_bech32_invalid_hrp() {
        assert!(matches!(
            Address::from_bech32("sisoa1qpcprk8jxpsjxw9fadxvzrv9ln7td69yus8rmtux").unwrap_err(),
            Error::MalformedAddress,
        ));
    }

    #[test]
    fn test_address_from_bech32_invalid_variant() {
        let b = vec![42u8; ADDRESS_SIZE];
        let bech32_addr =
            bech32::encode(ADDRESS_BECH32_HRP, b.to_base32(), Variant::Bech32).unwrap();
        let bech32m_addr =
            bech32::encode(ADDRESS_BECH32_HRP, b.to_base32(), Variant::Bech32m).unwrap();

        assert!(
            Address::from_bech32(&bech32_addr).is_ok(),
            "bech32 address should be ok"
        );
        assert!(matches!(
            Address::from_bech32(&bech32m_addr).unwrap_err(),
            Error::MalformedAddress,
        ));
    }
}
