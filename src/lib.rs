#![forbid(unsafe_op_in_unsafe_fn)]

#[cfg(feature = "diesel1")]
#[macro_use]
extern crate diesel;

mod util;

pub mod db;
pub mod feed;
pub mod hub;
#[cfg(feature = "diesel1")]
pub mod schema;
pub mod subscriber;

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
