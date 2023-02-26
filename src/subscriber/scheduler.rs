use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use futures::ready;
use futures::task::AtomicWaker;
use pin_project::pin_project;

use crate::util;

/// A `Future` that executes the specified function in a scheduled manner.
#[pin_project]
pub struct Scheduler<H, T> {
    handle: Weak<H>,
    #[pin]
    sleep: Option<tokio::time::Sleep>,
    tick: T,
}

pub struct Handle {
    next_tick: AtomicU64,
    task: AtomicWaker,
}

pub trait Tick<T> {
    type Error;

    fn tick(&mut self, handle: &Arc<T>) -> Result<Option<u64>, Self::Error>;
}

impl<T, F, E> Tick<T> for F
where
    F: FnMut(&Arc<T>) -> Result<Option<u64>, E>,
{
    type Error = E;

    fn tick(&mut self, handle: &Arc<T>) -> Result<Option<u64>, E> {
        self(handle)
    }
}

impl<H: AsRef<Handle>, T: Tick<H>> Scheduler<H, T> {
    pub fn new(handle: &Arc<H>, tick: T) -> Self {
        Scheduler {
            sleep: (**handle)
                .as_ref()
                .decode_next_tick()
                .map(|next_tick| tokio::time::sleep_until(next_tick.into())),
            handle: Arc::downgrade(handle),
            tick,
        }
    }
}

impl<H: AsRef<Handle>, T: Tick<H>> Future for Scheduler<H, T> {
    type Output = Result<(), T::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        log::trace!("Scheduler::<H, T>::poll");

        let mut this = self.project();

        let t = if let Some(t) = this.handle.upgrade() {
            t
        } else {
            return Poll::Ready(Ok(()));
        };
        let handle = (*t).as_ref();

        handle.task.register(cx.waker());

        let mut sleep = if let Some(mut sleep) = this.sleep.as_mut().as_pin_mut() {
            if let Some(next_tick) = handle.decode_next_tick() {
                if next_tick < sleep.deadline().into_std() {
                    sleep.as_mut().reset(next_tick.into());
                }
            }
            sleep
        } else if let Some(next_tick) = handle.decode_next_tick() {
            this.sleep
                .as_mut()
                .set(Some(tokio::time::sleep_until(next_tick.into())));
            this.sleep.as_mut().as_pin_mut().unwrap()
        } else {
            return Poll::Pending;
        };

        ready!(sleep.as_mut().poll(cx));

        if let Some(next_tick) = this.tick.tick(&t)? {
            handle.next_tick.store(next_tick, Ordering::Relaxed);
            let next_tick = util::instant_from_unix(Duration::from_secs(next_tick));
            sleep.reset(next_tick.into());
        } else {
            handle.next_tick.store(u64::MAX, Ordering::Relaxed);
            this.sleep.set(None);
        }

        Poll::Pending
    }
}

impl Handle {
    pub fn new(first_tick: Option<u64>) -> Self {
        Handle {
            next_tick: AtomicU64::new(first_tick.unwrap_or(u64::MAX)),
            task: AtomicWaker::new(),
        }
    }

    /// Hastens the next tick of the associated `Scheduler` to the specified time.
    ///
    /// Does nothing if the current next tick is after the specified time.
    pub fn hasten(&self, next_tick: u64) {
        let prev = self.next_tick.fetch_min(next_tick, Ordering::Relaxed);
        if next_tick < prev {
            // Wake the associated scheduler task so that it can reset the `Sleep`.
            self.task.wake();
        }
    }

