use std::convert::{TryFrom, TryInto};
use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::SystemTime;

use diesel::dsl::*;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool};
use futures::{channel::mpsc, future, FutureExt, Stream, StreamExt, TryFutureExt, TryStreamExt};
use hmac::digest::generic_array::typenum::Unsigned;
use hmac::digest::FixedOutput;
use hmac::{Hmac, Mac};
use http::{Request, Response, StatusCode, Uri};
use hyper::server::conn::Http;
use hyper::{Body, Client};
use sha1::Sha1;
use tokio::net::TcpListener;

use crate::schema::*;
use crate::sub;

pub struct Subscriber<C> {
    listener: TcpListener,
    http: Http,
    timer: Option<tokio::time::Delay>,
    rx: mpsc::UnboundedReceiver<Message>,
    shared: Arc<Shared<Client<C>>>,
}

pub struct Feed {
    pub content: Vec<u8>,
}

struct Service<S> {
    shared: Arc<Shared<S>>,
}

enum Message {
    Feed(Feed),
    UpdateTimer(tokio::time::Instant),
}

/// Immutable data shared between the main task and `Service`.
struct Shared<S> {
    host: Uri,
    client: S,
    pool: Pool<ConnectionManager<SqliteConnection>>,
    tx: mpsc::UnboundedSender<Message>,
}

enum Infallible {}

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

// XXX: mediocre naming
const RENEW: u64 = 10;

const X_HUB_SIGNATURE: &str = "x-hub-signature";

impl<C> Subscriber<C>
where
    C: hyper::client::connect::Connect + Clone + Send + Sync + 'static,
{
    pub fn new(
        listener: TcpListener,
        host: Uri,
        client: Client<C>,
        pool: Pool<ConnectionManager<SqliteConnection>>,
    ) -> Self {
        let expiry = active_subscriptions::table
            .select(active_subscriptions::expires_at)
            .order(active_subscriptions::expires_at.asc());
        let timer = if let Some(expires_at) = expiry
            .first::<i64>(&pool.get().unwrap())
            .optional()
            .unwrap()
        {
            let refresh_time = refresh_time(instant_from_epoch(expires_at));
            Some(tokio::time::delay_until(refresh_time))
        } else {
            None
        };

        let (tx, rx) = mpsc::unbounded();

        let shared = Arc::new(Shared {
            host,
            client,
            pool,
            tx,
        });

        Subscriber {
            listener,
            http: Http::new(),
            timer,
            rx,
            shared,
        }
    }

    fn renew_subscriptions(&mut self, cx: &mut Context<'_>) {
        if let Some(ref mut timer) = self.timer {
            if let Poll::Ready(()) = timer.poll_unpin(cx) {
                let now_epoch = now_epoch();
                let threshold: i64 = (now_epoch + RENEW).try_into().unwrap();

                let conn = self.shared.pool.get().unwrap();
                let expiring_subscriptions = active_subscriptions::table
                    .inner_join(subscriptions::table)
                    .select((subscriptions::hub, subscriptions::topic))
                    .filter(active_subscriptions::expires_at.le(threshold))
                    .load::<(String, String)>(&conn)
                    .unwrap();
                for (hub, topic) in expiring_subscriptions {
                    let subscribe =
                        sub::subscribe(&self.shared.host, &hub, &topic, &self.shared.client, &conn);
                    tokio::spawn(subscribe);
                }

                let expiry = active_subscriptions::table
                    .select(active_subscriptions::expires_at)
                    .order(active_subscriptions::expires_at.asc());
                if let Some(expires_at) = expiry.first::<i64>(&conn).optional().unwrap() {
                    let refresh_time = refresh_time(instant_from_epoch(expires_at));
                    timer.reset(refresh_time);
                }
            }
        }
    }

    fn accept_all(&mut self, cx: &mut Context<'_>) {
        while let Poll::Ready(result) = self.listener.poll_accept(cx) {
            let (sock, _) = result.unwrap();

            let service = Service {
                shared: self.shared.clone(),
            };

            tokio::spawn(self.http.serve_connection(sock, service));
        }
    }
}

impl<C> Stream for Subscriber<C>
where
    C: hyper::client::connect::Connect + Clone + Send + Sync + 'static,
{
    type Item = Feed;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Feed>> {
        self.renew_subscriptions(cx);
        self.accept_all(cx);

        while let Poll::Ready(msg) = self.rx.poll_next_unpin(cx) {
            match msg {
                Some(Message::Feed(feed)) => return Poll::Ready(Some(feed)),
                Some(Message::UpdateTimer(expires_at)) => {
                    let refresh_time = refresh_time(expires_at);
                    if let Some(ref mut timer) = self.timer {
                        if refresh_time < timer.deadline() {
                            timer.reset(refresh_time);
                        }
                    } else {
                        self.timer = Some(tokio::time::delay_until(refresh_time));
                    }
                }
                None => return Poll::Ready(None),
            }
        }

        Poll::Pending
    }
}

