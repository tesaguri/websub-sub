use std::convert::TryInto;
use std::io::{stdout, Write};
use std::net::SocketAddr;

use diesel::dsl::*;
use diesel::prelude::*;
use futures::{future, TryFutureExt, TryStreamExt};
use hyper::server::Server;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Error, Request, Response};
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
struct Maybe<T> {
    #[serde(flatten)]
    value: Option<T>,
}

pub async fn main(opt: Opt) {
    let pool = crate::common::database_pool();

    let make_svc = make_service_fn(move |_| {
        let pool = pool.clone();
        future::ok::<_, Error>(service_fn(move |req: Request<Body>| {
            eprintln!("* {}", req.uri());

            // Use an immediately invoked closure to prevent the `async` block from capturing `pool`
            let catch = (|| {
                const PREFIX: &str = "/websub/callback/";
                let path = req.uri().path();
                let id = if path.starts_with(PREFIX) {
                    let id: u64 = path[PREFIX.len()..].parse().unwrap();
                    let id: i64 = id.try_into().unwrap();
                    id
                } else {
                    return Some(
                        Response::builder()
                            .status(hyper::StatusCode::NOT_FOUND)
                            .body(Body::empty())
                            .unwrap(),
                    );
                };
                if let Some(q) = req.uri().query() {
                    if let Some(hub) = serde_urlencoded::from_str::<Maybe<Verify>>(q)
                        .unwrap()
                        .value
                    {
                        let conn = pool.get().unwrap();
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
                            return Some(Response::new(Body::from(challenge)));
                        } else {
                            return Some(
                                Response::builder()
                                    .status(hyper::StatusCode::NOT_FOUND)
                                    .body(Body::empty())
                                    .unwrap(),
                            );
                        }
                    }
                }

                None
            })();

            // TODO: verify the signature

            async {
                if let Some(response) = catch {
                    Ok(response)
                } else {
                    let mut stdout = stdout();
                    req.into_body()
                        .try_for_each(move |chunk| {
                            stdout.write_all(&chunk).unwrap();
                            future::ok(())
                        })
                        .map_ok(|()| Response::new(Body::empty()))
                        .await
                }
            }
        }))
    });

    Server::bind(&opt.addr).serve(make_svc).await.unwrap();
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
