#![cfg_attr(not(any(feature = "sha-1", feature = "sha-2")), allow(unused))]

use std::fmt::{self, Debug, Formatter};

use hmac::digest::block_buffer;
use hmac::digest::core_api::{
    AlgorithmName, BufferKindUser, CoreProxy, FixedOutputCore, UpdateCore,
};
use hmac::digest::crypto_common::BlockSizeUser;
use hmac::digest::typenum;
use hmac::digest::{self, HashMarker, OutputSizeUser};
use hmac::{Hmac, Mac};
#[cfg(feature = "sha-1")]
use sha1::Sha1;
#[cfg(feature = "sha-2")]
use sha2::{Sha256, Sha384, Sha512};

/// Serialized `X-Hub-Signature` header value.
pub enum Signature {
    #[cfg(feature = "sha-1")]
    Sha1(digest::Output<Sha1>),
    #[cfg(feature = "sha-2")]
    Sha256(digest::Output<Sha256>),
    #[cfg(feature = "sha-2")]
    Sha384(digest::Output<Sha384>),
    #[cfg(feature = "sha-2")]
    Sha512(digest::Output<Sha512>),
}

/// A pair of `Signature` and its associated `Hmac` instance, used to verify the given signature.
#[derive(Debug)]
pub enum Verifier {
    #[cfg(feature = "sha-1")]
    Sha1(GenericVerifier<Sha1>),
    #[cfg(feature = "sha-2")]
    Sha256(GenericVerifier<Sha256>),
    #[cfg(feature = "sha-2")]
    Sha384(GenericVerifier<Sha384>),
    #[cfg(feature = "sha-2")]
    Sha512(GenericVerifier<Sha512>),
}

pub struct GenericVerifier<D: CoreProxy>
where
    D::Core: HashMarker
        + UpdateCore
        + FixedOutputCore
        + BufferKindUser<BufferKind = block_buffer::Eager>
        + Default
        + Clone,
    <D::Core as BlockSizeUser>::BlockSize: typenum::IsLess<typenum::U256>,
    typenum::Le<<D::Core as BlockSizeUser>::BlockSize, typenum::U256>: typenum::NonZero,
{
    signature: digest::Output<Hmac<D>>,
    mac: Hmac<D>,
}

/// Error while serializing a `Signature`.
pub enum SerializeError<'a> {
    Parse,
    UnknownMethod(&'a [u8]),
}

/// Error verifying a signature sent by a hub, indicating that the distributed content has been
/// falsified.
///
/// If you encounter this error, you must not trust the content.
#[derive(thiserror::Error)]
#[error("the hub has sent an invalid signature")]
pub struct SignatureMismatch {
    _priv: (),
}

impl Signature {
    pub fn parse(header_value: &[u8]) -> Result<Self, SerializeError<'_>> {
        let pos = memchr::memchr(b'=', header_value);
        let (method, hex) = if let Some(i) = pos {
            let (method, hex) = header_value.split_at(i);
            (method, &hex[1..])
        } else {
            return Err(SerializeError::Parse);
        };

