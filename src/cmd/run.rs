use std::io::{stdout, Write};
use std::net::{SocketAddr, ToSocketAddrs};

use futures::{future, StreamExt};
use http::Uri;
use structopt::StructOpt;

use crate::subscriber::Subscriber;

#[derive(StructOpt)]
pub struct Opt {
    host: Uri,
    #[structopt(parse(try_from_str))]
    bind: Option<SocketAddr>,
}

pub async fn main(opt: Opt) {
    let client = crate::common::http_client();
    let pool = crate::common::database_pool();

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

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    let subscriber = Subscriber::new(listener, opt.host, client, pool);

    let stdout = stdout();
    let mut stdout = stdout.lock();

    subscriber
        .for_each(|feed| {
            writeln!(stdout, "Feed: {} ({})", feed.title, feed.id).unwrap();
            for e in feed.entries {
                stdout.write_all(b"Entry:").unwrap();
                if let Some(title) = e.title {
                    write!(stdout, " {}", title).unwrap();
                }
                if let Some(id) = e.id {
                    write!(stdout, " (ID: {})", id).unwrap();
                }
                if let Some(link) = e.link {
                    write!(stdout, " (link: {})", link).unwrap();
                }
                stdout.write_all(b"\n").unwrap();
            }
            future::ready(())
        })
        .await;
}
