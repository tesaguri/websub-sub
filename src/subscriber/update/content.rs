use std::pin::Pin;
use std::task::{ready, Context, Poll};

use bytes::Bytes;
use futures::Stream;
use http::HeaderMap;
use http_body::Body;

use crate::signature::{self, Signature};

use super::{Error, Update};

/// HTTP request body of an [`Update`].
///
/// This implements [`http_body::Body`] and computes the signature of the content as you read the
/// request body and returns an error if it doesn't match the one sent by the hub.
///
/// You can process the body in a streaming fashion, but you must not trust its content (i.e. use it
/// in any observable way, which might potentially allow a brute-force attack) until the body is
/// read to end (i.e. when `poll_data` returns `Poll::Ready(None)`) because the signature
/// verification is only done at the end of the body.
#[derive(Debug)]
pub struct Content {
    body: hyper::Body,
    verifier: signature::Verifier,
}

impl Content {
    pub(crate) fn new(body: hyper::Body, signature: Signature, secret: &[u8]) -> Self {
        let verifier = signature.verify_with(secret);
        Content { body, verifier }
    }
}

impl From<Update> for Content {
    fn from(update: Update) -> Self {
        update.content
    }
}

impl Body for Content {
    type Data = Bytes;
    type Error = Error;

    fn poll_data(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        if let Some(buf) = ready!(Pin::new(&mut self.body)
            .poll_data(cx)
            .map_err(Error::Body)?)
        {
            self.verifier.update(&buf);
            Poll::Ready(Some(Ok(buf)))
        } else if let Err(e) = self.verifier.verify_reset() {
            Poll::Ready(Some(Err(Error::SignatureMismatch(e))))
        } else {
            Poll::Ready(None)
        }
    }

    fn poll_trailers(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
        Pin::new(&mut self.body)
            .poll_trailers(cx)
            .map_err(Error::Body)
    }

    fn size_hint(&self) -> http_body::SizeHint {
        Body::size_hint(&self.body)
    }

    // The absence of `is_end_stream()` is intentional: if it returned `true`, the user wouldn't
    // call `poll_data` again, which would verify the signature for them.
}

impl Stream for Content {
    type Item = super::Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.poll_data(cx)
    }
}
