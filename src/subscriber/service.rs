use std::collections::BTreeMap;
use std::fmt::Debug;
use std::future;
use std::marker::PhantomData;
use std::mem;
use std::task::{Context, Poll};

use auto_enums::auto_enum;
use bytes::Bytes;
use futures::channel::mpsc;
use futures::{Future, FutureExt, TryFutureExt, TryStreamExt};
use hmac::digest::generic_array::typenum::Unsigned;
use hmac::digest::FixedOutput;
use hmac::{Hmac, Mac, NewMac};
use http::header::{HeaderName, CONTENT_TYPE};
use http::{Request, Response, StatusCode, Uri};
use http_body::{Body, Full};
use sha1::Sha1;
use tower::ServiceExt;

use crate::db::{Connection, Pool};
use crate::feed::{self, Feed, RawFeed};
use crate::hub;
use crate::util::{self, consts::HUB_SIGNATURE, now_unix, CollectBody, HttpService, Never};
use crate::Error;

use super::scheduler;

pub struct Service<P, S, B> {
    pub(super) callback: Uri,
    pub(super) renewal_margin: u64,
    pub(super) client: S,
    pub(super) pool: P,
    pub(super) tx: mpsc::Sender<(String, Feed)>,
    pub(super) handle: scheduler::Handle,
    pub(super) _marker: PhantomData<fn() -> B>,
}

