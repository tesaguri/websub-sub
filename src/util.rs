macro_rules! serde_delegate {
    (visit_str $($rest:tt)*) => {
        fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
            self.visit_bytes(s.as_bytes())
        }
    };
    (visit_bytes $($rest:tt)*) => {
        fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
            std::str::from_utf8(v).map_err(E::custom).and_then(|s| self.visit_str(s))
        }
        serde_delegate!($($rest)*);
    };
    (visit_borrowed_bytes $($rest:tt)*) => {
        fn visit_borrowed_bytes<E: de::Error>(self, v: &'de [u8]) -> Result<Self::Value, E> {
            std::str::from_utf8(v).map_err(E::custom).and_then(|s| self.visit_borrowed_str(s))
        }
        serde_delegate!($($rest)*);
    };
    (visit_byte_buf $($rest:tt)*) => {
        fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
            String::from_utf8(v).map_err(E::custom).and_then(|s| self.visit_string(s))
        }
        serde_delegate!($($rest)*);
    };
    () => {};
}

pub mod consts {
    use http::header::{HeaderName, HeaderValue};

    // <https://github.com/rust-lang/rust-clippy/issues/5812>
    #[allow(clippy::declare_interior_mutable_const)]
    pub const APPLICATION_WWW_FORM_URLENCODED: HeaderValue =
        HeaderValue::from_static("application/x-www-form-urlencoded");
    #[cfg(all(test, feature = "diesel2", feature = "sha-1"))]
    #[allow(clippy::declare_interior_mutable_const)]
    pub const APPLICATION_ATOM_XML: HeaderValue = HeaderValue::from_static("application/atom+xml");
    #[allow(clippy::declare_interior_mutable_const)]
    pub const HUB_SIGNATURE: HeaderName = HeaderName::from_static("x-hub-signature");
}

pub mod callback_id;
#[cfg(all(test, feature = "diesel2", feature = "sha-1"))]
pub mod connection;
pub mod http_service;
pub mod time;

#[cfg(all(test, feature = "diesel2", feature = "sha-1"))]
mod first;

use std::error::Error;
use std::fmt::{self, Display};
use std::io;
use std::marker::PhantomData;
use std::pin::Pin;
use std::str;
use std::str::FromStr;
use std::task::{Context, Poll};

use bytes::Bytes;
use cfg_if::cfg_if;
use serde::{de, Deserialize};
use tokio::io::ReadBuf;

#[cfg(all(test, feature = "diesel2", feature = "sha-1"))]
pub use self::connection::connection;
#[cfg(all(test, feature = "diesel2", feature = "sha-1"))]
pub use self::first::{first, First};
pub use self::http_service::HttpService;
pub use self::time::{instant_from_unix, instant_now, now_unix, system_time_now};
#[cfg(all(test, feature = "diesel2", feature = "sha-1"))]
pub use self::time::{FutureTimeoutExt, Sleep};

#[derive(Deserialize)]
#[serde(untagged)]
pub enum Maybe<T> {
    Just(T),
    Nothing(de::IgnoredAny),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Hash)]
pub enum Never {}

impl Display for Never {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

impl Error for Never {}

impl tokio::io::AsyncRead for Never {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match *self {}
    }
}

impl tokio::io::AsyncWrite for Never {
    fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, _: &[u8]) -> Poll<io::Result<usize>> {
        match *self {}
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        match *self {}
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        match *self {}
    }
}

pub fn deserialize_from_str<'de, T, D>(d: D) -> Result<T, D::Error>
where
    T: FromStr,
    D: de::Deserializer<'de>,
{
    struct Visitor<T>(PhantomData<T>);

    impl<'de, T> de::Visitor<'de> for Visitor<T>
    where
        T: FromStr,
    {
        type Value = T;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(std::any::type_name::<T>())
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            v.parse()
                .map_err(|_| E::invalid_value(de::Unexpected::Str(v), &self))
        }

        serde_delegate!(visit_bytes);
    }

    d.deserialize_str(Visitor::<T>(PhantomData))
}

pub fn empty_response(status: http::StatusCode) -> http::Response<http_body::Full<Bytes>> {
    let mut ret = http::Response::default();
    *ret.status_mut() = status;
    ret
}

cfg_if! {
    if #[cfg(test)] {
        use futures::future;

        pub trait EitherUnwrapExt {
            type Left;
            type Right;

            fn unwrap_left(self) -> Self::Left;
        }

        impl<A, B> EitherUnwrapExt for future::Either<A, B> {
            type Left = A;
            type Right = B;

            #[track_caller]
            fn unwrap_left(self) -> A {
                match self {
                    future::Either::Left(a) => a,
                    future::Either::Right(_) => {
                        panic!("called `Either::unwrap_left()` on a `Right` value");
                    }
                }
            }
        }
    }
}
