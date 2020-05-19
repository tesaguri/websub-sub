use std::convert::{TryFrom, TryInto};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use diesel::dsl::*;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool};
use futures::channel::mpsc;
use futures::task::AtomicWaker;
use futures::{future, Future, TryFutureExt, TryStreamExt};
use hmac::digest::generic_array::typenum::Unsigned;
use hmac::digest::FixedOutput;
use hmac::{Hmac, Mac};
use http::header::CONTENT_TYPE;
use http::{Request, Response, StatusCode, Uri};
use hyper::client::connect::Connect;
use hyper::{Body, Client};
use sha1::Sha1;
use std::fmt;

use crate::schema::*;
use crate::sub;

use super::MediaType;

pub(super) struct Service<C> {
    pub(super) inner: Arc<Inner<C>>,
}

pub struct Inner<C> {
    pub(super) host: Uri,
    pub(super) client: Client<C>,
    pub(super) pool: Pool<ConnectionManager<SqliteConnection>>,
    pub(super) tx: mpsc::UnboundedSender<(MediaType, Vec<u8>)>,
    pub(super) timer_task: AtomicWaker,
    pub(super) expires_at: AtomicI64,
}

#[derive(serde::Deserialize, Debug)]
#[serde(tag = "hub.mode")]
enum Verify {
    #[serde(rename = "subscribe")]
    Subscribe {
        #[serde(rename = "hub.topic")]
        topic: String,
        #[serde(rename = "hub.challenge")]
        challenge: String,
        #[serde(rename = "hub.lease_seconds")]
        #[serde(deserialize_with = "deserialize_str_as_u64")]
        lease_seconds: u64,
    },
    #[serde(rename = "unsubscribe")]
    Unsubscribe {
        #[serde(rename = "hub.topic")]
        topic: String,
        #[serde(rename = "hub.challenge")]
        challenge: String,
    },
}

pub enum Infallible {}

const CALLBACK_PREFIX: &str = "/websub/callback/";
const X_HUB_SIGNATURE: &str = "x-hub-signature";

