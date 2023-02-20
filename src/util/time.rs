use std::time::{Duration, Instant, SystemTime};

use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(test)] {
        #[cfg(feature = "diesel2")]
        mod sleep;

        #[cfg(feature = "diesel2")]
        pub use self::sleep::*;

        pub fn instant_now() -> Instant {
            tokio::time::Instant::now().into()
        }

        pub fn system_time_now() -> SystemTime {
            let instant = Instant::now();
            let sys = SystemTime::now();
            let mocked = instant_now();
            if mocked > instant {
                sys + (mocked - instant)
            } else {
                sys - (instant - mocked)
            }
        }
    } else {
        pub fn instant_now() -> Instant {
            Instant::now()
        }

        pub fn system_time_now() -> SystemTime {
            SystemTime::now()
        }
    }
}

/// Converts a `Duration` representing a Unix time to an `Instant`.
pub fn instant_from_unix(unix: Duration) -> Instant {
    let now_i = instant_now();
    let now_unix = now_unix();
    // Do not add the `Duration`s directly to the `Instant` to mitigate the risk of overflowing.
    if now_unix < unix {
        now_i + (unix - now_unix)
    } else {
        now_i.checked_sub(now_unix - unix).unwrap()
    }
}

/// Returns the Unix time representation of "now" as a `Duration`.
pub fn now_unix() -> Duration {
    system_time_now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
}

#[cfg(test)]
mod tests {
    use std::cmp::PartialOrd;
    use std::ops::Sub;

    use super::*;

    const EPSILON: Duration = Duration::from_millis(10);

    macro_rules! assert_almost_eq {
        ($lhs:expr, $rhs:expr) => {{
            if diff_abs($lhs, $rhs) >= EPSILON {
                panic!(
                    r#"assertion failed: `(left \approx right)`
    left: `{:?}`,
   right: `{:?}`,
 epsilon: `{:?}`"#,
                    $lhs, $rhs, EPSILON
                );
            }
        }};
    }

    #[tokio::test]
    async fn advance() {
        let delta = Duration::from_secs(42);

        tokio::time::pause();
        let start = system_time_now();
        tokio::time::advance(delta).await;
        let end = system_time_now();

        assert_almost_eq!(end.duration_since(start).unwrap(), delta);
    }

    #[tokio::test]
    async fn backward() {
        let delta = Duration::from_millis(420);

        tokio::time::pause();
        let start = system_time_now();
        std::thread::sleep(delta);
        let end = system_time_now();

        let d = end.duration_since(start).unwrap_or_else(|e| e.duration());
        assert_almost_eq!(d, Duration::default());
    }

    fn diff_abs<T: PartialOrd + Sub>(x: T, y: T) -> T::Output {
        if x > y {
            x - y
        } else {
            y - x
        }
    }
}
