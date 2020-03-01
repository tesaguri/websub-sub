use std::convert::{TryFrom, TryInto};
use std::fmt;
use std::io::{stdout, Write};
use std::net::{SocketAddr, ToSocketAddrs};
use std::time::SystemTime;

use diesel::dsl::*;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool};
use futures::{future, stream, StreamExt, TryFutureExt, TryStreamExt};
use hmac::digest::generic_array::typenum::Unsigned;
use hmac::digest::FixedOutput;
use hmac::{Hmac, Mac};
use http::{Request, Response, Uri};
use hyper::server::conn::Http;
use hyper::service::service_fn;
use hyper::{Body, Error};
use sha1::Sha1;
use structopt::StructOpt;

use crate::schema::*;
use crate::sub;

#[derive(StructOpt)]
pub struct Opt {
    host: Uri,
    #[structopt(parse(try_from_str))]
    bind: Option<SocketAddr>,
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

// XXX: mediocre naming
const RENEW: u64 = 10;

#[derive(serde::Deserialize)]
struct Maybe<T> {
    #[serde(flatten)]
    value: Option<T>,
}

const X_HUB_SIGNATURE: &str = "x-hub-signature";

pub async fn main(opt: Opt) {
    let client = crate::common::http_client();
    let pool = crate::common::database_pool();

    let (tx, rx) = futures::channel::mpsc::unbounded();
    let subscription_renewer =
        subscription_renewer(opt.host.clone(), rx, client.clone(), pool.clone());

    let addr = if let Some(addr) = opt.bind {
        addr
    } else {
        let port = if let Some(p) = opt.host.port_u16() {
            p
        } else {
            match opt.host.scheme() {
                Some(s) if s == "https" => 443,
                Some(s) if s == "http" => 80,
                Some(s) => panic!("default port for scheme `{}` is unknown", s),
                None => panic!("missing URI scheme for host argument"),
            }
        };
        (opt.host.host().unwrap(), port)
            .to_socket_addrs()
            .unwrap()
            .next()
            .unwrap()
    };

    let mut listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    let incoming =
        stream::poll_fn(move |cx| listener.poll_accept(cx).map(Some)).map_ok(|(sock, _)| sock);
    let server = incoming.try_fold(Http::new(), move |http, sock| {
        let host = opt.host.clone();
        let mut tx = tx.clone();
        let client = client.clone();
        let pool = pool.clone();

        let service = service_fn(move |req| {
            eprintln!("* {}", req.uri());
            future::ok::<_, Error>(serve(req, &host, &mut tx, &client, &pool))
        });
        tokio::spawn(http.serve_connection(sock, service));

        future::ok(http)
    });

    let (result, ()) = future::join(server, subscription_renewer).await;
    result.unwrap();
}

fn serve<C>(
    req: Request<Body>,
    host: &Uri,
    tx: &mut futures::channel::mpsc::UnboundedSender<tokio::time::Instant>,
    client: &hyper::Client<C>,
    pool: &Pool<ConnectionManager<SqliteConnection>>,
) -> Response<Body>
where
    C: hyper::client::connect::Connect + Clone + Send + Sync + 'static,
{
    const PREFIX: &str = "/websub/callback/";
    let path = req.uri().path();
    let id = if path.starts_with(PREFIX) {
        let id: u64 = path[PREFIX.len()..].parse().unwrap();
        let id: i64 = id.try_into().unwrap();
        id
    } else {
        return Response::builder()
            .status(http::StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap();
    };

    let conn = pool.get().unwrap();

    // Verification of intent (§5.3)
    if let Some(q) = req.uri().query() {
        if let Some(hub) = serde_urlencoded::from_str::<Maybe<Verify>>(q)
            .unwrap()
            .value
        {
            let row = |topic| {
                subscriptions::table
                    .filter(subscriptions::id.eq(id))
                    .filter(subscriptions::topic.eq(topic))
            };
            let sub_is_active = subscriptions::id
                .eq_any(active_subscriptions::table.select(active_subscriptions::id));
            let challenge = match hub {
                Verify::Subscribe {
                    topic,
                    challenge,
                    lease_seconds,
                } if select(exists(row(&topic).filter(not(sub_is_active))))
                    .get_result(&conn)
                    .unwrap() =>
                {
                    let now_i = tokio::time::Instant::now();
                    let now_epoch = now_epoch();

                    let expires_at_epoch: i64 = (now_epoch + lease_seconds).try_into().unwrap();
                    let expires_at_instant =
                        now_i + tokio::time::Duration::from_secs(lease_seconds);

                    tx.unbounded_send(expires_at_instant).unwrap();

                    // Remove the old subscription if the subscription was created by a renewal.
                    let hub = subscriptions::table
                        .select(subscriptions::hub)
                        .find(id)
                        .get_result::<String>(&conn)
                        .unwrap();
                    let active_ids = active_subscriptions::table.select(active_subscriptions::id);
                    let old_rows = subscriptions::table
                        .filter(subscriptions::id.eq_any(active_ids))
                        .filter(subscriptions::hub.eq(&hub))
                        .filter(subscriptions::topic.eq(&topic));
                    let old = old_rows
                        .select(subscriptions::id)
                        .load::<i64>(&conn)
                        .unwrap();
                    delete(old_rows).execute(&conn).unwrap();
                    for sub in old {
                        tokio::spawn(sub::unsubscribe(host, sub, &hub, &topic, client));
                    }

                    insert_into(active_subscriptions::table)
                        .values((
                            active_subscriptions::id.eq(id),
                            active_subscriptions::expires_at.eq(expires_at_epoch),
                        ))
                        .execute(&conn)
                        .unwrap();

                    Some(challenge)
                }
                Verify::Unsubscribe { topic, challenge }
                    if select(not(exists(row(&topic)))).get_result(&conn).unwrap() =>
                {
                    Some(challenge)
                }
                _ => None,
            };
            if let Some(challenge) = challenge {
                return Response::new(Body::from(challenge));
            } else {
                return Response::builder()
                    .status(http::StatusCode::NOT_FOUND)
                    .body(Body::empty())
                    .unwrap();
            }
        }
    }

    let mac = {
        let secret = subscriptions::table
            .select(subscriptions::secret)
            .find(id)
            .get_result::<String>(&conn)
            .unwrap();
        Hmac::<Sha1>::new_varkey(secret.as_bytes()).unwrap()
    };

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
            .status(http::StatusCode::BAD_REQUEST)
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
                .status(http::StatusCode::NOT_ACCEPTABLE)
                .body(Body::empty())
                .unwrap();
        }
    };

