//! The bounded, coalescing hint queue every `SourceTree` watch delivers through (plan §4.3).
//!
//! A watcher's thread pushes through a [`HintSink`]; the view's reconciler takes from the
//! [`HintQueue`]. A path already waiting is not queued twice, so a burst of writes to one file is
//! one hint, and the reconciler that drains the queue gets a batch without a debounce timer
//! delaying the first hint. The queue holds at most its capacity of paths: past it, the paths are
//! dropped, counted, and replaced by one [`Hint::Rescan`], so an overflow of the queue is always
//! reported and memory stays bounded (fnd §4.4 rule 5). A waiting rescan keeps every reason it was
//! asked for, so a lost watch never hides behind a full queue; and the paths that fold into it are
//! counted as the queue would have held them, so more distinct paths than its capacity add
//! [`RescanReason::QueueFull`] whichever reason the rescan was waiting for (FND-14: FSEvents'
//! event on the root made the rescan first, and a burst behind it hid the overflow). When the watch's backend stops, the
//! sink closes the queue: one last rescan ([`RescanReason::WatchEnded`]), then `next()` resolves to
//! `None` instead of waiting for ever.
//!
//! Rejected alternative: `notify-debouncer-mini` 0.7, which the card named. Its debounce map is
//! keyed by path, so the inotify overflow event (no path, `Flag::Rescan`) disappears in it, and it
//! feeds its thread through an unbounded channel (ADR 0019, amended 2026-11-18).

use core::future::Future;
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Poll, Waker};

use jfl_domain::RelPath;

use crate::source_tree::{Hint, Hints, Rescan, RescanReason, RescanReasons};

/// The hint queue's default capacity. A change that touches more files than this (a checkout,
/// a rebase) is cheaper to reconcile as one rescan, which asks Git what changed, than as one
/// hint per file.
pub const DEFAULT_HINT_CAPACITY: usize = 8192;

#[derive(Debug)]
struct State {
    order: VecDeque<RelPath>,
    waiting: HashSet<RelPath>,
    rescan: Option<Rescan>,
    /// The distinct paths folded into the waiting rescan, while it lacks `QueueFull`: at most
    /// `capacity`, since one more is the overflow, which adds `QueueFull` and clears them.
    folded: HashSet<RelPath>,
    capacity: usize,
    waker: Option<Waker>,
    closed: bool,
}

impl State {
    fn wake(&mut self) {
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }

    fn rescan(&mut self, reason: RescanReason, dropped: u64) {
        if self.closed {
            return;
        }
        let dropped = dropped.saturating_add(u64::try_from(self.order.len()).unwrap_or(u64::MAX));
        self.order.clear();
        if reason == RescanReason::QueueFull {
            self.waiting.clear();
            self.folded.clear();
        } else {
            let waiting = std::mem::take(&mut self.waiting);
            self.folded.extend(waiting);
        }
        self.rescan = Some(match self.rescan {
            Some(mut pending) => {
                pending.reasons.insert(reason);
                pending.dropped = pending.dropped.saturating_add(dropped);
                pending
            }
            None => Rescan {
                reasons: RescanReasons::of(reason),
                dropped,
            },
        });
        self.wake();
    }

    /// A path changed while a rescan waits: counted in it, and once more distinct paths than the
    /// queue holds have folded into it, the queue would have overflowed (`QueueFull`).
    fn fold(&mut self, path: RelPath) {
        let Some(rescan) = self.rescan.as_mut() else {
            return;
        };
        rescan.dropped = rescan.dropped.saturating_add(1);
        if rescan.reasons.contains(RescanReason::QueueFull) {
            return;
        }
        self.folded.insert(path);
        if self.folded.len() > self.capacity {
            rescan.reasons.insert(RescanReason::QueueFull);
            self.folded.clear();
        }
    }

    fn take(&mut self) -> Option<Hint> {
        if let Some(rescan) = self.rescan.take() {
            self.folded.clear();
            return Some(Hint::Rescan(rescan));
        }
        let path = self.order.pop_front()?;
        self.waiting.remove(&path);
        Some(Hint::Changed(path))
    }
}

#[derive(Debug)]
struct Shared(Mutex<State>);

