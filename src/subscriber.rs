mod service;

use std::convert::{TryFrom, TryInto};
use std::fmt::Debug;
use std::marker::Unpin;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::SystemTime;

use diesel::dsl::*;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool};
use futures::channel::mpsc;
use futures::{FutureExt, Stream, StreamExt, TryStream, TryStreamExt};
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
    timer: Option<tokio::time::Delay>,
    rx: mpsc::UnboundedReceiver<Msg>,
    service: Arc<service::Inner<C>>,
}

enum Msg {
    Content((MediaType, Vec<u8>)),
    UpdateTimer(tokio::time::Instant),
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
        let timer = if let Some(expires_at) = expiry()
            .first::<i64>(&pool.get().unwrap())
            .optional()
            .unwrap()
        {
            let refresh_time = refresh_time(instant_from_epoch(expires_at));
            Some(tokio::time::delay_until(refresh_time))
        } else {
            None
        };

        let (tx, rx) = mpsc::unbounded();

        let service = Arc::new(service::Inner {
            host,
            client,
            pool,
            tx,
        });

        Subscriber {
            incoming,
            server: Http::new(),
            timer,
            rx,
            service,
        }
    }

    fn renew_subscriptions(&mut self, cx: &mut Context<'_>) {
        let timer = if let Some(ref mut timer) = self.timer {
            timer
        } else {
            return;
        };

        if timer.poll_unpin(cx).is_pending() {
            return;
        }

        let now_epoch = now_epoch();
        let threshold: i64 = (now_epoch + RENEW).try_into().unwrap();

        let renewing = renewing_subscriptions::table.select(renewing_subscriptions::old);
        let conn = self.service.pool.get().unwrap();
        let expiring = subscriptions::table
            .inner_join(active_subscriptions::table)
            .select((subscriptions::id, subscriptions::hub, subscriptions::topic))
            .filter(active_subscriptions::expires_at.le(threshold))
            .filter(not(subscriptions::id.eq_any(renewing)))
            .load::<(i64, String, String)>(&conn)
            .unwrap();

        log::info!("Renewing {} expiring subscription(s)", expiring.len());

        for (id, hub, topic) in expiring {
            tokio::spawn(self.service.renew(id, &hub, &topic, &conn));
        }

        if let Some(expires_at) = expiry().first::<i64>(&conn).optional().unwrap() {
            let refresh_time = refresh_time(instant_from_epoch(expires_at));
            timer.reset(refresh_time);
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

        self.renew_subscriptions(cx);
        self.accept_all(cx);

        while let Poll::Ready(msg) = self.rx.poll_next_unpin(cx) {
            match msg.expect("the channel was closed") {
                Msg::Content((kind, body)) => return Poll::Ready(Some(parse_feed(&body, &kind))),
                Msg::UpdateTimer(expires_at) => {
                    let refresh_time = refresh_time(expires_at);
                    if let Some(ref mut timer) = self.timer {
                        if refresh_time < timer.deadline() {
                            timer.reset(refresh_time);
                        }
                    } else {
                        self.timer = Some(tokio::time::delay_until(refresh_time));
                    }
                }
            }
        }

        Poll::Pending
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
