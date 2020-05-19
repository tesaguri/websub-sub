use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};
use std::task::{Context, Poll};

use diesel::dsl::*;
use diesel::prelude::*;
use futures::{ready, FutureExt};
use hyper::client::connect::Connect;

use crate::schema::*;

use super::{instant_from_epoch, refresh_time, service};

pub struct Timer<C> {
    service: Weak<service::Inner<C>>,
    timer: Option<tokio::time::Delay>,
}

impl<C> Timer<C> {
    pub fn new(service: &Arc<service::Inner<C>>) -> Self {
        let timer = decode_expires_at(&service)
            .map(|expires_at| tokio::time::delay_until(refresh_time(expires_at)));
        Timer {
            service: Arc::downgrade(service),
            timer,
        }
    }
}

impl<C> Future for Timer<C>
where
    C: Connect + Clone + Send + Sync + 'static,
{
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        log::trace!("Timer::poll");

        let service = if let Some(service) = self.service.upgrade() {
            service
        } else {
            return Poll::Ready(());
        };

        service.timer_task.register(cx.waker());

        let expires_at = decode_expires_at(&service);
        let timer = if let Some(ref mut timer) = self.timer {
            if let Some(expires_at) = expires_at {
                let refresh_time = refresh_time(expires_at);
                if refresh_time < timer.deadline() {
                    timer.reset(refresh_time);
                }
            }
            timer
        } else {
            if let Some(expires_at) = expires_at {
                self.timer = Some(tokio::time::delay_until(refresh_time(expires_at)));
                self.timer.as_mut().unwrap()
            } else {
                return Poll::Pending;
            }
        };

        ready!(timer.poll_unpin(cx));

        let conn = &*service.pool.get().unwrap();
        service.renew_subscriptions(conn);

        let renewing = renewing_subscriptions::table.select(renewing_subscriptions::old);
        if let Some(expires_at) = super::expiry()
            .filter(not(active_subscriptions::id.eq_any(renewing)))
            .first::<i64>(conn)
            .optional()
            .unwrap()
        {
            service.expires_at.store(expires_at, Ordering::SeqCst);
            let refresh_time = refresh_time(instant_from_epoch(expires_at));
            timer.reset(refresh_time);
        } else {
            service.expires_at.store(i64::MAX, Ordering::SeqCst);
            self.timer = None;
        }

        Poll::Pending
    }
}

fn decode_expires_at<C>(service: &Arc<service::Inner<C>>) -> Option<tokio::time::Instant> {
    let val = service.expires_at.load(Ordering::SeqCst);
    if val == i64::MAX {
        None
    } else {
        Some(instant_from_epoch(val))
    }
}
