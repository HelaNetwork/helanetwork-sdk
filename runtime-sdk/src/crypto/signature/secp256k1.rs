//! Secp256k1 signatures.
use digest::{consts::U32, Digest, FixedOutput};

use k256::{
    self,
    ecdsa::{
        self,
        signature::{DigestSigner as _, DigestVerifier, Signer as _, Verifier as _},
    },
    elliptic_curve::sec1::{FromEncodedPoint, ToEncodedPoint},
    sha2::Sha512_256,
};

use crate::crypto::signature::{Error, Signature};

use once_cell::sync::Lazy;
use std::{num::NonZeroUsize, sync::Mutex};

static PK_CACHE: Lazy<Mutex<lru::LruCache<Vec<u8>, Vec<u8>>>> =
    Lazy::new(|| Mutex::new(lru::LruCache::new(NonZeroUsize::new(100000).unwrap())));

/// A Secp256k1 public key (in compressed form).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicKey(k256::EncodedPoint);

impl PublicKey {
    /// Return a byte representation of this public key.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// Return an alternative byte representation used in deriving Ethereum-compatible addresses. Cached version.
    pub fn to_uncompressed_untagged_bytes(&self) -> Vec<u8> {
        let key = self.as_bytes().to_vec();

        let cached = {
            let mut cache = PK_CACHE.lock().unwrap();
            let val = cache.get(&key);
            if val.is_some() {
                Some(val.unwrap().clone())
            } else {
                None
            }
        };

        if cached.is_some() {
            return cached.unwrap();
        }

        // If it does not exist in the cache, create a new public key and store it in the cache
        let pk = k256::PublicKey::from_encoded_point(&self.0).unwrap();
        let val = pk.to_encoded_point(false).as_bytes()[1..].to_vec();

        PK_CACHE.lock().unwrap().put(key, val.clone());

        val
    }

    /// Construct a public key from a slice of bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != 33 {
            return Err(Error::MalformedPublicKey);
        }
        let ep = k256::EncodedPoint::from_bytes(bytes).map_err(|_| Error::MalformedPublicKey)?;
        if !ep.is_compressed() {
            // This should never happen due to the size check above.
            return Err(Error::MalformedPublicKey);
        }
        Ok(PublicKey(ep))
    }

    /// Verify a signature.
    pub fn verify(
        &self,
        context: &[u8],
        message: &[u8],
        signature: &Signature,
    ) -> Result<(), Error> {
        // Note that we must use Sha512_256 instead of our Hash here,
        // even though it's the same thing, because it implements the Digest
        // trait, so we can use verify_digest() below, which doesn't pre-hash
        // the data (verify() does).
        let mut digest = Sha512_256::new();
        for byte in &[context, message] {
            <Sha512_256 as Digest>::update(&mut digest, byte);
        }
        let sig = ecdsa::Signature::from_der(signature.0.as_ref())
            .map_err(|_| Error::MalformedSignature)?;
        let verify_key = ecdsa::VerifyingKey::from_encoded_point(&self.0)
            .map_err(|_| Error::MalformedPublicKey)?;

        verify_key
            .verify_digest(digest, &sig)
            .map_err(|_| Error::VerificationFailed)
    }

    /// Verify signature without using any domain separation scheme.
    pub fn verify_raw(&self, message: &[u8], signature: &Signature) -> Result<(), Error> {
        let sig = ecdsa::Signature::from_der(signature.0.as_ref())
            .map_err(|_| Error::MalformedSignature)?;
        let verify_key = ecdsa::VerifyingKey::from_encoded_point(&self.0)
            .map_err(|_| Error::MalformedPublicKey)?;
        verify_key
            .verify(message, &sig)
            .map_err(|_| Error::VerificationFailed)
    }

    /// Verify signature of a pre-hashed message.
    pub fn verify_digest<D>(&self, digest: D, signature: &Signature) -> Result<(), Error>
    where
        D: Digest + FixedOutput<OutputSize = U32>,
    {
        let sig = ecdsa::Signature::from_der(signature.as_ref())
            .map_err(|_| Error::MalformedSignature)?;
        let verify_key = ecdsa::VerifyingKey::from_encoded_point(&self.0)
            .map_err(|_| Error::MalformedPublicKey)?;
        verify_key
            .verify_digest(digest, &sig)
            .map_err(|_| Error::VerificationFailed)
    }
}

impl From<&'static str> for PublicKey {
    fn from(s: &'static str) -> PublicKey {
        PublicKey::from_bytes(&base64::decode(s).unwrap()).unwrap()
    }
}

impl cbor::Encode for PublicKey {
    fn into_cbor_value(self) -> cbor::Value {
        cbor::Value::ByteString(self.as_bytes().to_vec())
    }
}

impl cbor::Decode for PublicKey {
    fn try_from_cbor_value(value: cbor::Value) -> Result<Self, cbor::DecodeError> {
        match value {
            cbor::Value::ByteString(data) => {
                Self::from_bytes(&data).map_err(|_| cbor::DecodeError::UnexpectedType)
            }
            _ => Err(cbor::DecodeError::UnexpectedType),
        }
    }
}

/// A memory-backed signer for Secp256k1.
pub struct MemorySigner {
    sk: ecdsa::SigningKey,
}

impl MemorySigner {
    pub fn sign_digest<D>(&self, digest: D) -> Result<Signature, Error>
    where
        D: Digest + FixedOutput<OutputSize = U32>,
    {
        let signature: ecdsa::Signature = self.sk.sign_digest(digest);
        Ok(signature.to_der().as_bytes().to_vec().into())
    }
}

impl super::Signer for MemorySigner {
    fn new_from_seed(seed: &[u8]) -> Result<Self, Error> {
        let sk = ecdsa::SigningKey::from_slice(seed).map_err(|_| Error::InvalidArgument)?;
        Ok(Self { sk })
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        Ok(Self {
            sk: ecdsa::SigningKey::from_slice(bytes).map_err(|_| Error::MalformedPrivateKey)?,
        })
    }

    fn to_bytes(&self) -> Vec<u8> {
        self.sk.to_bytes().to_vec()
    }

    fn public_key(&self) -> super::PublicKey {
        super::PublicKey::Secp256k1(PublicKey(self.sk.verifying_key().to_encoded_point(true)))
    }

    fn sign(&self, context: &[u8], message: &[u8]) -> Result<Signature, Error> {
        let mut digest = Sha512_256::new();
        <Sha512_256 as Digest>::update(&mut digest, context);
        <Sha512_256 as Digest>::update(&mut digest, message);
        let signature: ecdsa::Signature = self.sk.sign_digest(digest);
        Ok(signature.to_der().as_bytes().to_vec().into())
    }

    fn sign_raw(&self, message: &[u8]) -> Result<Signature, Error> {
        let signature: ecdsa::Signature = self.sk.sign(message);
        Ok(signature.to_der().as_bytes().to_vec().into())
    }
}
