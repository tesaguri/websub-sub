use std::fmt::Debug;
use std::future;
use std::marker::PhantomData;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures::channel::mpsc;
use futures::FutureExt;
use http::{Request, Response, StatusCode, Uri};
use http_body::{Body, Full};

use crate::db::{Connection, ConnectionRef, Pool};
use crate::hub;
use crate::signature::{self, Signature};
use crate::util::{consts::HUB_SIGNATURE, empty_response, now_unix, HttpService};

use super::scheduler;
use super::update::{self, Update};
use super::Error;

pub struct Service<P, S, SB, CB> {
    pub(super) callback: Uri,
    pub(super) renewal_margin: u64,
    pub(super) client: S,
    pub(super) pool: P,
    pub(super) tx: mpsc::UnboundedSender<Update<SB>>,
    pub(super) handle: scheduler::Handle,
    pub(super) marker: PhantomData<fn() -> CB>,
}

impl<P, S, SB, CB> Service<P, S, SB, CB>
where
    P: Pool,
    P::Connection: 'static,
    S: HttpService<CB> + Clone + Send + 'static,
    S::Future: Send,
    S::ResponseBody: Send,
    S::Error: Debug + Send,
    SB: 'static,
    CB: Default + From<Vec<u8>> + Send + 'static,
{
    pub fn subscribe<C>(
        &self,
        hub: String,
        topic: String,
        conn: C,
    ) -> Result<hub::ResponseFuture<'static, S::Error>, C::Error>
    where
        C: ConnectionRef<Error = <P::Connection as Connection>::Error>,
    {
        hub::subscribe(&self.callback, hub, topic, self.client.clone(), conn)
    }

    pub fn renew_subscriptions<C>(&self, mut conn: C) -> Result<(), C::Error>
    where
        C: ConnectionRef<Error = <P::Connection as Connection>::Error>,
    {
        let now_unix = now_unix();
        let threshold: i64 = (now_unix.as_secs() + self.renewal_margin + 1)
            .try_into()
            .unwrap();

        conn.transaction(|conn| {
            let expiring = conn.get_subscriptions_expire_before(threshold)?;

            if expiring.is_empty() {
                return Ok(());
            }

            log::info!("Renewing {} expiring subscription(s)", expiring.len());

            for (id, hub, topic) in expiring {
                log::info!(
                    "Renewing a subscription of topic {} at hub {} ({})",
                    topic,
                    hub,
                    id
                );
                let task = self
                    .subscribe(hub, topic, conn.reborrow())?
                    .map(log_and_discard_error);
                tokio::spawn(task);
            }

            conn.deactivate_subscriptions_expire_before(threshold)?;

            Ok(())
        })
    }
}

impl<P, S, SB, CB> Service<P, S, SB, CB>
where
    P: Pool,
    S: HttpService<CB> + Clone + Send + 'static,
    S::Error: Debug,
    S::Future: Send,
    CB: From<Vec<u8>> + Send + 'static,
{
    fn unsubscribe<C>(
        &self,
        id: u64,
        hub: String,
        topic: String,
        conn: C,
    ) -> Result<hub::ResponseFuture<'static, S::Error>, C::Error>
    where
        C: ConnectionRef<Error = <P::Connection as Connection>::Error>,
    {
        hub::unsubscribe(&self.callback, id, hub, topic, self.client.clone(), conn)
    }
}