impl Shared {
    /// The state; a panic elsewhere while holding the lock leaves it consistent (every update is
    /// a few field writes), so a poisoned lock is taken as it is.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The pushing side; cheap to clone, one per watcher thread.
#[derive(Debug, Clone)]
pub struct HintSink(Arc<Shared>);

/// The taking side.
#[derive(Debug)]
pub struct HintQueue(Arc<Shared>);

/// A queue holding at most `capacity` paths (at least one).
///
/// Pinned by: `hints::tests::should_report_one_rescan_when_the_queue_overflows`,
/// `hints::tests::should_queue_a_path_once_when_it_changes_twice` and
/// `hints::tests::should_add_queue_full_when_more_paths_than_the_queue_holds_fold_into_a_pending_rescan`.
#[must_use]
pub fn hint_queue(capacity: usize) -> (HintSink, HintQueue) {
    let shared = Arc::new(Shared(Mutex::new(State {
        order: VecDeque::new(),
        waiting: HashSet::new(),
        rescan: None,
        folded: HashSet::new(),
        capacity: capacity.max(1),
        waker: None,
        closed: false,
    })));
    (HintSink(Arc::clone(&shared)), HintQueue(shared))
}

impl HintSink {
    /// Something at `path` changed. Coalesced with a waiting hint for the same path; subsumed by
    /// a waiting rescan (and counted in it).
    pub fn changed(&self, path: RelPath) {
        let mut state = self.0.lock();
        if state.closed {
            return;
        }
        if state.rescan.is_some() {
            state.fold(path);
            return;
        }
        if state.waiting.contains(&path) {
            return;
        }
        if state.order.len() >= state.capacity {
            state.rescan(RescanReason::QueueFull, 1);
            return;
        }
        state.waiting.insert(path.clone());
        state.order.push_back(path);
        state.wake();
    }

    /// Everything may have changed; the paths waiting are dropped and counted.
    pub fn rescan(&self, reason: RescanReason) {
        self.0.lock().rescan(reason, 0);
    }