impl<C> Inner<C>
where
    C: Connect + Clone + Send + Sync + 'static,
{
    pub(super) fn renew(
        &self,
        id: i64,
        hub: &str,
        topic: &str,
        conn: &SqliteConnection,
    ) -> impl Future<Output = ()> {
        sub::renew(&self.host, id, hub, topic, &self.client, conn)
    }

    pub(super) fn unsubscribe(
        &self,
        id: i64,
        hub: &str,
        topic: &str,
        conn: &SqliteConnection,
    ) -> impl Future<Output = ()> {
        sub::unsubscribe(&self.host, id, hub, topic, &self.client, conn)
    }

    pub(super) fn renew_subscriptions(&self, conn: &SqliteConnection) {
        let now_epoch = super::now_epoch();
        let threshold: i64 = (now_epoch + super::RENEW).try_into().unwrap();

        let renewing = renewing_subscriptions::table.select(renewing_subscriptions::old);
        let expiring = subscriptions::table
            .inner_join(active_subscriptions::table)
            .select((subscriptions::id, subscriptions::hub, subscriptions::topic))
            .filter(active_subscriptions::expires_at.le(threshold))
            .filter(not(subscriptions::id.eq_any(renewing)))
            .load::<(i64, String, String)>(conn)
            .unwrap();

        log::info!("Renewing {} expiring subscription(s)", expiring.len());

        for (id, hub, topic) in expiring {
            tokio::spawn(self.renew(id, &hub, &topic, conn));
        }
    }

    fn call(&self, req: Request<Body>) -> Response<Body> {
        macro_rules! validate {
            ($input:expr) => {
                match $input {
                    Ok(x) => x,
                    Err(_) => {
                        return Response::builder()
                            .status(StatusCode::BAD_REQUEST)
                            .body(Body::empty())
                            .unwrap();
                    }
                }
            };
        }

        let path = req.uri().path();
        let id = if path.starts_with(CALLBACK_PREFIX) {
            let id: u64 = validate!(path[CALLBACK_PREFIX.len()..].parse());
            validate!(i64::try_from(id))
        } else {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .unwrap();
        };

        let conn = self.pool.get().unwrap();

        if let Some(q) = req.uri().query() {
            return self.verify_intent(id, q, &conn);
        }

        let kind: MediaType = if let Some(m) = req
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
        {
            m
        } else {
            return Response::builder()
                .status(StatusCode::UNSUPPORTED_MEDIA_TYPE)
                .body(Body::empty())
                .unwrap();
        };

        let signature_header = if let Some(v) = req.headers().get(X_HUB_SIGNATURE) {
            v.as_bytes()
        } else {
            log::debug!("Callback {}: missing signature", id);
            return Response::new(Body::empty());
        };

        let pos = signature_header.iter().position(|&b| b == b'=');
        let (method, signature_hex) = if let Some(i) = pos {
            let (method, hex) = signature_header.split_at(i);
            (method, &hex[1..])
        } else {
            log::debug!("Callback {}: malformed signature", id);
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::empty())
                .unwrap();
        };

        let signature = match method {
            b"sha1" => {
                const LEN: usize = <<Sha1 as FixedOutput>::OutputSize as Unsigned>::USIZE;
                let mut buf = [0u8; LEN];
                validate!(hex::decode_to_slice(signature_hex, &mut buf));
                buf
            }
            _ => {
                let method = String::from_utf8_lossy(method);
                log::debug!("Callback {}: unknown digest algorithm: {}", id, method);
                return Response::builder()
                    .status(StatusCode::NOT_ACCEPTABLE)
                    .body(Body::empty())
                    .unwrap();
            }
        };

        let mac = {
            let secret = subscriptions::table
                .select(subscriptions::secret)
                .find(id)
                .get_result::<String>(&conn)
                .optional()
                .unwrap();
            let secret = if let Some(s) = secret {
                s
            } else {
                return Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::empty())
                    .unwrap();
            };
            Hmac::<Sha1>::new_varkey(secret.as_bytes()).unwrap()
        };

        let tx = self.tx.clone();
        let verify_signature = req
            .into_body()
            .try_fold((Vec::new(), mac), move |(mut vec, mut mac), chunk| {
                vec.extend(&*chunk);
                mac.input(&chunk);
                future::ok((vec, mac))
            })
            .map_ok(move |(vec, mac)| {
                let code = mac.result().code();
                if *code == signature {
                    tx.unbounded_send((kind, vec)).unwrap();
                } else {
                    log::debug!("Callback {}: signature mismatch", id);
                }
            });
        tokio::spawn(verify_signature);

        Response::new(Body::empty())
    }

    fn verify_intent(&self, id: i64, query: &str, conn: &SqliteConnection) -> Response<Body> {
        let row = |topic| {
            subscriptions::table
                .filter(subscriptions::id.eq(id))
                .filter(subscriptions::topic.eq(topic))
        };
        let sub_is_active =
            subscriptions::id.eq_any(active_subscriptions::table.select(active_subscriptions::id));

        match serde_urlencoded::from_str::<Verify>(query) {
            Ok(Verify::Subscribe {
                topic,
                challenge,
                lease_seconds,
            }) if select(exists(row(&topic).filter(not(sub_is_active))))
                .get_result(conn)
                .unwrap() =>
            {
                log::info!("Verifying subscription {}", id);

                let now_epoch = super::now_epoch();
                let expires_at = now_epoch
                    .saturating_add(lease_seconds)
                    .try_into()
                    .unwrap_or(i64::max_value());

                self.reset_timer(expires_at);

                // Remove the old subscription if the subscription was created by a renewal.
                let old_id = renewing_subscriptions::table
                    .filter(renewing_subscriptions::new.eq(id))
                    .select(renewing_subscriptions::old)
                    .get_result::<i64>(conn)
                    .optional()
                    .unwrap();
                if let Some(old_id) = old_id {
                    let hub = subscriptions::table
                        .select(subscriptions::hub)
                        .find(id)
                        .get_result::<String>(conn)
                        .unwrap();
                    log::info!("Removing the old subscription");
                    tokio::spawn(self.unsubscribe(old_id, &hub, &topic, conn));
                }

                conn.transaction(|| {
                    delete(pending_subscriptions::table.find(id)).execute(conn)?;
                    insert_into(active_subscriptions::table)
                        .values((
                            active_subscriptions::id.eq(id),
                            active_subscriptions::expires_at.eq(expires_at),
                        ))
                        .execute(conn)
                })
                .unwrap();

                Response::new(Body::from(challenge))
            }
            Ok(Verify::Unsubscribe { topic, challenge })
                if select(not(exists(row(&topic)))).get_result(conn).unwrap() =>
            {
                log::info!("Vefirying unsubscription of {}", id);
                Response::new(Body::from(challenge))
            }
            _ => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .unwrap(),
        }
    }

    fn reset_timer(&self, expires_at: i64) {
        let prev = fetch_min(&self.expires_at, expires_at);
        if expires_at < prev {
            self.timer_task.wake();
        }
    }
}

impl<C> Drop for Inner<C> {
    fn drop(&mut self) {
        self.timer_task.wake();
    }
}

impl<C> tower_service::Service<Request<Body>> for Service<C>
where
    C: Connect + Clone + Send + Sync + 'static,
{
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        log::trace!("Service::call; req.uri()={:?}", req.uri());
        future::ok(self.inner.call(req))
    }
}

impl From<Infallible> for Box<dyn std::error::Error + Send + Sync> {
    fn from(i: Infallible) -> Self {
        match i {}
    }
}

fn deserialize_str_as_u64<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    struct Visitor;

    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = u64;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("u64")
        }

        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<u64, E> {
            v.parse()
                .map_err(|_| E::invalid_value(serde::de::Unexpected::Str(v), &self))
        }
    }

    d.deserialize_str(Visitor)
}

// TODO: Use `AtomicI64::fetch_min` once it hits stable.
// https://github.com/rust-lang/rust/issues/48655
fn fetch_min(atomic: &AtomicI64, val: i64) -> i64 {
    let mut prev = atomic.load(Ordering::SeqCst);
    while prev > val {
        match atomic.compare_exchange_weak(prev, val, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return prev,
            Err(p) => prev = p,
        }
    }
    prev
}
