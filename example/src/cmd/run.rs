use std::fs;
use std::io::{self, stdout, Write};
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::Path;

use futures::{future, Stream, TryStreamExt};
use hyper::Uri;
use structopt::StructOpt;
use tokio_stream::wrappers::{TcpListenerStream, UnixListenerStream};

use websub_sub::subscriber::Subscriber;

#[derive(StructOpt)]
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
            let subscriber = Subscriber::new(listener, opt.callback, client, pool);
            print_all(subscriber).await?;
        } else if let Some(path) = bind.strip_prefix("unix://") {
            let path = Path::new(path);
            let _ = fs::remove_file(path);
            let listener = UnixListenerStream::new(tokio::net::UnixListener::bind(path)?);
            _guard = crate::common::RmGuard(path);
            let subscriber = Subscriber::new(listener, opt.callback, client, pool);
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
        let subscriber = Subscriber::new(listener, opt.callback, client, pool);
        print_all(subscriber).await?;
    }

    Ok(())
}

async fn print_all(
    s: impl Stream<Item = io::Result<(String, websub_sub::feed::Feed)>>,
) -> io::Result<()> {
    let stdout = stdout();
    let mut stdout = stdout.lock();

    s.try_for_each(|(_topic, feed)| {
        let result = (|| {
            writeln!(stdout, "Feed: {} ({})", feed.title, feed.id)?;
            for e in feed.entries {
                stdout.write_all(b"Entry:")?;
                if let Some(title) = e.title {
                    write!(stdout, " {}", title)?;
                }
                if let Some(id) = e.id {
                    write!(stdout, " (ID: {})", id)?;
                }
                if let Some(link) = e.link {
                    write!(stdout, " (link: {})", link)?;
                }
                stdout.write_all(b"\n")?;
            }
            Ok(())
        })();
        future::ready(result)
    })
    .await
}