    let mut stdout = stdout();
    let print = req
        .into_body()
        .try_fold(mac, move |mut mac, chunk| {
            mac.input(&chunk);
            stdout.write_all(&chunk).unwrap();
            future::ok(mac)
        })
        .map_ok(move |mac| {
            let code = mac.result().code();
            if *code != signature {
                eprintln!("* signature mismatch");
            }
        });
    tokio::spawn(print);

    return Response::new(Body::empty());
}

async fn subscription_renewer<C>(
    host: Uri,
    mut rx: futures::channel::mpsc::UnboundedReceiver<tokio::time::Instant>,
    client: hyper::Client<C>,
    pool: Pool<ConnectionManager<SqliteConnection>>,
) where
    C: hyper::client::connect::Connect + Clone + Send + Sync + 'static,
{
    let expiry = active_subscriptions::table
        .select(active_subscriptions::expires_at)
        .order(active_subscriptions::expires_at.asc());
    let mut timer = if let Some(expires_at) = expiry
        .first::<i64>(&pool.get().unwrap())
        .optional()
        .unwrap()
    {
        let refresh_time = refresh_time(instant_from_epoch(expires_at));
        future::Either::Left(tokio::time::delay_until(refresh_time))
    } else {
        future::Either::Right(future::pending())
    };

    loop {
        match future::select(rx.next(), &mut timer).await {
            future::Either::Left((Some(expires_at), _)) => {
                let refresh_time = refresh_time(expires_at);
                match timer {
                    future::Either::Left(ref mut timer) => {
                        if refresh_time < timer.deadline() {
                            timer.reset(refresh_time);
                        }
                    }
                    future::Either::Right(_) => {
                        timer = future::Either::Left(tokio::time::delay_until(refresh_time));
                    }
                }
            }
            future::Either::Left((None, _)) => return,
            future::Either::Right(((), _)) => {
                let now_epoch = now_epoch();
                let threshold: i64 = (now_epoch + RENEW).try_into().unwrap();

                let conn = pool.get().unwrap();
                let expiring_subscriptions = active_subscriptions::table
                    .inner_join(subscriptions::table)
                    .select((subscriptions::hub, subscriptions::topic))
                    .filter(active_subscriptions::expires_at.le(threshold))
                    .load::<(String, String)>(&pool.get().unwrap())
                    .unwrap();
                for (hub, topic) in expiring_subscriptions {
                    tokio::spawn(sub::subscribe(&host, &hub, &topic, &client, &conn));
                }

                if let Some(expires_at) = expiry
                    .first::<i64>(&pool.get().unwrap())
                    .optional()
                    .unwrap()
                {
                    let refresh_time = refresh_time(instant_from_epoch(expires_at));
                    match timer {
                        future::Either::Left(ref mut timer) => timer.reset(refresh_time),
                        future::Either::Right(_pending) => unreachable!(),
                    }
                }
            }
        }
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