        fn decode_hex<T: OutputSizeUser>(
            hex: &[u8],
        ) -> Result<digest::Output<T>, SerializeError<'_>> {
            let mut ret: digest::Output<T> = Default::default();
            if hex::decode_to_slice(hex, &mut ret).is_err() {
                return Err(SerializeError::Parse);
            }
            Ok(ret)
        }

        match method {
            #[cfg(feature = "sha-1")]
            b"sha1" => decode_hex::<Sha1>(hex).map(Signature::Sha1),
            #[cfg(feature = "sha-2")]
            b"sha256" => decode_hex::<Sha256>(hex).map(Signature::Sha256),
            #[cfg(feature = "sha-2")]
            b"sha384" => decode_hex::<Sha384>(hex).map(Signature::Sha384),
            #[cfg(feature = "sha-2")]
            b"sha512" => decode_hex::<Sha512>(hex).map(Signature::Sha512),
            _ => Err(SerializeError::UnknownMethod(method)),
        }
    }

    pub fn is_preferable_to(&self, other: &Self) -> bool {
        self.discriminant() >= other.discriminant()
    }

    fn discriminant(&self) -> u8 {
        match *self {
            #[cfg(feature = "sha-1")]
            Signature::Sha1(_) => 0,
            #[cfg(feature = "sha-2")]
            Signature::Sha256(_) => 1,
            #[cfg(feature = "sha-2")]
            Signature::Sha384(_) => 2,
            #[cfg(feature = "sha-2")]
            Signature::Sha512(_) => 3,
        }
    }

    pub fn verify_with(self, secret: &[u8]) -> Verifier {
        match self {
            #[cfg(feature = "sha-1")]
            Signature::Sha1(signature) => Verifier::Sha1(GenericVerifier::new(signature, secret)),
            #[cfg(feature = "sha-2")]
            Signature::Sha256(signature) => {
                Verifier::Sha256(GenericVerifier::new(signature, secret))
            }
            #[cfg(feature = "sha-2")]
            Signature::Sha384(signature) => {
                Verifier::Sha384(GenericVerifier::new(signature, secret))
            }
            #[cfg(feature = "sha-2")]
            Signature::Sha512(signature) => {
                Verifier::Sha512(GenericVerifier::new(signature, secret))
            }
        }
    }
}

impl Verifier {
    pub fn update(&mut self, bytes: &[u8]) {
        match *self {
            #[cfg(feature = "sha-1")]
            Verifier::Sha1(ref mut inner) => inner.mac.update(bytes),
            #[cfg(feature = "sha-2")]
            Verifier::Sha256(ref mut inner) => inner.mac.update(bytes),
            #[cfg(feature = "sha-2")]
            Verifier::Sha384(ref mut inner) => inner.mac.update(bytes),
            #[cfg(feature = "sha-2")]
            Verifier::Sha512(ref mut inner) => inner.mac.update(bytes),
        }
    }

    pub fn verify_reset(&mut self) -> std::result::Result<(), SignatureMismatch> {
        match *self {
            #[cfg(feature = "sha-1")]
            Verifier::Sha1(ref mut inner) => inner.verify_reset(),
            #[cfg(feature = "sha-2")]
            Verifier::Sha256(ref mut inner) => inner.verify_reset(),
            #[cfg(feature = "sha-2")]
            Verifier::Sha384(ref mut inner) => inner.verify_reset(),
            #[cfg(feature = "sha-2")]
            Verifier::Sha512(ref mut inner) => inner.verify_reset(),
        }
    }
}

impl Debug for SignatureMismatch {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignatureMismatch").finish()
    }
}

impl<D: CoreProxy> Debug for GenericVerifier<D>
where
    D::Core: AlgorithmName
        + HashMarker
        + UpdateCore
        + FixedOutputCore
        + BufferKindUser<BufferKind = block_buffer::Eager>
        + Default
        + Clone,
    <D::Core as BlockSizeUser>::BlockSize: typenum::IsLess<typenum::U256>,
    typenum::Le<<D::Core as BlockSizeUser>::BlockSize, typenum::U256>: typenum::NonZero,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let GenericVerifier {
            ref signature,
            ref mac,
        } = *self;
        f.debug_struct("GenericVerifier")
            .field("signature", signature)
            .field("mac", mac)
            .finish()
    }
}

impl<D: CoreProxy> GenericVerifier<D>
where
    D::Core: HashMarker
        + UpdateCore
        + FixedOutputCore
        + BufferKindUser<BufferKind = block_buffer::Eager>
        + Default
        + Clone,
    <D::Core as BlockSizeUser>::BlockSize: typenum::IsLess<typenum::U256>,
    typenum::Le<<D::Core as BlockSizeUser>::BlockSize, typenum::U256>: typenum::NonZero,
{
    pub fn new(signature: digest::Output<D::Core>, secret: &[u8]) -> Self {
        GenericVerifier {
            signature,
            mac: Hmac::new_from_slice(secret).unwrap(),
        }
    }

    pub fn verify_reset(&mut self) -> Result<(), SignatureMismatch> {
        let code = self.mac.finalize_reset().into_bytes();
        if *code == *self.signature {
            Ok(())
        } else {
            Err(SignatureMismatch { _priv: () })
        }
    }
}
