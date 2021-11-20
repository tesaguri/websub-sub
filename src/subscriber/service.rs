use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::Debug;
use std::future;
use std::marker::PhantomData;
use std::mem;
use std::task::{Context, Poll};

use auto_enums::auto_enum;
use bytes::Bytes;
use diesel::dsl::*;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool};
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

use crate::feed::{self, Feed, RawFeed};
use crate::hub;
use crate::schema::*;
use crate::util::{self, consts::HUB_SIGNATURE, now_unix, CollectBody, HttpService, Never};

use super::scheduler;

pub struct Service<S, B> {
    pub(super) callback: Uri,
    pub(super) renewal_margin: u64,
    pub(super) client: S,
    pub(super) pool: Pool<ConnectionManager<SqliteConnection>>,
    pub(super) tx: mpsc::Sender<(String, Feed)>,
    pub(super) handle: scheduler::Handle,
    pub(super) _marker: PhantomData<fn() -> B>,
}

impl<S, B> Service<S, B>
where
    S: HttpService<B> + Clone + Send + 'static,
    S::Future: Send,
    S::ResponseBody: Send,
    B: From<Vec<u8>> + Send + 'static,
{
    pub fn remove_dangling_subscriptions(&self) {
        let dangling = subscriptions::table.filter(subscriptions::expires_at.is_null());
        diesel::delete(dangling)
            .execute(&*self.pool.get().unwrap())
            .unwrap();
    }

    pub fn discover_and_subscribe(&self, topic: String) -> impl Future<Output = anyhow::Result<()>>
    where
        S::Error: Error + Send + Sync,
        <S::ResponseBody as Body>::Error: Error + Send + Sync,
        B: Default,
    {
        let callback = self.callback.clone();
        let client = self.client.clone();
        let pool = self.pool.clone();
        self.discover(topic).map(|result| {
            result.and_then(move |(topic, hubs)| {
                if let Some(hubs) = hubs {
                    let conn = pool.get()?;
                    for hub in hubs {
                        tokio::spawn(hub::subscribe(
                            &callback,
                            hub,
                            topic.clone(),
                            client.clone(),
                            &*conn,
                        ));
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
    ) -> impl Future<Output = anyhow::Result<(String, Option<impl Iterator<Item = String>>)>>
    where
        S::Error: Error + Send + Sync,
        <S::ResponseBody as Body>::Error: Error + Send + Sync,
        B: Default,
    {
        log::info!("Attempting to discover WebSub hubs for topic {}", topic);

        let req = http::Request::get(&*topic).body(B::default()).unwrap();
        let mut tx = self.tx.clone();
        self.client
            .clone()
            .into_service()
            .oneshot(req)
            .map_err(Into::into)
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

                let body = CollectBody::new(res.into_body()).await?;

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

    pub fn subscribe(
        &self,
        hub: String,
        topic: String,
        conn: &SqliteConnection,
    ) -> impl Future<Output = Result<(), S::Error>> {
        hub::subscribe(&self.callback, hub, topic, self.client.clone(), conn)
    }

    pub fn renew_subscriptions(&self, conn: &SqliteConnection)
    where
        S::Error: Debug,
    {
        let now_unix = now_unix();
        let threshold: i64 = (now_unix.as_secs() + self.renewal_margin + 1)
            .try_into()
            .unwrap();

        let expiring_rows = subscriptions::table.filter(subscriptions::expires_at.le(threshold));

        conn.transaction::<_, diesel::result::Error, _>(|| {
            let expiring = expiring_rows
                .select((subscriptions::id, subscriptions::hub, subscriptions::topic))
                .load::<(i64, String, String)>(conn)?;

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
                tokio::spawn(self.subscribe(hub, topic, conn).map(log_and_discard_error));
            }

            diesel::update(expiring_rows)
                .set(subscriptions::expires_at.eq(None::<i64>))
                .execute(conn)?;

            Ok(())
        })
        .unwrap();
    }

    pub fn unsubscribe_all(
        &self,
        topic: String,
        conn: &SqliteConnection,
    ) -> impl Iterator<Item = impl Future<Output = Result<(), S::Error>>> {
        hub::unsubscribe_all(&self.callback, topic, self.client.clone(), conn)
    }

    fn unsubscribe(
        &self,
        id: i64,
        hub: String,
        topic: String,
        conn: &SqliteConnection,
    ) -> impl Future<Output = Result<(), S::Error>> {
        hub::unsubscribe(&self.callback, id, hub, topic, self.client.clone(), conn)
    }

    fn call(&self, req: Request<hyper::Body>) -> Response<Full<Bytes>>
    where
        S::Error: Debug,
    {
        macro_rules! validate {
            ($input:expr) => {
                match $input {
                    Ok(x) => x,
                    Err(_) => {
                        return Response::builder()
                            .status(StatusCode::BAD_REQUEST)
                            .body(Full::default())
                            .unwrap();
                    }
                }
            };
        }

        let path = req.uri().path();
        let id = if let Some(id) = path.strip_prefix(self.callback.path()) {
            let id = validate!(crate::util::callback_id::decode(id).ok_or(()));
            validate!(i64::try_from(id))
        } else {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Full::default())
                .unwrap();
        };

        let conn = self.pool.get().unwrap();

        if let Some(q) = req.uri().query() {
            return self.verify_intent(id, q, &conn);
        }

        let kind: feed::MediaType = if let Some(m) = req
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
        {
            m
        } else {
            return Response::builder()
                .status(StatusCode::UNSUPPORTED_MEDIA_TYPE)
                .body(Full::default())
                .unwrap();
        };

        let hub_signature = HeaderName::from_static(HUB_SIGNATURE);
        let signature_header = if let Some(v) = req.headers().get(hub_signature) {
            v.as_bytes()
        } else {
            log::debug!("Callback {}: missing signature", id);
            return Response::new(Full::default());
        };

        let pos = signature_header.iter().position(|&b| b == b'=');
        let (method, signature_hex) = if let Some(i) = pos {
            let (method, hex) = signature_header.split_at(i);
            (method, &hex[1..])
        } else {
            log::debug!("Callback {}: malformed signature", id);
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::default())
                .unwrap();
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
                return Response::builder()
                    .status(StatusCode::NOT_ACCEPTABLE)
                    .body(Full::default())
                    .unwrap();
            }
        };

        let (topic, mac) = {
            let cols = subscriptions::table
                .select((subscriptions::topic, subscriptions::secret))
                .find(id)
                .get_result::<(String, String)>(&conn)
                .optional()
                .unwrap();
            let (topic, secret) = if let Some(cols) = cols {
                cols
            } else {
                // Return HTTP 410 code to let the hub end the subscription if the ID is of
                // a previously removed subscription.
                return Response::builder()
                    .status(StatusCode::GONE)
                    .body(Full::default())
                    .unwrap();
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

        Response::new(Full::default())
    }

    fn verify_intent(&self, id: i64, query: &str, conn: &SqliteConnection) -> Response<Full<Bytes>>
    where
        S::Error: Debug,
    {
        let row = |topic| {
            subscriptions::table
                .filter(subscriptions::id.eq(id))
                .filter(subscriptions::topic.eq(topic))
        };

        match serde_urlencoded::from_str::<hub::Verify<String>>(query) {
            Ok(hub::Verify::Subscribe {
                topic,
                challenge,
                lease_seconds,
            }) => {
                if let Some(hub) = row(&topic)
                    .filter(subscriptions::expires_at.is_null())
                    .select(subscriptions::hub)
                    .get_result::<String>(conn)
                    .optional()
                    .unwrap()
                {
                    log::info!("Verifying subscription {}", id);

                    let now_unix = now_unix();
                    let expires_at = now_unix
                        .as_secs()
                        .saturating_add(lease_seconds)
                        .try_into()
                        .unwrap_or(i64::max_value());

                    self.handle.hasten(self.refresh_time(expires_at as u64));

                    conn.transaction::<_, diesel::result::Error, _>(|| {
                        // Remove old subscriptions if any.
                        let old = subscriptions::table
                            .filter(subscriptions::topic.eq(&topic))
                            .filter(subscriptions::hub.eq(&hub))
                            .filter(not(subscriptions::id.eq(id)))
                            .select(subscriptions::id);
                        for id in old.load(conn)? {
                            log::info!("Removing the old subscription");
                            let task = self
                                .unsubscribe(id, hub.clone(), topic.clone(), conn)
                                .map(log_and_discard_error);
                            tokio::spawn(task);
                        }

                        update(subscriptions::table.find(id))
                            .set(subscriptions::expires_at.eq(expires_at))
                            .execute(conn)
                            .unwrap();
                        Ok(())
                    })
                    .unwrap();

                    return Response::new(challenge.into());
                }
            }
            Ok(hub::Verify::Unsubscribe { topic, challenge })
                if select(not(exists(row(&topic)))).get_result(conn).unwrap() =>
            {
                log::info!("Vefirying unsubscription of {}", id);
                return Response::new(challenge.into());
            }
            _ => {}
        }

        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Full::default())
            .unwrap()
    }
}

impl<S, B> Service<S, B> {
    pub fn refresh_time(&self, expires_at: u64) -> u64 {
        expires_at - self.renewal_margin
    }
}

impl<S, B> AsRef<scheduler::Handle> for Service<S, B> {
    fn as_ref(&self) -> &scheduler::Handle {
        &self.handle
    }
}

impl<S, B> tower_service::Service<Request<hyper::Body>> for &Service<S, B>
where
    S: HttpService<B> + Clone + Send + 'static,
    S::Error: Debug,
    S::Future: Send,
    S::ResponseBody: Send,
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
        future::ready(Ok((*self).call(req)))
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