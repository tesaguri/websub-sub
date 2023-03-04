macro_rules! try_pool {
    ($result:expr $(,)?) => {
        match $result {
            Ok(x) => x,
            Err(e) => return Err($crate::subscriber::Error::Pool(e)),
        }
    };
}

macro_rules! try_conn {
    ($result:expr $(,)?) => {
        match $result {
            Ok(x) => x,
            Err(e) => return Err($crate::subscriber::Error::Connection(e)),
        }
    };
}

pub mod update;

mod renew_subscriptions;
mod scheduler;
mod service;

pub use self::renew_subscriptions::RenewSubscriptions;
pub use self::service::Service;
pub use self::update::Update;

use std::convert::Infallible;
use std::fmt::Debug;
use std::future::{self, Future};
use std::marker::{PhantomData, Unpin};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{ready, Context, Poll};
use std::time::Duration;

use futures::channel::mpsc;
use futures::{FutureExt, Stream, StreamExt, TryStream};
use http::uri::{PathAndQuery, Uri};
use http_body::Body;
use hyper::server::conn::Http;
use pin_project::pin_project;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::db::{Connection, Pool};
use crate::util::{empty_response, Backoff, HttpService};

/// A WebSub subscriber server.
#[pin_project]
pub struct Subscriber<P, S, B, I> {
    #[pin]
    incoming: I,
    backoff: Pin<Box<Backoff>>,
    server: Http,
    rx: mpsc::UnboundedReceiver<Update<hyper::Body>>,
    service: Arc<Service<P, S, hyper::Body, B>>,
}

