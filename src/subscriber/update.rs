mod content;

pub use crate::signature::SignatureMismatch;

pub use self::content::Content;

use http::HeaderMap;

/// Content and metadata of an update pushed by a hub.
#[derive(Debug)]
pub struct Update {
    /// The topic URI associated with the update.
    pub topic: Box<str>,
    /// The HTTP request header of the update.
    pub headers: HeaderMap,
    /// The HTTP request body of the update.
    pub content: Content,
}

/// Error while reading a {`Content`} body.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The underlying HTTP body returned an error.
    #[error("failed to read request body")]
    Body(#[source] hyper::Error),
    /// The signature sent by the hub didn't verify, indicating that the distributed [`Content`]
    /// has been falsified.
    ///
    /// If you encounter this error, you must not trust the content.
    #[error(transparent)]
    SignatureMismatch(SignatureMismatch),
}

/// Convenience type alias for the `Result` type returned by [`Content`].
pub type Result<T> = std::result::Result<T, Error>;
