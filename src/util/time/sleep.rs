use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use std::{env, thread};

use futures::task::AtomicWaker;
use pin_project::pin_project;

/// A sleep `Future` that just works even when `tokio::time::pause()` has been called.
pub struct Sleep {
    inner: Arc<SleepInner>,
}

struct SleepInner {
    elapsed: AtomicBool,
    waker: AtomicWaker,
}

impl Sleep {
    pub fn new(duration: Duration) -> Self {
        let inner = Arc::new(SleepInner {
            elapsed: AtomicBool::new(false),
            waker: AtomicWaker::new(),
        });
        let inner2 = Arc::clone(&inner);
        thread::spawn(move || {
            thread::sleep(duration);
            inner2.elapsed.store(true, Ordering::SeqCst);
            inner2.waker.wake();
        });
        Sleep { inner }
    }
}

impl Future for Sleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.inner.elapsed.load(Ordering::SeqCst) {
            Poll::Ready(())
        } else {
            self.inner.waker.register(cx.waker());
            Poll::Pending
        }
    }
}

#[pin_project]
pub struct Timeout<F> {
    #[pin]
    future: F,
    #[pin]
    sleep: Sleep,
}

pub trait FutureTimeoutExt: Future + Sized {
    /// Make the `Future` panic after a timeout.
    fn timeout(self) -> Timeout<Self> {
        let duration = env::var("PIPITOR_TEST_TIMEOUT")
            .ok()
            .and_then(|v| v.parse().ok())
            .map(Duration::from_millis)
            .unwrap_or_else(|| Duration::from_millis(500));
        Timeout {
            future: self,
            sleep: Sleep::new(duration),
        }
    }
}

impl<F: Future> FutureTimeoutExt for F {}

impl<F: Future> Future for Timeout<F> {
    type Output = F::Output;

    #[track_caller]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        if this.sleep.poll(cx).is_ready() {
            panic!("`Future` timed out");
        }
        this.future.poll(cx)
    }
}
