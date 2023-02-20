#![forbid(unsafe_op_in_unsafe_fn)]

pub mod db;
pub mod feed;
pub mod hub;
pub mod subscriber;

#[cfg(feature = "diesel2")]
mod schema;
mod util;

#[derive(Debug, thiserror::Error)]
pub enum Error<PE, CE, SE, BE> {
    #[error("failed establish a database connection")]
    Pool(#[source] PE),
    #[error("database connection failed")]
    Connection(#[source] CE),
    #[error("HTTP request failed")]
    Service(#[source] SE),
    #[error("failed to read the HTTP response body")]
    Body(#[source] BE),
}

#[doc(hidden)]
pub mod _private {
    #[cfg(feature = "diesel2")]
    pub extern crate diesel as diesel2;
    #[cfg(feature = "diesel2")]
    pub extern crate rand;
}
