use std::io::{stdout, Write};
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::Path;

use futures::{future, Stream, StreamExt};
use http::Uri;
use structopt::StructOpt;

use crate::subscriber::Subscriber;

#[derive(StructOpt)]
pub struct Opt {
    host: Uri,
    bind: Option<String>,
}

pub async fn main(opt: Opt) {
    let client = crate::common::http_client();
    let pool = crate::common::database_pool();

    if let Some(bind) = opt.bind.as_ref() {
        if bind.starts_with("tcp://") {
            let addr: SocketAddr = bind[6..].parse().unwrap();
            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
            let subscriber = Subscriber::new(listener, opt.host, client, pool);
            print_all(subscriber).await;
        } else if bind.starts_with("unix://") {
            let path = Path::new(&bind[7..]);
            let listener = tokio::net::UnixListener::bind(path).unwrap();
            let subscriber = Subscriber::new(listener, opt.host, client, pool);
            print_all(subscriber).await;
        } else {
            panic!("unknown bind address type");
        }
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
        let addr = (opt.host.host().unwrap(), port)
            .to_socket_addrs()
            .unwrap()
            .next()
            .unwrap();
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        let subscriber = Subscriber::new(listener, opt.host, client, pool);
        print_all(subscriber).await;
    }
}

async fn print_all(s: impl Stream<Item = crate::feed::Feed>) {
    let stdout = stdout();
    let mut stdout = stdout.lock();

    s.for_each(|feed| {
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
