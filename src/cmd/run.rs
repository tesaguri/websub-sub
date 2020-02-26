use std::convert::TryInto;
use std::io::{stdout, Write};
use std::net::SocketAddr;

use auto_enums::auto_enum;
use diesel::dsl::*;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool};
use futures::{future, Future, TryFutureExt, TryStreamExt};
use hmac::digest::generic_array::typenum::Unsigned;
use hmac::digest::FixedOutput;
use hmac::{Hmac, Mac};
use hyper::server::Server;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Error, Request, Response};
use sha1::Sha1;
use structopt::StructOpt;

use crate::schema::subscriptions;

#[derive(StructOpt)]
pub struct Opt {
    #[structopt(parse(try_from_str), default_value = "127.0.0.1:80")]
    addr: SocketAddr,
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
        lease_seconds: String,
    },
    #[serde(rename = "unsubscribe")]
    Unsubscribe {
        #[serde(rename = "hub.topic")]
        topic: String,
        #[serde(rename = "hub.challenge")]
        challenge: String,
    },
}

#[derive(serde::Deserialize)]
enum Signature<'a> {
    #[serde(rename = "sha1")]
    Sha1(&'a str),
}

#[derive(serde::Deserialize)]
struct Maybe<T> {
    #[serde(flatten)]
    value: Option<T>,
}

const X_HUB_SIGNATURE: &str = "x-hub-signature";

pub async fn main(opt: Opt) {
    let pool = crate::common::database_pool();

    let make_svc = make_service_fn(move |_| {
        let pool = pool.clone();
        future::ok::<_, Error>(service_fn(move |req: Request<Body>| {
            eprintln!("* {}", req.uri());

            serve(req, &pool)
        }))
    });

    Server::bind(&opt.addr).serve(make_svc).await.unwrap();
}

#[auto_enum(Future)]
fn serve(
    req: Request<Body>,
    pool: &Pool<ConnectionManager<SqliteConnection>>,
) -> impl Future<Output = Result<Response<Body>, Error>> {
    const PREFIX: &str = "/websub/callback/";
    let path = req.uri().path();
    let id = if path.starts_with(PREFIX) {
        let id: u64 = path[PREFIX.len()..].parse().unwrap();
        let id: i64 = id.try_into().unwrap();
        id
    } else {
        return future::ok(
            Response::builder()
                .status(hyper::StatusCode::NOT_FOUND)
                .body(Body::empty())
                .unwrap(),
        );
    };

    let conn = pool.get().unwrap();

    if let Some(q) = req.uri().query() {
        if let Some(hub) = serde_urlencoded::from_str::<Maybe<Verify>>(q)
            .unwrap()
            .value
        {
            let challenge = match hub {
                Verify::Subscribe {
                    topic, challenge, ..
                } => {
                    if subscription_exists(&conn, id, &topic) {
                        Some(challenge)
                    } else {
                        None
                    }
                }
                Verify::Unsubscribe { topic, challenge } => {
                    if !subscription_exists(&conn, id, &topic) {
                        Some(challenge)
                    } else {
                        None
                    }
                }
            };
            if let Some(challenge) = challenge {
                return future::ok(Response::new(Body::from(challenge)));
            } else {
                return future::ok(
                    Response::builder()
                        .status(hyper::StatusCode::NOT_FOUND)
                        .body(Body::empty())
                        .unwrap(),
                );
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
        return future::ok(Response::new(Body::empty()));
    };

    let pos = signature_header.iter().position(|&b| b == b'=');
    let (method, signature_hex) = if let Some(i) = pos {
        let (method, hex) = signature_header.split_at(i);
        (method, &hex[1..])
    } else {
        eprintln!("* malformed signature");
        return future::ok(
            Response::builder()
                .status(hyper::StatusCode::BAD_REQUEST)
                .body(Body::empty())
                .unwrap(),
        );
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
            return future::ok(
                Response::builder()
                    .status(hyper::StatusCode::NOT_ACCEPTABLE)
                    .body(Body::empty())
                    .unwrap(),
            );
        }
    };

    let mut stdout = stdout();
    return req
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
            Response::new(Body::empty())
        });
}

fn subscription_exists(conn: &SqliteConnection, id: i64, topic: &str) -> bool {
    select(exists(
        subscriptions::table.filter(
            subscriptions::id
                .eq(id)
                .and(subscriptions::topic.eq(&topic)),
        ),
    ))
    .get_result(conn)
    .unwrap()
}
