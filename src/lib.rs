#![forbid(unsafe_op_in_unsafe_fn)]

// These crates need to be imported as `diesel` for the `table!` macro to work.
// This means that we cannot use both of `diesel1::table!` and `diesel2::table!` at once.
#[cfg(all(test, feature = "diesel1", not(feature = "diesel2")))]
#[macro_use]
extern crate diesel1 as diesel;
#[cfg(all(test, feature = "diesel2"))]
extern crate diesel2 as diesel;

pub mod db;
pub mod hub;
pub mod subscriber;

#[cfg(all(test, any(feature = "diesel1", feature = "diesel2")))]
mod schema;
mod signature;
mod util;

pub use subscriber::Subscriber;

#[cfg(feature = "diesel1")]
#[doc(hidden)]
pub mod _private {
    pub use core;
    pub use diesel1 as diesel;
    pub use rand;
}
