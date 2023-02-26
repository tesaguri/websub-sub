use std::fs;
use std::io::{self, stdout, Write};
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::Path;

use futures::{Stream, TryStreamExt};
use hyper::header::CONTENT_TYPE;
use hyper::Uri;
use tokio_stream::wrappers::{TcpListenerStream, UnixListenerStream};
use websub_sub::subscriber::Subscriber;

use crate::feed::{self, Feed};

#[derive(clap::Args)]
pub struct Opt {
    callback: Uri,
    bind: Option<String>,
}

pub async fn main(opt: Opt) -> anyhow::Result<()> {
    let client = crate::common::http_client();
    let pool = crate::common::database_pool()?;
    let _guard;

    if let Some(bind) = opt.bind.as_ref() {
        if let Some(addr) = bind.strip_prefix("tcp://") {
            let addr: SocketAddr = addr.parse()?;
            let listener = TcpListenerStream::new(tokio::net::TcpListener::bind(addr).await?);
            let subscriber = Subscriber::new(listener, opt.callback, client, pool)?;
            print_all(subscriber).await?;
        } else if let Some(path) = bind.strip_prefix("unix://") {
            let path = Path::new(path);
            let _ = fs::remove_file(path);
            let listener = UnixListenerStream::new(tokio::net::UnixListener::bind(path)?);
            _guard = crate::common::RmGuard(path);
            let subscriber = Subscriber::new(listener, opt.callback, client, pool)?;
            print_all(subscriber).await?;
        } else {
            panic!("unknown bind address type");
        }
    } else {
        let port = if let Some(p) = opt.callback.port_u16() {
            p
        } else {
            match opt.callback.scheme() {
                Some(s) if s == "https" => 443,
                Some(s) if s == "http" => 80,
                Some(s) => panic!("default port for scheme `{}` is unknown", s),
                None => panic!("missing URI scheme for callback argument"),
            }
        };
        let addr = (opt.callback.host().unwrap(), port)
            .to_socket_addrs()?
            .next()
            .unwrap();
        let listener = TcpListenerStream::new(tokio::net::TcpListener::bind(addr).await?);
        let subscriber = Subscriber::new(listener, opt.callback, client, pool)?;
        print_all(subscriber).await?;
    }

    Ok(())
}

async fn print_all(
    s: impl Stream<Item = io::Result<websub_sub::subscriber::Update<hyper::Body>>>,
) -> io::Result<()> {
    let stdout = stdout();

    s.try_for_each(|update| {
        let stdout = &stdout;
        async move {
            let mut stdout = stdout.lock();

            let mime = if let Some(v) = update.headers.get(CONTENT_TYPE) {
                if let Some(mime) = v.to_str().ok().and_then(|s| s.parse().ok()) {
                    mime
                } else {
                    writeln!(
                        stdout,
                        "Topic {}: unsupported media type `{:?}`",
                        update.topic, v
                    )
                    .unwrap();
                    return Ok(());
                }
            } else {
                feed::MediaType::Xml
            };

            let body = match hyper::body::to_bytes(update.content).await {
                Ok(body) => body,
                Err(e) => {
                    writeln!(stdout, "error reading request body: {}", e).unwrap();
                    return Ok(());
                }
            };

            let feed = if let Some(feed) = Feed::parse(mime, &body) {
                feed
            } else {
                writeln!(stdout, "failed to parse request body as feed").unwrap();
                return Ok(());
            };

            writeln!(stdout, "Feed: {} ({})", feed.title, feed.id).unwrap();
            for e in &feed.entries[..] {
                stdout.write_all(b"Entry:").unwrap();
                if let Some(ref title) = e.title {
                    write!(stdout, " {}", title).unwrap();
                }
                if let Some(ref id) = e.id {
                    write!(stdout, " (ID: {})", id).unwrap();
                }
                if let Some(ref link) = e.link {
                    write!(stdout, " (link: {})", link).unwrap();
                }
                stdout.write_all(b"\n").unwrap();
            }

            Ok(())
        }
    })
    .await
}
