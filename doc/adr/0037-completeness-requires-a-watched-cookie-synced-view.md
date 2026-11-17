# 0037. Completeness requires a watched, cookie-synced view

Date: 2026-11-18

## Status

Proposed (FORMAL-03, round 2; the owner decides at the next checkpoint).

## Context

ADR 0014 promises that `complete=true` is never wrong: when it is set, every module that can see
the target is indexed and fresh at the pinned generation (plan §2.1). Plan §4 gets freshness from
three sources:

- a per-request stat of the file, `HEAD` and the git index (§4.3);
- filesystem watcher events, as hints;
- `sync`, as an explicit barrier.

None of these, as plan §4 states them, proves a whole view fresh:

- **The stat covers one file**, plus `HEAD` and the index.
- **Watcher events can be late.** A change whose event has not arrived yet is invisible to the
  daemon.
- **Watcher events can be lost.** Some losses are reported:
  - inotify's `IN_Q_OVERFLOW`;
  - FSEvents' `MustScanSubDirs`, `UserDropped` and `KernelDropped`.

  Others are signalled only as a lost watch:
  - an `add_watch` that fails with ENOSPC or EMFILE;
  - `IN_IGNORED`, `IN_DELETE_SELF` and `IN_UNMOUNT`;
  - FSEvents' `RootChanged` and `EventIdsWrapped`.

  And some are never signalled at all:
  - files created in a new directory before its watch is added;
  - writes through `mmap`;
  - network and FUSE filesystems (NFS, SMB, sshfs, 9p, virtiofs);
  - the lower layer of an overlayfs.
- **A linked worktree's `HEAD` and index live outside its root**, under the common directory's
  `worktrees/<name>/`. No watcher of the worktree sees them change.
- **A same-size write within the mtime granularity of the last summary** leaves a stat unchanged.
  This is git's racy-index problem.

The FORMAL-03 review found that `formal/views.qnt` broke `CompleteIsHonest` in 13 steps. A
checkout of a worktree was reconciled through its file events only. A request waiting from before
the checkout then pinned `complete=true` with the old `HEAD`.

## Decision

`complete=true` requires, at the request's pin, all of these:

1. **The view is watched and certain.**
   - "Watched" means its watch is established.
   - "Certain" means there has been no loss since a full scan that started after the watch was
     set up: watch, then scan.
   - Every loss signal makes a view uncertain. An overflow makes uncertain every view that shares
     the inotify instance or FSEvents stream, because the signal does not say whose event was lost.
   - A lost watch makes its view uncertain until the watch is re-established and a full scan
     started after that has published.
   - A filesystem whose changes raise no events (network, FUSE, overlayfs; found with `statfs`) is
     never watched, so it is never complete.
2. **The request's cookie is back.**
   - The request writes a cookie file into the view at its stat, and its event comes back through
     the same watcher stream.
   - Events within one inotify instance, or one FSEvents stream after `FSEventStreamFlushSync`,
     arrive in order. So every change made before the cookie has been delivered by the time the
     cookie is.
   - The cookie and the view's files share one instance or stream, with no debouncer, coalescer
     or other reordering layer between the kernel and the daemon.
3. **No published summary was read after that cookie was written.**
   - The daemon stamps each summary with when it was read, on a local clock ordered with its
     cookie writes.
   - If the pinned generation holds a summary read after the request's cookie, the generation may
     place a newer version of one file beside an older version of another: a state the disk never
     held. A `refs` answer could then miss a call that moved from one file to another, though the
     call was on disk at every instant.
   - In that case the request writes a second cookie at the pin. If that round trip leaves nothing
     delivered and unpublished, and no summary read after it, the generation is the disk's state
     at the moment the second cookie was written. Otherwise the answer is incomplete.
   - A request writes at most two cookies.
4. **Nothing delivered is unpublished.** No delivered event awaits a build, no rescan is due, and
   no build is in progress.
5. **`HEAD` and the index are re-statted at the pin** and equal the pinned generation's. A change
   there starts a rescan, and the answer is incomplete.
6. **Every stat applies the racy-mtime rule.** A summary taken within the mtime granularity of its
   file's last write never proves that file fresh.

What this promises is a snapshot: the answer's index equals one state the view's files actually
had between the request's sync point (its stat and first cookie) and its pin, and `HEAD` and the
index are the disk's at the pin (`CompleteIsHonest` in `views.qnt`). So `exact ∪ possible` holds
every reference of that state, as ADR 0014 requires. A write that lands after the state the
answer reflects may be missing; it is not part of the promise.

**Accepted limitation: writes through a writable `mmap`.** They raise no watcher event and change
the mtime only at writeback, so a file another process maps writable cannot be proven fresh. The
daemon cannot detect such a mapping cheaply; `complete=true` does not cover it.

## Consequences

- **Pinned by the model.** `formal/views.qnt` models all of this. Its controls each violate an
  invariant:
  - no re-stat at the pin;
  - a generation mixing a summary read after the cookie with an older one (no second cookie);
  - a HEAD-only move in a linked worktree;
  - no cookie;
  - trusting a lost watch;
  - an overflow that marks only one view;
  - no racy rule.

  `doc/formal-models.md` lists IDX-09's obligations, and IDX-09's driver conforms to the model.
- **IDX-09 implements:**
  - the cookie, and the second cookie when a summary was read after the first;
  - a read stamp on every summary, on a clock ordered with cookie writes;
  - the per-view watch state;
  - watch-then-scan, including a scan of each new directory after its watch is added;
  - the treatment of watch errors as losses;
  - the `statfs` check;
  - the racy rule;
  - the re-stat at the pin.
- **Cost.** A complete answer costs at most two cookie round trips, each the watcher's latency
  (milliseconds): one always, the second only when the generation was re-summarised during the
  request. IDX-09 measures them against plan §1.7's 100 ms freshness target. An answer that
  cannot wait for them is honestly incomplete.
- **Plan §4.3 is extended**: the stat at the pin covers `HEAD` and the index again.

## Alternatives considered

- **A full scan per request** (`git status` over the view, or a stat of every file). It is sound
  without watchers, but it costs tens to hundreds of milliseconds per request on a large
  repository, against plan §1.7's speed budget. It remains the fallback on a filesystem that
  cannot be watched, as a `sync`.
- **Trusting the watcher without a cookie.** An event that is late but not lost would stand behind
  a wrong `complete=true`.
- **Treating every loss as reported.** Lost watches, new directories and network filesystems
  are not reported, as the context lists.