    /// The watch ended: a last [`RescanReason::WatchEnded`] rescan, then the queue reports its
    /// end. Later pushes through any sink are ignored.
    ///
    /// Pinned by: `hints::tests::should_end_the_stream_when_the_sink_closes`.
    pub fn close(&self) {
        let mut state = self.0.lock();
        if state.closed {
            return;
        }
        state.rescan(RescanReason::WatchEnded, 0);
        state.closed = true;
    }
}

impl HintQueue {
    /// Takes the next hint, or registers `cx`'s waker; `Ready(None)` once the queue is closed and
    /// empty.
    pub fn poll_take(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Option<Hint>> {
        let mut state = self.0.lock();
        match state.take() {
            Some(hint) => Poll::Ready(Some(hint)),
            None if state.closed => Poll::Ready(None),
            None => {
                state.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl Hints for HintQueue {
    fn try_next(&mut self) -> Option<Hint> {
        self.0.lock().take()
    }

    fn next(&mut self) -> impl Future<Output = Option<Hint>> + Send {
        std::future::poll_fn(|cx| self.poll_take(cx))
    }
}

#[cfg(test)]
mod tests {
    use core::pin::pin;
    use std::task::Context;

    use super::*;
    use crate::source_tree::WatchErrorKind;

    fn rel(path: &str) -> RelPath {
        RelPath::new(path).unwrap()
    }

    #[test]
    fn should_queue_a_path_once_when_it_changes_twice() {
        // given
        let (sink, mut queue) = hint_queue(8);

        // when
        sink.changed(rel("a/A.java"));
        sink.changed(rel("b/B.java"));
        sink.changed(rel("a/A.java"));

        // then
        assert_eq!(queue.try_next(), Some(Hint::Changed(rel("a/A.java"))));
        assert_eq!(queue.try_next(), Some(Hint::Changed(rel("b/B.java"))));
        assert_eq!(queue.try_next(), None);
    }

    #[test]
    fn should_queue_a_path_again_when_it_changes_after_it_was_taken() {
        // given
        let (sink, mut queue) = hint_queue(8);
        sink.changed(rel("A.java"));
        let _ = queue.try_next();

        // when
        sink.changed(rel("A.java"));

        // then
        assert_eq!(queue.try_next(), Some(Hint::Changed(rel("A.java"))));
    }

    #[test]
    fn should_report_one_rescan_when_the_queue_overflows() {
        // given
        let (sink, mut queue) = hint_queue(2);

        // when
        for name in ["A.java", "B.java", "C.java", "D.java"] {
            sink.changed(rel(name));
        }

        // then
        assert_eq!(
            queue.try_next(),
            Some(Hint::Rescan(Rescan {
                reasons: RescanReasons::of(RescanReason::QueueFull),
                dropped: 4,
            }))
        );
        assert_eq!(queue.try_next(), None);
    }

    #[test]
    fn should_keep_every_reason_when_rescans_pile_up() {
        // given: a large checkout first fills the queue, then a new directory's watch fails
        let (sink, mut queue) = hint_queue(1);
        sink.changed(rel("A.java"));
        sink.changed(rel("B.java"));

        // when
        sink.rescan(RescanReason::WatcherError(WatchErrorKind::LimitReached));
        sink.changed(rel("C.java"));

        // then
        let Some(Hint::Rescan(rescan)) = queue.try_next() else {
            panic!("no rescan");
        };
        assert!(rescan.reasons.contains(RescanReason::QueueFull));
        assert!(
            rescan
                .reasons
                .contains(RescanReason::WatcherError(WatchErrorKind::LimitReached))
        );
        assert!(rescan.reasons.degrade_the_watch());
        assert_eq!(rescan.dropped, 3);
        assert_eq!(queue.try_next(), None);
    }

    #[test]
    fn should_add_queue_full_when_more_paths_than_the_queue_holds_fold_into_a_pending_rescan() {
        // given: a rescan already waits for another reason (FSEvents' event on the root itself)
        let (sink, mut queue) = hint_queue(2);
        sink.rescan(RescanReason::Unnamed);

        // when: more distinct paths change than the queue holds
        for name in ["A.java", "B.java", "C.java"] {
            sink.changed(rel(name));
        }

        // then
        let mut reasons = RescanReasons::of(RescanReason::Unnamed);
        reasons.insert(RescanReason::QueueFull);
        assert_eq!(
            queue.try_next(),
            Some(Hint::Rescan(Rescan {
                reasons,
                dropped: 3,
            }))
        );
        assert_eq!(queue.try_next(), None);
    }

    #[test]
    fn should_not_add_queue_full_when_paths_that_fold_into_a_pending_rescan_fit_the_queue() {
        // given
        let (sink, mut queue) = hint_queue(2);
        sink.changed(rel("A.java"));
        sink.rescan(RescanReason::Unnamed);

        // when: one path more, and a path already counted changing again
        sink.changed(rel("B.java"));
        sink.changed(rel("A.java"));

        // then
        assert_eq!(
            queue.try_next(),
            Some(Hint::Rescan(Rescan {
                reasons: RescanReasons::of(RescanReason::Unnamed),
                dropped: 3,
            }))
        );
    }

    #[test]
    fn should_keep_every_reason_when_any_reason_folds_into_a_pending_rescan() {
        let reasons = [
            RescanReason::QueueFull,
            RescanReason::WatcherOverflow,
            RescanReason::Unnamed,
            RescanReason::WatcherError(WatchErrorKind::Io),
        ];
        for first in reasons {
            for second in [
                RescanReason::WatcherOverflow,
                RescanReason::Unnamed,
                RescanReason::WatcherError(WatchErrorKind::Io),
                RescanReason::WatchEnded,
            ] {
                // given: a rescan waits for `first` (QueueFull by an overflow of a queue of one)
                let (sink, mut queue) = hint_queue(1);
                if first == RescanReason::QueueFull {
                    sink.changed(rel("A.java"));
                    sink.changed(rel("B.java"));
                } else {
                    sink.rescan(first);
                }

                // when
                if second == RescanReason::WatchEnded {
                    sink.close();
                } else {
                    sink.rescan(second);
                }

                // then
                let Some(Hint::Rescan(rescan)) = queue.try_next() else {
                    panic!("no rescan after {first:?} then {second:?}");
                };
                assert!(
                    rescan.reasons.contains(first) && rescan.reasons.contains(second),
                    "{first:?} then {second:?}: {rescan:?}"
                );
            }
        }
    }

    #[test]
    fn should_end_the_stream_when_the_sink_closes() {
        // given
        let (sink, mut queue) = hint_queue(8);
        sink.changed(rel("A.java"));
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);

        // when
        sink.close();
        sink.changed(rel("B.java"));
        let last = queue.poll_take(&mut cx);
        let after = queue.poll_take(&mut cx);

        // then
        assert_eq!(
            last,
            Poll::Ready(Some(Hint::Rescan(Rescan {
                reasons: RescanReasons::of(RescanReason::WatchEnded),
                dropped: 1,
            })))
        );
        assert_eq!(after, Poll::Ready(None));
    }

    #[test]
    fn should_wake_a_waiting_reader_when_a_hint_arrives() {
        // given
        let (sink, mut queue) = hint_queue(8);
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        let mut next = pin!(queue.next());
        let first = next.as_mut().poll(&mut cx);

        // when
        sink.changed(rel("A.java"));
        let second = next.as_mut().poll(&mut cx);

        // then
        assert_eq!(first, Poll::Pending);
        assert_eq!(second, Poll::Ready(Some(Hint::Changed(rel("A.java")))));
    }
}
