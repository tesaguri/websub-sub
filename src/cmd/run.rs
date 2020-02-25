use std::io::{stdout, Write};
use std::net::SocketAddr;

use futures::{future, TryFutureExt, TryStreamExt};
use hyper::server::Server;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Error, Request, Response};
use structopt::StructOpt;

#[derive(StructOpt)]
pub struct Opt {
    #[structopt(parse(try_from_str), default_value = "127.0.0.1:80")]
    addr: SocketAddr,
}

#[derive(serde::Deserialize, Default, Debug)]
struct Verify {
    #[serde(rename = "hub.mode")]
    mode: String,
    #[serde(rename = "hub.topic")]
    topic: String,
    #[serde(rename = "hub.challenge")]
    challenge: String,
    #[serde(rename = "hub.lease_seconds")]
    lease_seconds: Option<String>,
}

#[derive(serde::Deserialize)]
struct Maybe<T> {
    #[serde(flatten)]
    value: Option<T>,
}

pub async fn main(opt: Opt) {
    let make_svc = make_service_fn(|_| {
        future::ok::<_, Error>(service_fn(|req: Request<Body>| {
            eprintln!("* {}", req.uri());
            let mut stdout = stdout();
            async {
                if let Some(q) = req.uri().query() {
                    if let Some(hub) = serde_urlencoded::from_str::<Maybe<Verify>>(q)
                        .unwrap()
                        .value
                    {
                        // TODO: verify the request
                        return Ok(Response::new(Body::from(hub.challenge)));
                    }
                }
                req.into_body()
                    .try_for_each(move |chunk| {
                        stdout.write_all(&chunk).unwrap();
                        future::ok(())
                    })
                    .map_ok(|()| Response::new(Body::empty()))
                    .await
            }
        }))
    });

    Server::bind(&opt.addr).serve(make_svc).await.unwrap();
}