impl<P, S, B> Service<P, S, B>
where
    P: Pool,
    P::Connection: 'static,
    S: HttpService<B> + Clone + Send + 'static,
    S::Future: Send,
    S::ResponseBody: Send,
    S::Error: Debug + Send,
    B: Default + From<Vec<u8>> + Send + 'static,
{
    pub fn discover_and_subscribe(
        &self,
        topic: String,
    ) -> impl Future<
        Output = Result<
            (),
            Error<
                P::Error,
                <P::Connection as Connection>::Error,
                S::Error,
                <S::ResponseBody as Body>::Error,
            >,
        >,
    > {
        let callback = self.callback.clone();
        let client = self.client.clone();
        let pool = self.pool.clone();
        self.discover(topic).map(|result| {
            result.and_then(move |(topic, hubs)| {
                if let Some(hubs) = hubs {
                    let mut conn = try_pool!(pool.get());
                    for hub in hubs {
                        let task = try_conn!(hub::subscribe(
                            &callback,
                            hub,
                            topic.clone(),
                            client.clone(),
                            &mut conn,
                        ));
                        tokio::spawn(task);
                    }
                }
                Ok(())
            })
        })
    }

    #[auto_enum]
    pub fn discover(
        &self,
        topic: String,
    ) -> impl Future<
        Output = Result<
            (String, Option<impl Iterator<Item = String>>),
            Error<
                P::Error,
                <P::Connection as Connection>::Error,
                S::Error,
                <S::ResponseBody as Body>::Error,
            >,
        >,
    >
    where
        B: Default,
    {
        log::info!("Attempting to discover WebSub hubs for topic {}", topic);

        let req = http::Request::get(&*topic).body(B::default()).unwrap();
        let mut tx = self.tx.clone();
        self.client
            .clone()
            .into_service()
            .oneshot(req)
            .map_err(Error::Service)
            .and_then(|res| async move {
                // TODO: Web Linking discovery

                let kind = if let Some(v) = res.headers().get(CONTENT_TYPE) {
                    if let Some(m) = v.to_str().ok().and_then(|s| s.parse().ok()) {
                        m
                    } else {
                        log::warn!("Topic {}: unsupported media type `{:?}`", topic, v);
                        return Ok((topic, None));
                    }
                } else {
                    feed::MediaType::Xml
                };

                let body = try_body!(CollectBody::new(res.into_body()).await);

                if let Some(mut feed) = RawFeed::parse(kind, &body) {
                    #[auto_enum(Iterator)]
                    let hubs = match feed {
                        RawFeed::Atom(ref mut feed) => mem::take(&mut feed.links)
                            .into_iter()
                            .filter(|link| link.rel == "hub")
                            .map(|link| link.href),
                        RawFeed::Rss(ref mut channel) => rss_hub_links(
                            mem::take(&mut channel.extensions),
                            mem::take(&mut channel.namespaces),
                        ),
                    };
                    if let Err(e) = tx.start_send((topic.clone(), feed.into())) {
                        // A `Sender` has a guaranteed slot in the channel capacity
                        // so it won't return a `full` error in this case.
                        // https://docs.rs/futures/0.3.17/futures/channel/mpsc/fn.channel.html
                        debug_assert!(e.is_disconnected());
                    }
                    Ok((topic, Some(hubs)))
                } else {
                    log::warn!("Topic {}: failed to parse the content", topic);
                    Ok((topic, None))
                }
            })
    }

    pub fn subscribe<C>(
        &self,
        hub: String,
        topic: String,
        conn: &C,
    ) -> Result<impl Future<Output = Result<(), S::Error>>, C::Error>
    where
        C: Connection<Error = <P::Connection as Connection>::Error>,
    {
        hub::subscribe(&self.callback, hub, topic, self.client.clone(), conn)
    }

    pub fn renew_subscriptions<C>(&self, conn: &C) -> Result<(), C::Error>
    where
        C: Connection<Error = <P::Connection as Connection>::Error> + 'static,
    {
        let now_unix = now_unix();
        let threshold: i64 = (now_unix.as_secs() + self.renewal_margin + 1)
            .try_into()
            .unwrap();

        conn.transaction(|| {
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

impl<P, S, B> Service<P, S, B>
where
    P: Pool,
    S: HttpService<B> + Clone + Send + 'static,
    S::Error: Debug,
    S::Future: Send,
    B: From<Vec<u8>> + Send + 'static,
{
    fn unsubscribe<C>(
        &self,
        id: u64,
        hub: String,
        topic: String,
        conn: &C,
    ) -> Result<impl Future<Output = Result<(), S::Error>>, C::Error>
    where
        C: Connection<Error = <P::Connection as Connection>::Error>,
    {
        hub::unsubscribe(&self.callback, id, hub, topic, self.client.clone(), conn)
    }

    fn call(
        &self,
        req: Request<hyper::Body>,
    ) -> Result<
        Response<Full<Bytes>>,
        Error<
            P::Error,
            <P::Connection as Connection>::Error,
            S::Error,
            <S::ResponseBody as Body>::Error,
        >,
    >
    where
        S::Error: Debug,
    {
        macro_rules! validate {
            ($input:expr) => {
                match $input {
                    Ok(x) => x,
                    Err(_) => {
                        return Ok(Response::builder()
                            .status(StatusCode::BAD_REQUEST)
                            .body(Full::default())
                            .unwrap());
                    }
                }
            };
        }

        let path = req.uri().path();
        let id = if let Some(id) = path.strip_prefix(self.callback.path()) {
            validate!(crate::util::callback_id::decode(id).ok_or(()))
        } else {
            return Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Full::default())
                .unwrap());
        };

        let mut conn = try_pool!(self.pool.get());

        if let Some(q) = req.uri().query() {
            return self
                .verify_intent(id, q, &mut conn)
                .map_err(Error::Connection);
        }

        let kind: feed::MediaType = if let Some(m) = req
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
        {
            m
        } else {
            return Ok(Response::builder()
                .status(StatusCode::UNSUPPORTED_MEDIA_TYPE)
                .body(Full::default())
                .unwrap());
        };

        let hub_signature = HeaderName::from_static(HUB_SIGNATURE);
        let signature_header = if let Some(v) = req.headers().get(hub_signature) {
            v.as_bytes()
        } else {
            log::debug!("Callback {}: missing signature", id);
            return Ok(Response::new(Full::default()));
        };

        let pos = memchr::memchr(b'=', signature_header);
        let (method, signature_hex) = if let Some(i) = pos {
            let (method, hex) = signature_header.split_at(i);
            (method, &hex[1..])
        } else {
            log::debug!("Callback {}: malformed signature", id);
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::default())
                .unwrap());
        };

        let signature = match method {
            b"sha1" => {
                const LEN: usize = <<Sha1 as FixedOutput>::OutputSize as Unsigned>::USIZE;
                let mut buf = [0_u8; LEN];
                validate!(hex::decode_to_slice(signature_hex, &mut buf));
                buf
            }
            _ => {
                let method = String::from_utf8_lossy(method);
                log::debug!("Callback {}: unknown digest algorithm: {}", id, method);
                return Ok(Response::builder()
                    .status(StatusCode::NOT_ACCEPTABLE)
                    .body(Full::default())
                    .unwrap());
            }
        };

        let (topic, mac) = {
            let cols = try_conn!(conn.get_topic(id));
            let (topic, secret) = if let Some(cols) = cols {
                cols
            } else {
                // Return HTTP 410 code to let the hub end the subscription if the ID is of
                // a previously removed subscription.
                return Ok(Response::builder()
                    .status(StatusCode::GONE)
                    .body(Full::default())
                    .unwrap());
            };
            let mac = Hmac::<Sha1>::new_from_slice(secret.as_bytes()).unwrap();
            (topic, mac)
        };

        let mut tx = self.tx.clone();
        let verify_signature = req
            .into_body()
            .try_fold((Vec::new(), mac), move |(mut vec, mut mac), chunk| {
                vec.extend(&*chunk);
                mac.update(&chunk);
                future::ready(Ok((vec, mac)))
            })
            .map_ok(move |(content, mac)| {
                let code = mac.finalize().into_bytes();
                if *code == signature {
                    let feed = if let Some(feed) = Feed::parse(kind, &content) {
                        feed
                    } else {
                        log::warn!("Failed to parse an updated content of topic {}", topic);
                        return;
                    };
                    if let Err(e) = tx.start_send((topic, feed)) {
                        debug_assert!(e.is_disconnected());
                    }
                } else {
                    log::debug!("Callback {}: signature mismatch", id);
                }
            })
            .map_err(move |e| log::debug!("Callback {}: failed to load request body: {:?}", id, e))
            .map(|_| ());
        tokio::spawn(verify_signature);

        Ok(Response::new(Full::default()))
    }

    fn verify_intent<C>(
        &self,
        id: u64,
        query: &str,
        conn: &C,
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

                    conn.transaction(|| {
                        // Remove old subscriptions if any.
                        for id in conn.get_old_subscriptions(id, &hub, &topic)? {
                            log::info!("Removing the old subscription");
                            let task = self
                                .unsubscribe(id as u64, hub.clone(), topic.clone(), conn)?
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

        Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Full::default())
            .unwrap())
    }
}

impl<P, S, B> Service<P, S, B> {
    pub fn refresh_time(&self, expires_at: u64) -> u64 {
        expires_at - self.renewal_margin
    }
}

impl<P, S, B> AsRef<scheduler::Handle> for Service<P, S, B> {
    fn as_ref(&self) -> &scheduler::Handle {
        &self.handle
    }
}

impl<P, S, B> tower_service::Service<Request<hyper::Body>> for &Service<P, S, B>
where
    P: Pool,
    P::Error: Debug,
    <P::Connection as Connection>::Error: Debug,
    S: HttpService<B> + Clone + Send + 'static,
    <S::ResponseBody as Body>::Error: Debug,
    S::Error: Debug,
    S::Future: Send,
    B: From<Vec<u8>> + Send + 'static,
{
    type Response = Response<Full<Bytes>>;
    type Error = Never;
    type Future = future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<hyper::Body>) -> Self::Future {
        log::trace!("Service::call; req.uri()={:?}", req.uri());
        match (*self).call(req) {
            Ok(ret) => future::ready(Ok(ret)),
            Err(e) => {
                log::error!("error while serving an HTTP request: {:?}", e);
                future::ready(Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Full::default())
                    .unwrap()))
            }
        }
    }
}

fn rss_hub_links(
    extensions: rss::extension::ExtensionMap,
    namespaces: BTreeMap<String, String>,
) -> impl Iterator<Item = String> {
    extensions.into_iter().flat_map(move |(prefix, map)| {
        let prefix_is_atom = namespaces
            .get(&*prefix)
            .map_or(false, |s| s == util::consts::NS_ATOM);
        map.into_iter()
            .filter_map(|(name, elms)| (name == "link").then(move || elms))
            .flatten()
            .filter(move |elm| {
                if let Some((_, ns)) = elm
                    .attrs
                    .iter()
                    .find(|(k, _)| k.strip_prefix("xmlns:") == Some(&prefix))
                {
                    ns == util::consts::NS_ATOM
                } else {
                    prefix_is_atom
                }
            })
            .filter(|elm| elm.attrs.get("rel").map(|s| &**s) == Some("hub"))
            .filter_map(|mut elm| elm.attrs.remove("href"))
    })
}

fn log_and_discard_error<T, E>(result: Result<T, E>)
where
    E: std::fmt::Debug,
{
    if let Err(e) = result {
        log::error!("An HTTP request failed: {:?}", e);
    }
}
