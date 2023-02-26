use std::fmt::Debug;
use std::future;
use std::marker::PhantomData;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures::channel::mpsc;
use futures::{Future, FutureExt};
use http::{Request, Response, StatusCode, Uri};
use http_body::{Body, Full};

use crate::db::{Connection, Pool};
use crate::hub;
use crate::signature::{self, Signature};
use crate::util::{consts::HUB_SIGNATURE, empty_response, now_unix, HttpService};
use crate::Error;

use super::scheduler;
use super::update::{self, Update};

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
        conn: &mut C,
    ) -> Result<impl Future<Output = Result<(), S::Error>>, C::Error>
    where
        C: Connection<Error = <P::Connection as Connection>::Error>,
    {
        hub::subscribe(&self.callback, hub, topic, self.client.clone(), conn)
    }

    pub fn renew_subscriptions<C>(&self, conn: &mut C) -> Result<(), C::Error>
    where
        C: Connection<Error = <P::Connection as Connection>::Error> + 'static,
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
                tokio::spawn(self.subscribe(hub, topic, conn)?.map(log_and_discard_error));
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
        conn: &mut C,
    ) -> Result<impl Future<Output = Result<(), S::Error>>, C::Error>
    where
        C: Connection<Error = <P::Connection as Connection>::Error>,
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
        let id = if let Some(id) = path
            .strip_prefix(self.callback.path())
            .and_then(crate::util::callback_id::decode)
        {
            id
        } else {
            return Ok(empty_response(StatusCode::NOT_FOUND));
        };

        let mut conn = try_pool!(self.pool.get());

        if let Some(q) = req.uri().query() {
            return self
                .verify_intent(id, q, &mut conn)
                .map_err(Error::Connection);
        }

        let signature_header = if let Some(v) = req.headers().get(HUB_SIGNATURE) {
            v
        } else {
            log::debug!("Callback {}: missing signature", id);
            // The WebSub spec doesn't seem to specify appropriate error code in this case
            return Ok(Response::default());
        };

        let signature = match Signature::parse(signature_header.as_bytes()) {
            Ok(signature) => signature,
            Err(signature::SerializeError::Parse) => {
                log::debug!(
                    "Callback {}: malformed signature: {:?}",
                    id,
                    signature_header
                );
                return Ok(empty_response(StatusCode::BAD_REQUEST));
            }
            Err(signature::SerializeError::UnknownMethod(method)) => {
                let method = String::from_utf8_lossy(method);
                log::debug!("Callback {}: unknown digest algorithm: {}", id, method);
                return Ok(empty_response(StatusCode::NOT_ACCEPTABLE));
            }
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
        conn: &mut C,
    ) -> Result<Response<Full<Bytes>>, C::Error>
    where
        C: Connection<Error = <P::Connection as Connection>::Error> + 'static,
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
                                .unsubscribe(id, hub.clone(), topic.clone(), conn)?
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
