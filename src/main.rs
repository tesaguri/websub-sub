use std::io::{stdout, Write};
use std::net::SocketAddr;

use futures::{future, TryFutureExt, TryStreamExt};
use hyper::server::Server;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Error, Request, Response};
use structopt::StructOpt;

#[derive(StructOpt)]
struct Opt {
    #[structopt(parse(try_from_str), default_value = "127.0.0.1:80")]
    addr: SocketAddr,
}

#[tokio::main]
async fn main() {
    let opt = Opt::from_args();
    let make_svc = make_service_fn(|_| {
        future::ok::<_, Error>(service_fn(|req: Request<Body>| {
            eprintln!("* {}", req.uri());
            let mut stdout = stdout();
            req.into_body()
                .try_for_each(move |chunk| {
                    stdout.write_all(&chunk).unwrap();
                    future::ok(())
                })
                .map_ok(|()| Response::new(Body::empty()))
        }))
    });

    Server::bind(&opt.addr).serve(make_svc).await.unwrap();
}