impl<P, S, SB, CB> Service<P, S, SB, CB>
where
    P: Pool,
    S: HttpService<CB> + Clone + Send + 'static,
    S::Error: Debug,
    S::Future: Send,
    SB: Send + 'static,
    CB: From<Vec<u8>> + Send + 'static,
{
    #[allow(clippy::type_complexity)]
    pub(crate) fn call(
        &self,
        req: Request<SB>,
    ) -> Result<Response<Full<Bytes>>, Error<P::Error, <P::Connection as Connection>::Error>>
    where
        S::Error: Debug,
    {
        let path = req.uri().path();
        let id = if let Some(tail) = path.strip_prefix(self.callback.path()) {
            if let Some(id) = crate::util::callback_id::decode(tail) {
                id
            } else {
                let status = if tail.as_bytes().contains(&b'/') || req.uri().query().is_some() {
                    // Former case is unknown endpoint which might be used in the future or by an
                    // outer `Service`.
                    // The latter is invalid intent verification (see below for rationale).
                    StatusCode::NOT_FOUND
                } else {
                    // Return the same status code as the case of a non-existing callback, in case
                    // an attacker is brute-forcing with no knowledge of our ID format
                    StatusCode::GONE
                };
                return Ok(empty_response(status));
            }
        } else {
            return Ok(empty_response(StatusCode::NOT_FOUND));
        };

        let mut conn = try_pool!(self.pool.get());
        let mut conn = conn.as_conn_ref();

        if let Some(q) = req.uri().query() {
            return self
                .verify_intent(id, q, conn.reborrow())
                .map_err(Error::Connection);
        }

        // Process all `X-Hub-Signature` values.
        // It's not specified in the WebSub recommendation, but seems to be a natural extension.
        let mut signature = None;
        for signature_header in req.headers().get_all(HUB_SIGNATURE) {
            let sig = match Signature::parse(signature_header.as_bytes()) {
                Ok(sig) => sig,
                Err(signature::SerializeError::Parse) => {
                    log::debug!(
                        "Callback {}: unrecognized `X-Hub-Signature` syntax: {:?}",
                        id,
                        signature_header
                    );
                    // Probably something is terribly wrong here, but we'll keep going anyway
                    // because this might be an unknown extension of some sort.
                    continue;
                }
                Err(signature::SerializeError::UnknownMethod(method)) => {
                    let method = String::from_utf8_lossy(method);
                    log::debug!("Callback {}: unknown digest algorithm: {}", id, method);
                    continue;
                }
            };

            if let Some(ref mut prev) = signature {
                // Take the most "preferable" one.
                // XXX: Should we store and verify all of them?
                if sig.is_preferable_to(prev) {
                    *prev = sig
                }
            } else {
                signature = Some(sig)
            }
        }

        let signature = if let Some(signature) = signature {
            signature
        } else {
            log::debug!("Callback {}: missing signature", id);
            // The WebSub spec doesn't seem to specify appropriate error code in this case
            return Ok(Response::default());
        };

        let (parts, body) = req.into_parts();

        let (topic, content) = {
            let cols = try_conn!(conn.get_topic(id));
            let (topic, secret) = if let Some(cols) = cols {
                cols
            } else {
                // Return HTTP 410 code to let the hub end the subscription if the ID is of
                // a previously removed subscription.
                return Ok(empty_response(StatusCode::GONE));
            };
            let content = update::Content::new(body, signature, secret.as_bytes());
            (topic, content)
        };

        if let Err(e) = self.tx.clone().start_send(Update {
            topic: topic.into(),
            headers: parts.headers,
            content,
        }) {
            // A `Sender` has a guaranteed slot in the channel capacity
            // so it won't return a `full` error in this case.
            // https://docs.rs/futures/0.3.17/futures/channel/mpsc/fn.channel.html
            debug_assert!(e.is_disconnected());
        }

        Ok(Response::default())
    }

    fn verify_intent<C>(
        &self,
        id: u64,
        query: &str,
        mut conn: C,
    ) -> Result<Response<Full<Bytes>>, C::Error>
    where
        C: ConnectionRef<Error = <P::Connection as Connection>::Error>,
    {
        match serde_urlencoded::from_str::<hub::Verify<String>>(query) {
            Ok(hub::Verify::Subscribe {
                topic,
                challenge,
                lease_seconds,
            }) => {
                if let Some(hub) = conn.get_hub_of_inactive_subscription(id, &topic)? {
                    log::info!("Verifying subscription {}", id);

                    let now_unix = now_unix();
                    let expires_at = now_unix
                        .as_secs()
                        .saturating_add(lease_seconds)
                        .try_into()
                        .unwrap_or(i64::max_value());

                    self.handle.hasten(self.refresh_time(expires_at as u64));

                    conn.transaction(|conn| {
                        // Remove old subscriptions if any.
                        for id in conn.get_old_subscriptions(id, &hub, &topic)? {
                            log::info!("Removing the old subscription");
                            let task = self
                                .unsubscribe(id, hub.clone(), topic.clone(), conn.reborrow())?
                                .map(log_and_discard_error);
                            tokio::spawn(task);
                        }

                        conn.activate_subscription(id, expires_at)?;
                        Ok(())
                    })?;

                    return Ok(Response::new(challenge.into()));
                }
            }
            Ok(hub::Verify::Unsubscribe { topic, challenge })
                if !conn.subscription_exists(id, &topic)? =>
            {
                log::info!("Vefirying unsubscription of {}", id);
                return Ok(Response::new(challenge.into()));
            }
            _ => {}
        }

        Ok(empty_response(StatusCode::NOT_FOUND))
    }
}

impl<P, S, SB, CB> Service<P, S, SB, CB> {
    pub(crate) fn refresh_time(&self, expires_at: u64) -> u64 {
        expires_at - self.renewal_margin
    }
}

impl<P, S, SB, CB> AsRef<scheduler::Handle> for Service<P, S, SB, CB> {
    fn as_ref(&self) -> &scheduler::Handle {
        &self.handle
    }
}

impl<P, S, SB, CB> tower_service::Service<Request<SB>> for &Service<P, S, SB, CB>
where
    P: Pool,
    S: HttpService<CB> + Clone + Send + 'static,
    <S::ResponseBody as Body>::Error: Debug,
    S::Error: Debug,
    S::Future: Send,
    SB: Send + 'static,
    CB: From<Vec<u8>> + Send + 'static,
{
    type Response = Response<Full<Bytes>>;
    type Error = Error<P::Error, <P::Connection as Connection>::Error>;
    type Future = future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<SB>) -> Self::Future {
        log::trace!("Service::call; req.uri()={:?}", req.uri());
        future::ready((*self).call(req))
    }
}

fn log_and_discard_error<T, E>(result: Result<T, E>)
where
    E: std::fmt::Debug,
{
    if let Err(e) = result {
        log::error!("An HTTP request failed: {:?}", e);
    }
}