    fn decode_next_tick(&self) -> Option<Instant> {
        let next_tick = self.next_tick.load(Ordering::Relaxed);
        (next_tick != u64::MAX).then(|| util::instant_from_unix(Duration::from_secs(next_tick)))
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.task.wake();
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::sync::atomic::AtomicU32;

    use super::*;

    const PERIOD: Duration = Duration::from_secs(42);

    struct Handle {
        inner: super::Handle,
        count: AtomicU32,
    }

    impl Handle {
        fn new(first_tick: Option<u64>) -> Self {
            Handle {
                inner: super::Handle::new(first_tick),
                count: AtomicU32::new(0),
            }
        }

        fn hasten(&self, next_tick: u64) {
            self.inner.hasten(next_tick)
        }

        fn count(&self) -> u32 {
            self.count.load(Ordering::SeqCst)
        }

        fn incr_count(&self) -> u32 {
            self.count.fetch_add(1, Ordering::SeqCst) + 1
        }
    }

    impl AsRef<super::Handle> for Handle {
        fn as_ref(&self) -> &super::Handle {
            &self.inner
        }
    }

    #[tokio::test]
    async fn scheduler() {
        tokio::time::pause();

        let handle = Arc::new(Handle::new(Some(util::now_unix().as_secs() + 1)));
        let scheduler = Scheduler::new(&handle, move |handle: &Arc<Handle>| {
            let count = handle.incr_count();
            Ok::<_, Infallible>(Some((util::now_unix() + count * PERIOD).as_secs()))
        });
        let mut task = tokio_test::task::spawn(scheduler);

        task.enter(|cx, mut scheduler| {
            let _ = scheduler.as_mut().poll(cx);
            assert_eq!(handle.count(), 0);
        });
        assert!(!task.is_woken());

        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(task.is_woken());
        task.enter(|cx, mut scheduler| {
            let _ = scheduler.as_mut().poll(cx);
            assert_eq!(handle.count(), 1);
            let _ = scheduler.poll(cx);
            assert_eq!(handle.count(), 1);
        });

        tokio::time::advance(PERIOD).await;
        assert!(task.is_woken());
        task.enter(|cx, mut scheduler| {
            let _ = scheduler.as_mut().poll(cx);
            assert_eq!(handle.count(), 2);
            let _ = scheduler.poll(cx);
            assert_eq!(handle.count(), 2);
        });

        tokio::time::advance(PERIOD).await;
        // This fails on very rare occasions.
        // You may need to run the test hundreds of times to reproduce the failure.
        assert!(!task.is_woken());
        task.enter(|cx, scheduler| {
            let _ = scheduler.poll(cx);
            assert_eq!(handle.count(), 2);
        });

        tokio::time::advance(PERIOD).await;
        assert!(task.is_woken());
        task.enter(|cx, mut scheduler| {
            let _ = scheduler.as_mut().poll(cx);
            assert_eq!(handle.count(), 3);
            let _ = scheduler.poll(cx);
            assert_eq!(handle.count(), 3);
        });
    }

    #[tokio::test]
    async fn hasten() {
        tokio::time::pause();

        let handle = Arc::new(Handle::new(None));
        let scheduler = Scheduler::new(&handle, move |handle: &Arc<Handle>| {
            let count = handle.incr_count();
            Ok::<_, Infallible>(Some((util::now_unix() + count * PERIOD).as_secs()))
        });
        let mut task = tokio_test::task::spawn(scheduler);

        task.enter(|cx, mut scheduler| {
            let _ = scheduler.as_mut().poll(cx);
            assert_eq!(handle.count(), 0);
        });

        handle.hasten(util::now_unix().as_secs());
        // `handle.hasten` should wake the `task` to let it reset the inner `Sleep`.
        assert!(task.is_woken());
        task.enter(|cx, mut scheduler| {
            let _ = scheduler.as_mut().poll(cx);
            assert_eq!(handle.count(), 1);
            let _ = scheduler.poll(cx);
            assert_eq!(handle.count(), 1);
        });

        tokio::time::advance(PERIOD / 2).await;
        assert!(!task.is_woken());
        task.enter(|cx, mut scheduler| {
            let _ = scheduler.as_mut().poll(cx);
            assert_eq!(handle.count(), 1);
        });

        handle.hasten(util::now_unix().as_secs());
        assert!(task.is_woken());
        task.enter(|cx, mut scheduler| {
            let _ = scheduler.as_mut().poll(cx);
            assert_eq!(handle.count(), 2);
            let _ = scheduler.poll(cx);
            assert_eq!(handle.count(), 2);
        });

        handle.hasten((util::now_unix() + PERIOD).as_secs());
        assert!(task.is_woken());
        tokio::time::advance(PERIOD).await;
        task.enter(|cx, mut scheduler| {
            let _ = scheduler.as_mut().poll(cx);
            assert_eq!(handle.count(), 3);
            let _ = scheduler.poll(cx);
            assert_eq!(handle.count(), 3);
        });
    }
}
