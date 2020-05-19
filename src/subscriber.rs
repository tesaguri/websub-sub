mod service;
mod timer;

use std::convert::TryFrom;
use std::fmt::Debug;
use std::marker::Unpin;
use std::pin::Pin;
use std::sync::atomic::AtomicI64;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::SystemTime;

use diesel::dsl::*;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool};
use futures::channel::mpsc;
use futures::task::AtomicWaker;
use futures::{Stream, StreamExt, TryStream, TryStreamExt};
use http::Uri;
use hyper::client::connect::Connect;
use hyper::server::conn::Http;
use hyper::Client;
use mime::Mime;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::feed::Feed;
use crate::schema::*;

use service::Service;

pub struct Subscriber<I, C> {
    incoming: I,
    server: Http,
    rx: mpsc::UnboundedReceiver<(MediaType, Vec<u8>)>,
    service: Arc<service::Inner<C>>,
}

enum MediaType {
    Atom,
    Rss,
    Xml,
}

// XXX: mediocre naming
const RENEW: u64 = 10;

impl<I, C> Subscriber<I, C>
where
    I: TryStream + Unpin,
    I::Ok: AsyncRead + AsyncWrite + Send + Sync + Unpin + 'static,
    I::Error: Debug,
    C: Connect + Clone + Send + Sync + 'static,
{
    pub fn new(
        incoming: I,
        host: Uri,
        client: Client<C>,
        pool: Pool<ConnectionManager<SqliteConnection>>,
    ) -> Self {
        let expires_at = if let Some(expires_at) = expiry()
            .first::<i64>(&pool.get().unwrap())
            .optional()
            .unwrap()
        {
            expires_at
        } else {
            i64::MAX
        };

        let (tx, rx) = mpsc::unbounded();

        let service = Arc::new(service::Inner {
            host,
            client,
            pool,
            tx,
            expires_at: AtomicI64::new(expires_at),
            timer_task: AtomicWaker::new(),
        });

        let _ = tokio::spawn(timer::Timer::new(&service));

        Subscriber {
            incoming,
            server: Http::new(),
            rx,
            service,
        }
    }

    fn accept_all(&mut self, cx: &mut Context<'_>) {
        while let Poll::Ready(option) = self.incoming.try_poll_next_unpin(cx) {
            let sock = option.unwrap().unwrap();
            let service = Service {
                inner: self.service.clone(),
            };
            tokio::spawn(self.server.serve_connection(sock, service));
        }
    }
}

impl<I, C> Stream for Subscriber<I, C>
where
    I: TryStream + Unpin,
    I::Ok: AsyncRead + AsyncWrite + Send + Sync + Unpin + 'static,
    I::Error: Debug,
    C: Connect + Clone + Send + Sync + 'static,
{
    type Item = Feed;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Feed>> {
        log::trace!("Subscriber::poll_next");

        self.accept_all(cx);

        if let Poll::Ready(msg) = self.rx.poll_next_unpin(cx) {
            let (kind, body) = msg.expect("the channel was closed");
            Poll::Ready(Some(parse_feed(&body, &kind)))
        } else {
            Poll::Pending
        }
    }
}

impl std::str::FromStr for MediaType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        let mime = if let Ok(m) = s.parse::<Mime>() {
            m
        } else {
            return Err(());
        };

        if mime.type_() == mime::APPLICATION
            && mime.subtype() == "atom"
            && mime.suffix() == Some(mime::XML)
        {
            Ok(MediaType::Atom)
        } else if mime.type_() == mime::APPLICATION
            && (mime.subtype() == "rss" || mime.subtype() == "rdf")
            && mime.suffix() == Some(mime::XML)
        {
            Ok(MediaType::Rss)
        } else if (mime.type_() == mime::APPLICATION || mime.type_() == mime::TEXT)
            && mime.subtype() == mime::XML
        {
            Ok(MediaType::Xml)
        } else {
            Err(())
        }
    }
}

fn parse_feed(body: &[u8], kind: &MediaType) -> Feed {
    match kind {
        MediaType::Atom => atom::Feed::read_from(body).unwrap().into(),
        MediaType::Rss => rss::Channel::read_from(body).unwrap().into(),
        MediaType::Xml => match atom::Feed::read_from(body) {
            Ok(feed) => feed.into(),
            Err(_) => rss::Channel::read_from(body).unwrap().into(),
        },
    }
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

fn expiry() -> Order<
    Select<active_subscriptions::table, active_subscriptions::expires_at>,
    Asc<active_subscriptions::expires_at>,
> {
    active_subscriptions::table
        .select(active_subscriptions::expires_at)
        .order(active_subscriptions::expires_at.asc())
}
