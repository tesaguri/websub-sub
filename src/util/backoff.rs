use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use pin_project::pin_project;
use tokio::time::Sleep;

/// A future to perform exponential backoff.
#[pin_project]
pub struct Backoff {
    #[pin]
    inner: Option<Inner>,
}

#[pin_project]
struct Inner {
    #[pin]
    sleep: Sleep,
    wait: u64,
}

const INITIAL_WAIT: u64 = 1;

impl Backoff {
    pub fn new() -> Self {
        Backoff { inner: None }
    }

    /// Get the length of the time period this `Backoff` is/was waiting for.
    ///
    /// It is set to `0` in the initial state.
    pub fn wait_secs(&self) -> u64 {
        self.inner.as_ref().map_or(0, |inner| inner.wait)
    }

    /// Increases the value of [`wait_secs`] of the backoff.
    ///
    /// If the current value is non-zero, it is doubled, and is set to `1` if zero.
    ///
    /// On first call after `Backoff::new`, the task associated with `cx` is scheduled to be
    /// notified when the backlff has completed.
    pub fn increase(self: Pin<&mut Self>, cx: &mut Context<'_>) {
        let mut inner = self.project().inner;
        if let Some(inner) = inner.as_mut().as_pin_mut() {
            let inner = inner.project();
            let current_deadline = if *inner.wait == 0 {
                *inner.wait = INITIAL_WAIT;
                super::instant_now()
            } else {
                *inner.wait *= 2;
                inner.sleep.deadline().into_std()
            };
            inner
                .sleep
                .reset((current_deadline + Duration::from_secs(*inner.wait)).into());
        } else {
            inner.as_mut().set(Some(Inner {
                sleep: tokio::time::sleep(Duration::from_secs(INITIAL_WAIT)),
                wait: INITIAL_WAIT,
            }));
            // Register the task
            let _ = inner.as_pin_mut().unwrap().project().sleep.poll(cx);
        }
    }

    /// Resets a completed `Backoff` to the initial state.
    pub fn reset(self: Pin<&mut Self>) {
        if let Some(inner) = self.project().inner.as_pin_mut() {
            *inner.project().wait = 0;
        }
    }
}

impl Future for Backoff {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project().inner.as_pin_mut() {
            Some(inner) if inner.wait > 0 => inner.project().sleep.poll(cx),
            _ => Poll::Ready(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future;

    use crate::util::{self, EitherUnwrapExt};

    use super::*;

    async fn increase(mut backoff: Pin<&mut Backoff>) {
        future::poll_fn(move |cx| {
            backoff.as_mut().increase(cx);
            Poll::Ready(())
        })
        .await;
    }

    #[tokio::test]
    async fn it_works() {
        tokio::time::pause();

        let backoff = Backoff::new();
        tokio::pin!(backoff);

        for _ in 0..2 {
            // The timer should be set on the first `increase()` regardless of the duration of time
            // elapsed since the construction.
            tokio::time::advance(Duration::from_secs(42)).await;

            assert_eq!(backoff.wait_secs(), 0);
            // Initially, it should complete immidiately
            util::first(backoff.as_mut(), future::ready(()))
                .await
                .unwrap_left();

            increase(backoff.as_mut()).await;
            assert_eq!(backoff.wait_secs(), 1);
            // It should not complete right after `increase`-ing
            util::first(backoff.as_mut(), future::ready(()))
                .await
                .unwrap_right();

            tokio::time::advance(Duration::from_secs(1)).await;
            // Advance 2 ms more because `Sleep`'s precision is of 1 ms
            tokio::time::advance(Duration::from_millis(2)).await;
            // It should complete after the backoff period has elapsed
            util::first(backoff.as_mut(), future::ready(()))
                .await
                .unwrap_left();

            increase(backoff.as_mut()).await;
            assert_eq!(backoff.wait_secs(), 2);
            util::first(backoff.as_mut(), future::ready(()))
                .await
                .unwrap_right();
            tokio::time::advance(Duration::from_secs(1)).await;
            // Polling too early
            util::first(backoff.as_mut(), future::ready(()))
                .await
                .unwrap_right();
            tokio::time::advance(Duration::from_secs(1)).await;
            util::first(backoff.as_mut(), future::ready(()))
                .await
                .unwrap_left();

            increase(backoff.as_mut()).await;
            // It should *exponentially* backoff
            assert_eq!(backoff.wait_secs(), 4);
            util::first(backoff.as_mut(), future::ready(()))
                .await
                .unwrap_right();
            tokio::time::advance(Duration::from_secs(2)).await;
            util::first(backoff.as_mut(), future::ready(()))
                .await
                .unwrap_right();
            tokio::time::advance(Duration::from_secs(2)).await;
            util::first(backoff.as_mut(), future::ready(()))
                .await
                .unwrap_left();

            println!("Resetting `backoff`");
            backoff.as_mut().reset();

            // It should work the same way as the above after `reset`-ing
        }
    }
}