#[derive(Clone, Debug)]
pub struct Builder {
    renewal_margin: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum Error<PE, CE> {
    #[error("failed establish a database connection")]
    Pool(#[source] PE),
    #[error("database connection failed")]
    Connection(#[source] CE),
}

impl Subscriber<(), (), (), ()> {
    pub fn builder() -> Builder {
        Builder::new()
    }
}

impl<P, S, B, I> Subscriber<P, S, B, I>
where
    P: Pool,
    P::Error: Debug,
    <P::Connection as Connection>::Error: Debug,
    S: HttpService<B> + Clone + Send + Sync + 'static,
    S::Error: Debug + Send,
    S::Future: Send,
    S::ResponseBody: Send,
    <S::ResponseBody as Body>::Error: Debug,
    B: Default + From<Vec<u8>> + Send + 'static,
    I: TryStream,
    I::Ok: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    #[allow(clippy::type_complexity)]
    pub fn new(
        incoming: I,
        callback: Uri,
        client: S,
        pool: P,
    ) -> Result<Self, Error<P::Error, <P::Connection as Connection>::Error>> {
        let (subscriber, renew_subscriptions) =
            Subscriber::builder().build(incoming, callback, client, pool)?;
        let renew_subscriptions = renew_subscriptions.map(|result| match result {
            Ok(()) => {}
            Err(e) => {
                log::error!("error while renewing a subscription: {:?}", e);
            }
        });
        tokio::spawn(renew_subscriptions);
        Ok(subscriber)
    }

    fn accept_all(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), I::Error>> {
        const MAX_BACKOFF: u64 = 2 << 6;

        let mut this = self.project();

        ready!(this.backoff.as_mut().poll(cx));

        while let Poll::Ready(result) = this.incoming.as_mut().try_poll_next(cx) {
            match result {
                None => return Poll::Ready(Ok(())),
                Some(Err(e)) => {
                    if this.backoff.wait_secs() >= MAX_BACKOFF {
                        return Poll::Ready(Err(e));
                    }
                    // Try to recover from the error
                    this.backoff.as_mut().increase(cx);
                    return Poll::Pending;
                }
                Some(Ok(sock)) => {
                    this.backoff.as_mut().reset();

                    let service = this.service.clone();
                    let service = tower::service_fn(move |req| match (*service).call(req) {
                        Ok(ret) => future::ready(Ok::<_, Infallible>(ret)),
                        Err(e) => {
                            log::error!("error while serving an HTTP request: {:?}", e);
                            future::ready(Ok(empty_response(
                                http::StatusCode::INTERNAL_SERVER_ERROR,
                            )))
                        }
                    });

                    tokio::spawn(this.server.serve_connection(sock, service));
                }
            }
        }

        Poll::Pending
    }
}

impl<P, S, B, I> Subscriber<P, S, B, I> {
    pub fn service(&self) -> &Arc<Service<P, S, hyper::Body, B>> {
        &self.service
    }
}

/// The `Stream` impl yields topic updates that the server has received.
impl<P, S, B, I> Stream for Subscriber<P, S, B, I>
where
    P: Pool,
    P::Error: Debug,
    <P::Connection as Connection>::Error: Debug,
    S: HttpService<B> + Clone + Send + Sync + 'static,
    S::Error: Debug + Send,
    S::Future: Send,
    S::ResponseBody: Send,
    <S::ResponseBody as Body>::Error: Debug,
    B: Default + From<Vec<u8>> + Send + 'static,
    I: TryStream,
    I::Ok: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Item = Result<Update<hyper::Body>, I::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        log::trace!("Subscriber::poll_next");

        let incoming_done = matches!(self.as_mut().accept_all(cx)?, Poll::Ready(()));

        match self.as_mut().project().rx.poll_next_unpin(cx) {
            Poll::Ready(option) => Poll::Ready(option.map(Ok)),
            Poll::Pending if incoming_done && Arc::strong_count(&self.service) == 1 => {
                Poll::Ready(None)
            }
            // TODO: Notify the task when `strong_count` reaches `1`.
            // Though I'm optimistic that this won't affect production code since the real
            // `TcpListener` won't terminate except in a fatal error, which is propagated above.
            // This may affect mocking codes however.
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Builder {
    pub fn new() -> Self {
        Self {
            renewal_margin: Duration::from_secs(3600),
        }
    }

    pub fn renewal_margin(&mut self, renewal_margin: Duration) -> &mut Self {
        self.renewal_margin = renewal_margin;
        self
    }

    #[allow(clippy::type_complexity)]
    pub fn build<P, S, B, I>(
        &self,
        incoming: I,
        callback: Uri,
        client: S,
        pool: P,
    ) -> Result<
        (
            Subscriber<P, S, B, I>,
            RenewSubscriptions<P, S, hyper::Body, B>,
        ),
        Error<P::Error, <P::Connection as Connection>::Error>,
    >
    where
        P: Pool,
        P::Error: Debug,
        <P::Connection as Connection>::Error: Debug,
        S: HttpService<B> + Clone + Send + Sync + 'static,
        S::Error: Debug + Send,
        S::Future: Send,
        S::ResponseBody: Send,
        <S::ResponseBody as Body>::Error: Debug,
        B: Default + From<Vec<u8>> + Send + 'static,
        I: TryStream,
        I::Ok: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        let (service, rx, renew_subscriptions) = self.build_service(callback, client, pool)?;
        let subscriber = Subscriber {
            incoming,
            backoff: Box::pin(Backoff::new()),
            server: Http::new(),
            rx,
            service,
        };
        Ok((subscriber, renew_subscriptions))
    }

    #[allow(clippy::type_complexity)]
    pub fn build_service<P, S, SB, CB>(
        &self,
        callback: Uri,
        client: S,
        pool: P,
    ) -> Result<
        (
            Arc<Service<P, S, SB, CB>>,
            mpsc::UnboundedReceiver<Update<SB>>,
            RenewSubscriptions<P, S, SB, CB>,
        ),
        Error<P::Error, <P::Connection as Connection>::Error>,
    >
    where
        P: Pool,
        P::Error: Debug,
        <P::Connection as Connection>::Error: Debug,
        S: HttpService<CB> + Clone + Send + Sync + 'static,
        S::Error: Debug + Send,
        S::Future: Send,
        S::ResponseBody: Send,
        <S::ResponseBody as Body>::Error: Debug,
        SB: Send + 'static,
        CB: Default + From<Vec<u8>> + Send + 'static,
    {
        let renewal_margin = self.renewal_margin.as_secs();

        let first_tick = try_conn!(try_pool!(pool.get()).get_next_expiry()).map(|expires_at| {
            u64::try_from(expires_at)
                .unwrap_or(0)
                .saturating_sub(renewal_margin)
        });

        let callback = prepare_callback_prefix(callback);

        let (tx, rx) = mpsc::unbounded();

        let service = Arc::new(service::Service {
            callback,
            renewal_margin,
            client,
            pool,
            tx,
            handle: scheduler::Handle::new(first_tick),
            marker: PhantomData,
        });

        let renew_subscriptions = RenewSubscriptions::new(&service);

        Ok((service, rx, renew_subscriptions))
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

// Ensure that `prefix` ends with a slash.
fn prepare_callback_prefix(prefix: Uri) -> Uri {
    // Existence of `path_and_query` should have been verified upon deserialization.
    let path = prefix.path_and_query().unwrap().as_str();

    if path.ends_with('/') {
        prefix
    } else {
        let mut buf = String::with_capacity(path.len() + 1);
        buf.push_str(path);
        buf.push('/');
        let path = PathAndQuery::try_from(buf).unwrap();
        let mut parts = prefix.into_parts();
        parts.path_and_query = Some(path);
        Uri::from_parts(parts).unwrap()
    }
}

#[cfg(all(test, feature = "diesel2", feature = "sha-1"))]
mod tests {
    use std::convert::Infallible;
    use std::str;
    use std::time::Duration;

    use bytes::Bytes;
    use diesel::dsl::*;
    use diesel::prelude::*;
    use diesel::r2d2::ConnectionManager;
    use diesel::SqliteConnection;
    use diesel_migrations::{FileBasedMigrations, MigrationHarness};
    use futures::channel::oneshot;
    use futures::future;
    use hmac::{Hmac, Mac};
    use http::header::CONTENT_TYPE;
    use http::Uri;
    use http::{Request, Response, StatusCode};
    use http_body::Full;
    use hyper::server::conn::Http;
    use hyper::service;
    use sha1::Sha1;

    use crate::db::Pool as _;
    use crate::hub;
    use crate::schema::*;
    use crate::util::connection::{Connector, Listener};
    use crate::util::consts::{
        APPLICATION_ATOM_XML, APPLICATION_WWW_FORM_URLENCODED, HUB_SIGNATURE,
    };
    use crate::util::{self, EitherUnwrapExt, FutureTimeoutExt};

    use super::*;

    type Body = Full<Bytes>;
    type Client = hyper::Client<Connector, Body>;
    type Pool = crate::db::diesel2::Pool<
        ConnectionManager<SqliteConnection>,
        subscriptions::id,
        subscriptions::hub,
        subscriptions::topic,
        subscriptions::secret,
        subscriptions::expires_at,
    >;

    const TOPIC: &str = "http://example.com/feed.xml";
    const HUB: &str = "http://example.com/hub";
    #[allow(clippy::declare_interior_mutable_const)]
    const CONTENT: Bytes = Bytes::from_static(b"<feed><title>Example feed</title></feed>");
    const MARGIN: Duration = Duration::from_secs(42);
    /// Network delay for communications between the subscriber and the hub.
    const DELAY: Duration = Duration::from_millis(1);

    #[test]
    fn callback_prefix() {
        // Should remain intact (root directory).
        let uri = prepare_callback_prefix("http://example.com/".try_into().unwrap());
        assert_eq!(uri, "http://example.com/");

        // Should remain intact (subdirectory).
        let uri = prepare_callback_prefix("http://example.com/websub/".try_into().unwrap());
        assert_eq!(uri, "http://example.com/websub/");

        // Should append a slash (root directory).
        let uri = prepare_callback_prefix("http://example.com".try_into().unwrap());
        assert_eq!(uri, "http://example.com/");

        // Should append a slash (subdirectory).
        let uri = prepare_callback_prefix("http://example.com/websub".try_into().unwrap());
        assert_eq!(uri, "http://example.com/websub/");
    }

    #[tokio::test]
    async fn renew_multi_subs() {
        tokio::time::pause();

        let begin = i64::try_from(util::now_unix().as_secs()).unwrap();

        let pool = diesel::r2d2::Pool::builder()
            .max_size(1)
            .build(ConnectionManager::<SqliteConnection>::new(":memory:"))
            .unwrap();
        let mut conn = pool.get().unwrap();
        run_migrations(&mut conn);

        let expiry1 = begin + MARGIN.as_secs() as i64 + 1;
        let expiry2 = expiry1 + MARGIN.as_secs() as i64;
        let values = [
            (
                subscriptions::hub.eq(HUB),
                subscriptions::topic.eq("http://example.com/topic/1"),
                subscriptions::secret.eq("secret1"),
                subscriptions::expires_at.eq(expiry1),
            ),
            (
                subscriptions::hub.eq(HUB),
                subscriptions::topic.eq("http://example.com/topic/2"),
                subscriptions::secret.eq("secret2"),
                subscriptions::expires_at.eq(expiry2),
            ),
        ];
        insert_into(subscriptions::table)
            .values(&values[..])
            .execute(&mut *conn)
            .unwrap();

        drop(conn);

        let (mut subscriber, client, listener) = prepare_subscriber_with_pool(Pool::from(pool));
        let mut listener = tokio_test::task::spawn(listener);

        let hub = Http::new();

        listener.enter(|cx, listener| assert!(listener.poll_next(cx).is_pending()));

        // Renewal of first subscription.

        tokio::time::advance(Duration::from_secs(2)).await;

        // Subscriber should re-subscribe to the topic.
        let form = accept_request(&mut listener, &hub, "/hub").timeout().await;
        let callback1 = match form {
            hub::Form::Subscribe {
                callback, topic, ..
            } => {
                assert_eq!(topic, "http://example.com/topic/1");
                callback
            }
            _ => panic!(),
        };
        listener.enter(|cx, listener| assert!(listener.poll_next(cx).is_pending()));

        // Intent verification of the re-subscription.
        let task = verify_intent(
            &client,
            &callback1,
            &hub::Verify::Subscribe {
                topic: "http://example.com/topic/1",
                challenge: "subscription_challenge1",
                // The subscription should never be renewed again in this test.
                lease_seconds: 2 * MARGIN.as_secs() + 2,
            },
        );
        util::first(task.timeout(), subscriber.next())
            .await
            .unwrap_left();

        // Subscriber should unsubscribe from the old subscription.
        let id = crate::util::callback_id::encode(&1_i64.to_le_bytes()).to_string();
        let old_callback1 = Uri::try_from(format!("http://example.com/{}", id)).unwrap();
        let form = accept_request(&mut listener, &hub, "/hub").timeout().await;
        match form {
            hub::Form::Unsubscribe {
                callback, topic, ..
            } => {
                assert_eq!(callback, old_callback1);
                assert_eq!(topic, "http://example.com/topic/1");
            }
            _ => panic!(),
        }
        listener.enter(|cx, listener| assert!(listener.poll_next(cx).is_pending()));

        // Intent verification of the unsubscription.
        let task = verify_intent(
            &client,
            &old_callback1,
            &hub::Verify::Unsubscribe {
                topic: "http://example.com/topic/1",
                challenge: "unsubscription_challenge1",
            },
        );
        util::first(task.timeout(), subscriber.next())
            .await
            .unwrap_left();

        // Renewal of second subscription.

        tokio::time::advance(MARGIN).await;

        let form = accept_request(&mut listener, &hub, "/hub").timeout().await;
        let callback2 = match form {
            hub::Form::Subscribe {
                callback, topic, ..
            } => {
                assert_eq!(topic, "http://example.com/topic/2");
                callback
            }
            _ => panic!(),
        };
        listener.enter(|cx, listener| assert!(listener.poll_next(cx).is_pending()));

        let task = verify_intent(
            &client,
            &callback2,
            &hub::Verify::Subscribe {
                topic: "http://example.com/topic/2",
                challenge: "subscription_challenge2",
                lease_seconds: MARGIN.as_secs() + 1,
            },
        );
        util::first(task.timeout(), subscriber.next())
            .await
            .unwrap_left();

        let id = crate::util::callback_id::encode(&2_i64.to_le_bytes()).to_string();
        let old_callback2 = Uri::try_from(format!("http://example.com/{}", id)).unwrap();
        let form = accept_request(&mut listener, &hub, "/hub").timeout().await;
        match form {
            hub::Form::Unsubscribe {
                callback, topic, ..
            } => {
                assert_eq!(callback, old_callback2);
                assert_eq!(topic, "http://example.com/topic/2");
            }
            _ => panic!(),
        }
        listener.enter(|cx, listener| assert!(listener.poll_next(cx).is_pending()));

        let task = verify_intent(
            &client,
            &old_callback1,
            &hub::Verify::Unsubscribe {
                topic: "http://example.com/topic/2",
                challenge: "unsubscription_challenge2",
            },
        );
        util::first(task.timeout(), subscriber.next())
            .await
            .unwrap_left();
    }

    /// Go through the entire protocol flow at once.
    #[tokio::test]
    async fn entire_flow() {
        tokio::time::pause();

        let (subscriber, client, listener) = prepare_subscriber();
        let mut subscriber = tokio_test::task::spawn(subscriber);
        let mut listener = tokio_test::task::spawn(listener);

        let hub = Http::new();
        listener.enter(|cx, listener| assert!(listener.poll_next(cx).is_pending()));

        // Subscribe to the topic.

        let req_task = subscriber
            .service
            .subscribe(
                HUB.to_owned(),
                TOPIC.to_owned(),
                &mut subscriber.service.pool.get().unwrap(),
            )
            .unwrap();
        tokio::time::advance(DELAY).await;
        let accept_task = accept_request(&mut listener, &hub, "/hub");
        let (result, form) = future::join(req_task, accept_task).timeout().await;
        result.unwrap();
        let (callback, topic, secret) = match form {
            hub::Form::Subscribe {
                callback,
                topic,
                secret,
            } => (callback, topic, secret),
            _ => panic!(),
        };
        assert_eq!(topic, TOPIC);
        listener.enter(|cx, listener| assert!(listener.poll_next(cx).is_pending()));

        // Hub verifies intent of the subscriber.

        let task = verify_intent(
            &client,
            &callback,
            &hub::Verify::Subscribe {
                topic: TOPIC,
                challenge: "subscription_challenge",
                lease_seconds: (2 * MARGIN).as_secs(),
            },
        );
        util::first(task.timeout(), subscriber.next())
            .await
            .unwrap_left();

        // Content distribution.

        let mut mac = Hmac::<Sha1>::new_from_slice(secret.as_bytes()).unwrap();
        #[allow(clippy::borrow_interior_mutable_const)]
        mac.update(&CONTENT);
        let signature = mac.finalize().into_bytes();
        let req = Request::post(&callback)
            .header(CONTENT_TYPE, APPLICATION_ATOM_XML)
            .header(HUB_SIGNATURE, format!("sha1={}", hex::encode(&*signature)))
            .body(Full::new(CONTENT))
            .unwrap();
        let task = client.request(req);
        let (res, update) = future::join(task, subscriber.next()).timeout().await;

        let res = res.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let update = update.unwrap().unwrap();
        assert_eq!(*update.topic, *TOPIC);
        let body = hyper::body::to_bytes(update.content).await.unwrap();
        assert_eq!(body, {
            #[allow(clippy::borrow_interior_mutable_const)]
            CONTENT
        },);

        // Subscription renewal.

        tokio::time::advance(MARGIN).await;

        tokio::time::advance(DELAY).await;
        let task = accept_request(&mut listener, &hub, "/hub").timeout();
        let form = util::first(task, subscriber.next()).await.unwrap_left();
        let (new_callback, topic) = match form {
            hub::Form::Subscribe {
                callback,
                topic,
                secret: _,
            } => (callback, topic),
            _ => panic!(),
        };
        assert_ne!(new_callback, callback);
        assert_eq!(topic, TOPIC);
        listener.enter(|cx, listener| assert!(listener.poll_next(cx).is_pending()));

        // Hub verifies intent of the subscriber.

        let task = verify_intent(
            &client,
            &new_callback,
            &hub::Verify::Subscribe {
                topic: TOPIC,
                challenge: "renewal_challenge",
                lease_seconds: (2 * MARGIN).as_secs(),
            },
        );
        util::first(task.timeout(), subscriber.next())
            .await
            .unwrap_left();

        // `subscriber` should unsubscribe from the old subscription.

        tokio::time::advance(DELAY).await;
        let task = accept_request(&mut listener, &hub, "/hub").timeout();
        let form = util::first(task, subscriber.next()).await.unwrap_left();
        let (unsubscribed, topic) = match form {
            hub::Form::Unsubscribe { callback, topic } => (callback, topic),
            _ => panic!(),
        };
        assert_eq!(unsubscribed, callback);
        assert_eq!(topic, TOPIC);
        // FIXME: The listener unexpectedly receives a subscription request.
        listener.enter(|cx, listener| assert!(listener.poll_next(cx).is_pending()));

        // Hub verifies intent of the subscriber.

        let task = verify_intent(
            &client,
            &callback,
            &hub::Verify::Unsubscribe {
                topic: TOPIC,
                challenge: "unsubscription_challenge",
            },
        );
        util::first(task.timeout(), subscriber.next())
            .await
            .unwrap_left();

        // Subscriber should have processed all requests.

        subscriber.enter(|cx, subscriber| assert!(subscriber.poll_next(cx).is_pending()));

        // Old subscription should be deleted from the database.

        let id = callback.path().rsplit('/').next().unwrap();
        let id = crate::util::callback_id::decode(id).unwrap() as i64;
        let mut conn = subscriber.service.pool.get().unwrap();

        let row = subscriptions::table.find(id);
        assert!(!select(exists(row))
            .get_result::<bool>(conn.as_mut())
            .unwrap());
    }

    fn prepare_subscriber() -> (Subscriber<Pool, Client, Body, Listener>, Client, Listener) {
        let pool = diesel::r2d2::Pool::builder()
            .max_size(1)
            .build(ConnectionManager::new(":memory:"))
            .unwrap();
        run_migrations(&mut pool.get().unwrap());
        prepare_subscriber_with_pool(Pool::from(pool))
    }

    fn prepare_subscriber_with_pool(
        pool: Pool,
    ) -> (Subscriber<Pool, Client, Body, Listener>, Client, Listener) {
        let (hub_conn, sub_listener) = util::connection();
        let (sub_conn, hub_listener) = util::connection();
        let sub_client = hyper::Client::builder()
            // https://github.com/hyperium/hyper/issues/2312#issuecomment-722125137
            .pool_idle_timeout(Duration::from_secs(0))
            .pool_max_idle_per_host(0)
            .build::<_, Body>(sub_conn);
        let hub_client = hyper::Client::builder()
            .pool_idle_timeout(Duration::from_secs(0))
            .pool_max_idle_per_host(0)
            .build::<_, Body>(hub_conn);

        let (subscriber, renew_subscriptions) = Subscriber::builder()
            .renewal_margin(MARGIN)
            .build(
                sub_listener,
                Uri::from_static("http://example.com/"),
                sub_client,
                pool,
            )
            .unwrap();
        tokio::spawn(renew_subscriptions);

        (subscriber, hub_client, hub_listener)
    }

    fn run_migrations(conn: &mut SqliteConnection) {
        let source = FileBasedMigrations::find_migrations_directory().unwrap();
        conn.run_pending_migrations(source).unwrap();
    }

    async fn accept_request<'a>(
        listener: &'a mut Listener,
        http: &'a Http,
        path: &'static str,
    ) -> hub::Form {
        let (tx, rx) = oneshot::channel();
        let mut tx = Some(tx);

        let sock = listener.next().await;
        http.serve_connection(
            sock.unwrap().unwrap(),
            service::service_fn(move |req| {
                let tx = tx.take().unwrap();
                #[allow(clippy::borrow_interior_mutable_const)]
                async move {
                    assert_eq!(req.uri().path_and_query().unwrap(), path);
                    assert_eq!(
                        req.headers().get(CONTENT_TYPE).unwrap(),
                        APPLICATION_WWW_FORM_URLENCODED
                    );
                    let body = hyper::body::to_bytes(req).await.unwrap();
                    let form: hub::Form = serde_urlencoded::from_bytes(&body).unwrap();
                    tx.send(form).unwrap();

                    let res = Response::builder()
                        .status(StatusCode::ACCEPTED)
                        .body(http_body::Empty::<Bytes>::new())
                        .unwrap();
                    tokio::time::advance(DELAY).await;
                    Ok::<_, Infallible>(res)
                }
            }),
        )
        .await
        .unwrap();
        rx.await.unwrap()
    }

    fn verify_intent<'a>(
        client: &Client,
        callback: &Uri,
        query: &hub::Verify<&'a str>,
    ) -> impl std::future::Future<Output = ()> + 'a {
        let challenge = match *query {
            hub::Verify::Subscribe { challenge, .. }
            | hub::Verify::Unsubscribe { challenge, .. } => challenge,
        };
        let query = serde_urlencoded::to_string(query).unwrap();
        let res = client.get(format!("{}?{}", callback, query).try_into().unwrap());
        async move {
            let res = res.await.unwrap();
            assert_eq!(res.status(), StatusCode::OK);
            let body = hyper::body::to_bytes(res).timeout().await.unwrap();
            assert_eq!(body, challenge.as_bytes());
        }
    }
}
