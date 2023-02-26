use std::pin::Pin;
use std::task::{ready, Context, Poll};

use bytes::Buf;
use futures::Stream;
use http::HeaderMap;
use http_body::Body;
use pin_project::pin_project;

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
#[pin_project]
pub struct Content<B> {
    #[pin]
    body: B,
    verifier: signature::Verifier,
}

impl<B> Content<B> {
    pub(crate) fn new(body: B, signature: Signature, secret: &[u8]) -> Self {
        let verifier = signature.verify_with(secret);
        Content { body, verifier }
    }
}

impl<B> From<Update<B>> for Content<B> {
    fn from(update: Update<B>) -> Self {
        update.content
    }
}

impl<B: Body> Body for Content<B>
where
    B::Data: Clone,
{
    type Data = B::Data;
    type Error = Error<B::Error>;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let this = self.project();
        if let Some(buf) = ready!(this.body.poll_data(cx).map_err(Error::Body)?) {
            let mut chunk = buf.chunk();
            if chunk.len() == buf.remaining() {
                // Skip cloning in the common case, including when `B::Data = Bytes`
                this.verifier.update(chunk);
            } else {
                let mut buf = buf.clone();
                loop {
                    this.verifier.update(chunk);
                    buf.advance(chunk.len());
                    if !buf.has_remaining() {
                        break;
                    }
                    chunk = buf.chunk();
                }
            }
            Poll::Ready(Some(Ok(buf)))
        } else if let Err(e) = this.verifier.verify_reset() {
            Poll::Ready(Some(Err(Error::SignatureMismatch(e))))
        } else {
            Poll::Ready(None)
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
        self.project().body.poll_trailers(cx).map_err(Error::Body)
    }

    fn size_hint(&self) -> http_body::SizeHint {
        Body::size_hint(&self.body)
    }

    // The absence of `is_end_stream()` is intentional: if it returned `true`, the user wouldn't
    // call `poll_data` again, which would verify the signature for them.
}

impl<B: Body> Stream for Content<B>
where
    B::Data: Clone,
{
    type Item = Result<B::Data, Error<B::Error>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.poll_data(cx)
    }
}

#[cfg(all(tests, feature = "sha-1"))]
mod tests {
    use std::convert::Infallible;

    use super::*;

    #[derive(Clone)]
    struct MockBuf(&'static [Bytes]);

    #[derive(Clone)]
    struct MockBody(&'static [MockBuf]);

    const SECRET: &[u8] = b"super strong secret";

    impl Buf for MockBuf {
        fn remaining(&self) -> usize {
            self.0.iter().map(|buf| buf.len()).sum()
        }

        fn chunk(&self) -> &[u8] {
            self.0[0]
        }

        fn advance(&mut self, mut cnt: usize) {
            while cnt > 0 {
                let chunk = &mut self.0[0];
                if cnt < chunk.len() {
                    *chunk = chunk[cnt..];
                    return;
                }
                cnt -= chunk.len();
                self.0 = &self.0[1..];
            }
        }
    }

    impl Body for MockBody {
        type Data = MockBuf;
        type Error = Infallible;

        fn poll_data(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
            let ret = self.0[0];
            self.0 = &self.0[1..];
            Poll::Ready(Some(Ok(ret)))
        }

        fn poll_trailers(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
            Poll::Ready(None)
        }
    }

    #[tokio::test]
    async fn preserves_body() {
        #[track_caller]
        async fn assert_identity(body: MockBody) {
            let signature = sign(body.clone()).await;
            let content = Content::new(body.clone(), signature, SECRET);
            let content = hyper::body::to_bytes(content).await.unwrap();
            let body = hyper::body::to_bytes(body.clone()).await.unwrap();
            assert_eq!(
                content,
                body,
                "body: {:?}",
                body.0.iter().map(|buf| buf.0).collect::<Vec<_>>()
            );
        }

        assert_identity(MockBody(&[Bytes::from_static(b"hello")])).await;
        assert_identity(MockBody(&[
            Bytes::from_static(b"hello"),
            Bytes::from_static(b" world"),
        ]))
        .await;
    }

    #[tokio::test]
    async fn rejects_invalid_signature() {
        #[track_caller]
        async fn assert_reject(body: MockBody) {
            const INVALID_SECRET: &[u8] = "super malicious secret";

            let signature = sign(body.clone(), INVALID_SECRET).await;
            let content = Content::new(body.clone(), signature, SECRET);
            let result = hyper::body::to_bytes().await;
            assert!(matches!(
                result,
                Err(Error::SignatureMismatch(_)),
                "body: {:?}"
            ));
        }

        assert_reject(MockBody(&[Bytes::from_static(b"hello")])).await;
        assert_reject(MockBody(&[
            Bytes::from_static(b"hello"),
            Bytes::from_static(b" world"),
        ]))
        .await;
    }

    async fn sign(body: MockBody, secret: &[u8]) -> Signature {
        let mac = Hmac::<Sha1>::new_from_slice(secret).unwrap();
        mac.update(&hyper::body::to_bytes(body).await.unwrap());
        Signature::Sha1(mac.finalize().into())
    }
}