impl<C> Service<Client<C>>
where
    C: hyper::client::connect::Connect + Clone + Send + Sync + 'static,
{
    fn call_(&mut self, req: Request<Body>) -> Response<Body> {
        const PREFIX: &str = "/websub/callback/";
        let path = req.uri().path();
        let id = if path.starts_with(PREFIX) {
            let id: u64 = path[PREFIX.len()..].parse().unwrap();
            let id: i64 = id.try_into().unwrap();
            id
        } else {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .unwrap();
        };

        let conn = self.shared.pool.get().unwrap();

        if let Some(q) = req.uri().query() {
            return self.verify_intent(id, q, &conn);
        }

        let signature_header = if let Some(v) = req.headers().get(X_HUB_SIGNATURE) {
            v.as_bytes()
        } else {
            eprintln!("* missing signature");
            return Response::new(Body::empty());
        };

        let pos = signature_header.iter().position(|&b| b == b'=');
        let (method, signature_hex) = if let Some(i) = pos {
            let (method, hex) = signature_header.split_at(i);
            (method, &hex[1..])
        } else {
            eprintln!("* malformed signature");
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::empty())
                .unwrap();
        };

        let signature = match method {
            b"sha1" => {
                const LEN: usize = <<Sha1 as FixedOutput>::OutputSize as Unsigned>::USIZE;
                let mut buf = [0u8; LEN];
                hex::decode_to_slice(signature_hex, &mut buf).unwrap();
                buf
            }
            _ => {
                eprintln!(
                    "* unknown digest algorithm: {}",
                    String::from_utf8_lossy(method)
                );
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
                .unwrap();
            Hmac::<Sha1>::new_varkey(secret.as_bytes()).unwrap()
        };

        let tx = self.shared.tx.clone();
        let verify_signature = req
            .into_body()
            .try_fold((Vec::new(), mac), move |(mut vec, mut mac), chunk| {
                vec.extend(&*chunk);
                mac.input(&chunk);
                future::ok((vec, mac))
            })
            .map_ok(move |(content, mac)| {
                let code = mac.result().code();
                if *code == signature {
                    let feed = Feed { content };
                    tx.unbounded_send(Message::Feed(feed)).unwrap();
                } else {
                    eprintln!("* signature mismatch");
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
                let now_i = tokio::time::Instant::now();
                let now_epoch = now_epoch();

                let expires_at_epoch: i64 = (now_epoch + lease_seconds).try_into().unwrap();
                let expires_at_instant = now_i + tokio::time::Duration::from_secs(lease_seconds);

                let msg = Message::UpdateTimer(expires_at_instant);
                self.shared.tx.unbounded_send(msg).unwrap();

                // Remove the old subscription if the subscription was created by a renewal.
                let hub = subscriptions::table
                    .select(subscriptions::hub)
                    .find(id)
                    .get_result::<String>(conn)
                    .unwrap();
                let active_ids = active_subscriptions::table.select(active_subscriptions::id);
                let old_rows = subscriptions::table
                    .filter(subscriptions::id.eq_any(active_ids))
                    .filter(subscriptions::hub.eq(&hub))
                    .filter(subscriptions::topic.eq(&topic));
                let old = old_rows
                    .select(subscriptions::id)
                    .load::<i64>(conn)
                    .unwrap();
                delete(old_rows).execute(conn).unwrap();
                for sub in old {
                    tokio::spawn(sub::unsubscribe(
                        &self.shared.host,
                        sub,
                        &hub,
                        &topic,
                        &self.shared.client,
                    ));
                }

                insert_into(active_subscriptions::table)
                    .values((
                        active_subscriptions::id.eq(id),
                        active_subscriptions::expires_at.eq(expires_at_epoch),
                    ))
                    .execute(conn)
                    .unwrap();

                Response::new(Body::from(challenge))
            }
            Ok(Verify::Unsubscribe { topic, challenge })
                if select(not(exists(row(&topic)))).get_result(conn).unwrap() =>
            {
                Response::new(Body::from(challenge))
            }
            _ => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .unwrap(),
        }
    }
}

impl<C> tower_service::Service<Request<Body>> for Service<Client<C>>
where
    C: hyper::client::connect::Connect + Clone + Send + Sync + 'static,
{
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        future::ok(self.call_(req))
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

fn refresh_time(expires_at: tokio::time::Instant) -> tokio::time::Instant {
    expires_at + tokio::time::Duration::from_secs(RENEW)
}

fn instant_from_epoch(epoch: i64) -> tokio::time::Instant {
    let now_i = tokio::time::Instant::now();
    let now_epoch = now_epoch();
    let eta = u64::try_from(epoch).unwrap().saturating_sub(now_epoch);
    now_i + tokio::time::Duration::from_secs(eta)
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
