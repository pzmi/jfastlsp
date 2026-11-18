//! Running the ports' futures in tests without a runtime: [`block_on`] and [`InlinePool`].
//!
//! The ports are native `async fn` traits (fnd §4.4) and the test crates hold no Tokio. A port's
//! future that waits on an OS event (a watch's next hint) is not driven here: a test reads such a
//! source with its synchronous `try_next` under [`crate::poll::wait_until`], with a deadline.

use core::future::Future;
use core::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};

use jfl_application::{BlockingPool, Interrupted};

struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// Polls `future` on this thread until it resolves, parking between polls until its waker is
/// called. A future that is never woken parks forever: use it for futures that complete or wake.
///
/// Pinned by: `exec::tests::should_resolve_a_future_when_it_is_woken_from_another_poll`.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
        thread::park();
    }
}

/// A [`BlockingPool`] that runs each job on the calling thread, at once: for tests and the
/// benchmark, which have no runtime to keep free.
#[derive(Debug, Clone, Copy, Default)]
pub struct InlinePool;

impl BlockingPool for InlinePool {
    fn run<T, F>(&self, job: F) -> impl Future<Output = Result<T, Interrupted>> + Send
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        core::future::ready(Ok(job()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_resolve_a_future_when_it_is_woken_from_another_poll() {
        // given
        let mut polls = 0_u32;
        let future = core::future::poll_fn(|cx| {
            polls = polls.saturating_add(1);
            if polls < 3 {
                cx.waker().wake_by_ref();
                Poll::Pending
            } else {
                Poll::Ready(polls)
            }
        });

        // when
        let output = block_on(future);

        // then
        assert_eq!(output, 3);
    }

    #[test]
    fn should_run_the_job_when_the_pool_is_inline() {
        // given
        let pool = InlinePool;

        // when
        let output = block_on(pool.run(|| 6_u32.saturating_mul(7)));

        // then
        assert_eq!(output, Ok(42));
    }
}
