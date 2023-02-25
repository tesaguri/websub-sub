use std::fmt::{self, Debug, Formatter};

use hmac::digest::block_buffer;
use hmac::digest::core_api::{
    AlgorithmName, BufferKindUser, CoreProxy, FixedOutputCore, UpdateCore,
};
use hmac::digest::crypto_common::BlockSizeUser;
use hmac::digest::typenum;
use hmac::digest::{self, HashMarker, OutputSizeUser};
use hmac::{Hmac, Mac};
use sha1::Sha1;

/// Serialized `X-Hub-Signature` header value.
pub enum Signature {
    Sha1(digest::Output<Sha1>),
}

/// A pair of `Signature` and its associated `Hmac` instance, used to verify the given signature.
#[derive(Debug)]
pub enum Verifier {
    Sha1(GenericVerifier<Sha1>),
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
            b"sha1" => decode_hex::<Sha1>(hex).map(Signature::Sha1),
            _ => Err(SerializeError::UnknownMethod(method)),
        }
    }

    pub fn verify_with(self, secret: &[u8]) -> Verifier {
        match self {
            Signature::Sha1(signature) => Verifier::Sha1(GenericVerifier::new(signature, secret)),
        }
    }
}

impl Verifier {
    pub fn update(&mut self, bytes: &[u8]) {
        match *self {
            Verifier::Sha1(ref mut inner) => inner.mac.update(bytes),
        }
    }

    pub fn verify_reset(&mut self) -> std::result::Result<(), SignatureMismatch> {
        match *self {
            Verifier::Sha1(ref mut inner) => inner.verify_reset(),
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
