use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use pin_project::pin_project;

use crate::db::{Connection, ConnectionRef, Pool};
// Import `super` as `subscriber` to make the intra doc links (and their `title` attributes) look
// nicer without hassles.
use crate::subscriber;
use crate::util::HttpService;

use super::scheduler::{self, Scheduler};
use super::Error;

/// A future that renews WebSub subscriptions of the associated [`subscriber::Service`] as the
/// expiration time of any of them comes close.
///
/// You should spawn this future onto an executor so that it can drive the renewal tasks.
#[must_use = "futures do nothing unless polled"]
#[pin_project]
pub struct RenewSubscriptions<P, S, SB, CB> {
    #[pin]
    scheduler: Scheduler<subscriber::Service<P, S, SB, CB>, Tick>,
}

struct Tick;

impl<P, S, SB, CB> RenewSubscriptions<P, S, SB, CB>
where
    P: Pool,
    P::Connection: 'static,
    S: HttpService<CB> + Clone + Send + 'static,
    S::Future: Send,
    S::ResponseBody: Send,
    S::Error: Debug + Send,
    SB: 'static,
    CB: Default + From<Vec<u8>> + Send + 'static,
{
    pub(crate) fn new(service: &Arc<subscriber::Service<P, S, SB, CB>>) -> Self {
        RenewSubscriptions {
            scheduler: Scheduler::new(service, Tick),
        }
    }
}

impl<P, S, SB, CB> Future for RenewSubscriptions<P, S, SB, CB>
where
    P: Pool,
    P::Connection: 'static,
    S: HttpService<CB> + Clone + Send + 'static,
    S::Future: Send,
    S::ResponseBody: Send,
    S::Error: Debug + Send,
    SB: 'static,
    CB: Default + From<Vec<u8>> + Send + 'static,
{
    type Output = Result<(), Error<P::Error, <P::Connection as crate::db::Connection>::Error>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().scheduler.poll(cx)
    }
}

impl<P, S, SB, CB> scheduler::Tick<subscriber::Service<P, S, SB, CB>> for Tick
where
    P: Pool,
    P::Connection: 'static,
    S: HttpService<CB> + Clone + Send + 'static,
    S::Future: Send,
    S::ResponseBody: Send,
    S::Error: Debug + Send,
    SB: 'static,
    CB: Default + From<Vec<u8>> + Send + 'static,
{
    type Error = Error<P::Error, <P::Connection as crate::db::Connection>::Error>;

    fn tick(
        &mut self,
        service: &Arc<subscriber::Service<P, S, SB, CB>>,
    ) -> Result<Option<u64>, Self::Error> {
        let mut conn = try_pool!(service.pool.get());
        try_conn!(service.renew_subscriptions(conn.as_conn_ref()));
        let expiry = try_conn!(conn.as_conn_ref().get_next_expiry());
        Ok(expiry.map(|expires_at| {
            expires_at
                .try_into()
                .map_or(0, |expires_at| service.refresh_time(expires_at))
        }))
    }
}
