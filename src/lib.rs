#![forbid(unsafe_op_in_unsafe_fn)]

pub mod db;
pub mod hub;
pub mod subscriber;

#[cfg(feature = "diesel2")]
mod schema;
mod signature;
mod util;

pub use subscriber::Subscriber;

#[derive(Debug, thiserror::Error)]
pub enum Error<PE, CE> {
    #[error("failed establish a database connection")]
    Pool(#[source] PE),
    #[error("database connection failed")]
    Connection(#[source] CE),
}
