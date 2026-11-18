//! Component tests of the view registry (IDX-09, plan §4, ADR 0037): the registry over what a
//! stat of a scripted file sees, with no I/O.

use std::collections::BTreeSet;
use std::time::Duration;

use jfl_application::source_tree::{FileStat, MTime};
use jfl_application::views::{
    Event, FileSeen, GitSeen, RequestId, Stamp, Stored, ViewError, ViewRegistry,
};
use jfl_domain::{FileUri, IncompleteReason, RelPath};

type TestResult = Result<(), String>;
type Registry = ViewRegistry<FileSeen, FileSeen, u32>;

const GRANULARITY: Duration = Duration::from_millis(10);

#[derive(Default)]
struct Store(BTreeSet<u32>);

impl Stored<u32> for Store {
    fn holds(&self, key: &u32) -> bool {
        self.0.contains(key)
    }
}

fn root() -> Result<FileUri, String> {
    FileUri::from_path("/w/repo").map_err(|e| e.to_string())
}

fn rel(path: &str) -> Result<RelPath, String> {
    RelPath::new(path).map_err(|e| e.to_string())
}

fn file(path: &str) -> Result<FileUri, String> {
    root()?.join(&rel(path)?).map_err(|e| e.to_string())
}

/// A file of `size` bytes at `inode`, written at second `secs`, seen a minute later (settled).
fn seen(size: u64, inode: u64, secs: i64) -> FileSeen {
    let stat = FileStat {
        size,
        mtime: MTime { secs, nanos: 0 },
        inode,
    };
    FileSeen::new(
        Some(stat),
        MTime {
            secs: secs.saturating_add(60),
            nanos: 0,
        },
        GRANULARITY,
    )
}

fn git(head: u64) -> GitSeen<FileSeen> {
    GitSeen {
        head: seen(41, head, 10),
        index: seen(100, 1, 10),
    }
}

/// A watched view of `A.java` (key 1) and `B.java` (key 2) at generation 1, both settled.
fn opened() -> Result<(Registry, Store), String> {
    let mut registry = Registry::new();
    registry
        .open(
            root()?,
            true,
            git(1),
            [
                (rel("A.java")?, seen(10, 1, 100), 1),
                (rel("B.java")?, seen(20, 2, 100), 2),
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok((registry, Store(BTreeSet::from([1, 2]))))
}

fn err(e: ViewError) -> String {
    e.to_string()
}

#[test]
fn should_answer_wholly_from_n_when_n_plus_1_publishes_while_pinned() -> TestResult {
    // given: a request pinned to generation 1, complete
    let (mut registry, mut store) = opened()?;
    let r1 = RequestId(1);
    let started = registry
        .request_start(r1, &file("A.java")?, &seen(10, 1, 100), &git(1), Stamp(0))
        .map_err(err)?;
    assert!(started.write_cookie);
    registry
        .deliver(Event::Cookie {
            id: r1,
            at: Stamp(0),
        })
        .map_err(err)?;
    let complete = registry
        .request_pin(r1, &git(1))
        .map_err(err)?
        .completeness
        .is_complete();
    assert!(complete);

    // when: both files change and generation 2 publishes
    let (a2, b2) = (seen(11, 1, 200), seen(21, 2, 200));
    for path in ["A.java", "B.java"] {
        registry
            .deliver(Event::File {
                view: root()?,
                path: rel(path)?,
            })
            .map_err(err)?;
    }
    let pending = registry
        .reconcile_start(
            &root()?,
            None,
            |p| if p.as_str() == "A.java" { a2 } else { b2 },
            || git(1),
        )
        .map_err(err)?;
    assert_eq!(pending.len(), 2);
    for (path, now, key) in [("A.java", a2, 3), ("B.java", b2, 4)] {
        store.0.insert(key);
        registry
            .summarised(&root()?, &rel(path)?, now, key, Stamp(2))
            .map_err(err)?;
    }
    let published = registry.publish(&root()?, &store).map_err(err)?;

    // then: the pinned request reads generation 1 only, both files
    assert_eq!(published.0, 2);
    let a = registry.read(r1, &rel("A.java")?).map_err(err)?.summary;
    let b = registry.read(r1, &rel("B.java")?).map_err(err)?.summary;
    assert_eq!((a, b), (Some(1), Some(2)));
    let pinned = registry
        .request(r1)
        .and_then(|r| r.pin.as_ref())
        .map(|p| p.generation.generation.0);
    assert_eq!(pinned, Some(1));
    Ok(())
}

#[test]
fn should_see_a_sed_i_write_when_the_stat_sees_it_without_sync() -> TestResult {
    // given: `sed -i` replaced A.java (a new inode, the same size, the same second); its event
    // has not arrived
    let (mut registry, mut store) = opened()?;
    let replaced = seen(10, 9, 100);
    let r1 = RequestId(1);

    // when: a request's stat sees it, and the view reconciles and publishes, with no `sync`
    registry
        .request_start(r1, &file("A.java")?, &replaced, &git(1), Stamp(1))
        .map_err(err)?;
    let early = registry.request_pin(r1, &git(1)).map(|_| ());
    let pending = registry
        .reconcile_start(&root()?, None, |_| replaced, || git(1))
        .map_err(err)?;
    store.0.insert(5);
    registry
        .summarised(&root()?, &rel("A.java")?, replaced, 5, Stamp(2))
        .map_err(err)?;
    registry.publish(&root()?, &store).map_err(err)?;
    registry
        .deliver(Event::Cookie {
            id: r1,
            at: Stamp(1),
        })
        .map_err(err)?;
    // the summary was read after the cookie: a second cookie proves no change since
    registry.request_recookie(r1, Stamp(3)).map_err(err)?;
    registry
        .deliver(Event::Cookie {
            id: r1,
            at: Stamp(3),
        })
        .map_err(err)?;
    let pin = registry.request_pin(r1, &git(1)).map_err(err)?;
    let complete = pin.completeness.is_complete();
    let generation = pin.generation.generation.0;
    let answered = registry.read(r1, &rel("A.java")?).map_err(err)?.summary;

    // then
    assert_eq!(early, Err(ViewError::NotPinnable));
    assert_eq!(pending, BTreeSet::from([rel("A.java")?]));
    assert_eq!(generation, 2);
    assert_eq!(answered, Some(5));
    assert!(complete);
    Ok(())
}

#[test]
fn should_answer_incomplete_when_a_summary_was_read_after_the_only_cookie() -> TestResult {
    // given: a request whose generation holds a summary read after its cookie
    let (mut registry, mut store) = opened()?;
    let r1 = RequestId(1);
    let changed = seen(12, 1, 300);
    registry
        .request_start(r1, &file("A.java")?, &changed, &git(1), Stamp(1))
        .map_err(err)?;
    registry
        .reconcile_start(&root()?, None, |_| changed, || git(1))
        .map_err(err)?;
    store.0.insert(7);
    registry
        .summarised(&root()?, &rel("A.java")?, changed, 7, Stamp(2))
        .map_err(err)?;
    registry.publish(&root()?, &store).map_err(err)?;
    registry
        .deliver(Event::Cookie {
            id: r1,
            at: Stamp(1),
        })
        .map_err(err)?;

    // when: it pins without a second cookie
    let pin = registry.request_pin(r1, &git(1)).map_err(err)?;

    // then
    assert!(!pin.completeness.is_complete());
    assert!(
        pin.completeness
            .reasons()
            .contains(&IncompleteReason::PendingChanges { events: 1 })
    );
    Ok(())
}

#[test]
fn should_wait_for_a_new_generation_when_the_indexed_entry_is_racy() -> TestResult {
    // given: A.java summarised within its write's tick
    let mut registry = Registry::new();
    let written = FileStat {
        size: 10,
        mtime: MTime {
            secs: 100,
            nanos: 0,
        },
        inode: 1,
    };
    let racy = FileSeen::new(
        Some(written),
        MTime {
            secs: 100,
            nanos: 1,
        },
        GRANULARITY,
    );
    registry
        .open(root()?, true, git(1), [(rel("A.java")?, racy, 1)])
        .map_err(err)?;

    // when: a request's stat sees an equal stat
    registry
        .request_start(
            RequestId(1),
            &file("A.java")?,
            &seen(10, 1, 100),
            &git(1),
            Stamp(0),
        )
        .map_err(err)?;

    // then: an equal stat proves nothing about a racy entry
    let need = registry.request(RequestId(1)).map(|r| r.need.0);
    assert_eq!(need, Some(2));
    let dirty = registry.view(&root()?).map(|v| v.dirty.clone());
    assert_eq!(dirty, Some(BTreeSet::from([rel("A.java")?])));
    Ok(())
}

#[test]
fn should_never_claim_complete_when_the_view_is_not_watched() -> TestResult {
    // given: a view on a filesystem that cannot be watched
    let mut registry = Registry::new();
    registry
        .open(
            root()?,
            false,
            git(1),
            [(rel("A.java")?, seen(10, 1, 100), 1)],
        )
        .map_err(err)?;
    let r1 = RequestId(1);

    // when
    let started = registry
        .request_start(r1, &file("A.java")?, &seen(10, 1, 100), &git(1), Stamp(0))
        .map_err(err)?;
    let reasons = registry
        .request_pin(r1, &git(1))
        .map_err(err)?
        .completeness
        .reasons()
        .to_vec();

    // then
    assert!(!started.write_cookie);
    assert!(reasons.contains(&IncompleteReason::NotWatched));
    assert!(reasons.contains(&IncompleteReason::WatcherLostEvents));
    Ok(())
}

#[test]
fn should_answer_incomplete_and_rescan_when_head_moved_before_the_pin() -> TestResult {
    // given: a fresh request whose cookie came back
    let (mut registry, _) = opened()?;
    let r1 = RequestId(1);
    registry
        .request_start(r1, &file("B.java")?, &seen(20, 2, 100), &git(1), Stamp(0))
        .map_err(err)?;
    registry
        .deliver(Event::Cookie {
            id: r1,
            at: Stamp(0),
        })
        .map_err(err)?;

    // when: HEAD moved outside the watched root (a linked worktree's) before the pin
    let complete = registry
        .request_pin(r1, &git(2))
        .map_err(err)?
        .completeness
        .is_complete();

    // then
    assert!(!complete);
    assert_eq!(registry.view(&root()?).map(|v| v.rescan), Some(true));
    Ok(())
}

#[test]
fn should_return_from_sync_only_when_a_rescan_started_after_it_has_published() -> TestResult {
    // given: a build that started before the sync
    let (mut registry, store) = opened()?;
    registry
        .deliver(Event::File {
            view: root()?,
            path: rel("A.java")?,
        })
        .map_err(err)?;
    let unchanged = seen(10, 1, 100);
    registry
        .reconcile_start(&root()?, None, |_| unchanged, || git(1))
        .map_err(err)?;
    registry
        .summarised(&root()?, &rel("A.java")?, unchanged, 1, Stamp(0))
        .map_err(err)?;

    // when
    registry.sync_start(&root()?).map_err(err)?;
    registry.publish(&root()?, &store).map_err(err)?;
    let early = registry.sync_done(&root()?);
    registry
        .reconcile_start(
            &root()?,
            None,
            |p| {
                if p.as_str() == "A.java" {
                    unchanged
                } else {
                    seen(20, 2, 100)
                }
            },
            || git(1),
        )
        .map_err(err)?;
    registry.publish(&root()?, &store).map_err(err)?;
    let done = registry.sync_done(&root()?);

    // then
    assert_eq!(early, Err(ViewError::SyncState));
    assert_eq!(done, Ok(()));
    Ok(())
}

#[test]
fn should_wait_for_the_second_cookie_when_a_stale_first_cookie_event_arrives() -> TestResult {
    // given: the first cookie's first event is back, and a summary was read after it
    let (mut registry, mut store) = opened()?;
    let r1 = RequestId(1);
    let changed = seen(12, 1, 300);
    registry
        .request_start(r1, &file("A.java")?, &changed, &git(1), Stamp(1))
        .map_err(err)?;
    registry
        .deliver(Event::Cookie {
            id: r1,
            at: Stamp(1),
        })
        .map_err(err)?;
    registry
        .reconcile_start(&root()?, None, |_| changed, || git(1))
        .map_err(err)?;
    store.0.insert(7);
    registry
        .summarised(&root()?, &rel("A.java")?, changed, 7, Stamp(2))
        .map_err(err)?;
    registry.publish(&root()?, &store).map_err(err)?;
    registry.request_recookie(r1, Stamp(3)).map_err(err)?;

    // when: the first cookie's late event (its IN_CLOSE_WRITE) arrives after the second was written
    registry
        .deliver(Event::Cookie {
            id: r1,
            at: Stamp(1),
        })
        .map_err(err)?;
    let early = registry.request_pin(r1, &git(1)).map(|_| ());
    registry
        .deliver(Event::Cookie {
            id: r1,
            at: Stamp(3),
        })
        .map_err(err)?;
    let complete = registry
        .request_pin(r1, &git(1))
        .map_err(err)?
        .completeness
        .is_complete();

    // then: only the second cookie's own event lets it pin, and then it is complete
    assert_eq!(early, Err(ViewError::CookieQueued));
    assert!(complete);
    Ok(())
}

/// The view after its watch was lost, `C.java` created meanwhile (no event), and the watch
/// restored: a rescan is due.
fn created_while_unwatched() -> Result<(Registry, Store, FileSeen), String> {
    let (mut registry, store) = opened()?;
    registry.watch_lost(&root()?).map_err(err)?;
    registry.watch_restored(&root()?).map_err(err)?;
    Ok((registry, store, seen(30, 3, 400)))
}

fn stat_of(path: &RelPath, created: FileSeen) -> FileSeen {
    match path.as_str() {
        "A.java" => seen(10, 1, 100),
        "B.java" => seen(20, 2, 100),
        _ => created,
    }
}

/// A request on `A.java` started, its cookie back and pinned: whether it is complete, and
/// whether its generation holds `C.java`.
fn answer(registry: &mut Registry) -> Result<(bool, bool), String> {
    let r1 = RequestId(1);
    registry
        .request_start(r1, &file("A.java")?, &seen(10, 1, 100), &git(1), Stamp(5))
        .map_err(err)?;
    registry
        .deliver(Event::Cookie {
            id: r1,
            at: Stamp(5),
        })
        .map_err(err)?;
    let pin = registry.request_pin(r1, &git(1)).map_err(err)?;
    let holds = pin.generation.index.contains_key(&rel("C.java")?);
    Ok((pin.completeness.is_complete(), holds))
}

#[test]
fn should_read_a_file_created_while_the_watch_was_lost_when_the_rescan_lists_it() -> TestResult {
    // given
    let (mut registry, mut store, created) = created_while_unwatched()?;
    let listed = BTreeSet::from([rel("A.java")?, rel("B.java")?, rel("C.java")?]);

    // when
    let pending = registry
        .reconcile_start(&root()?, Some(&listed), |p| stat_of(p, created), || git(1))
        .map_err(err)?;
    store.0.insert(9);
    registry
        .summarised(&root()?, &rel("C.java")?, created, 9, Stamp(4))
        .map_err(err)?;
    registry.publish(&root()?, &store).map_err(err)?;
    let (complete, holds) = answer(&mut registry)?;

    // then: the new file is in the answer's generation, which may claim completeness
    assert_eq!(pending, BTreeSet::from([rel("C.java")?]));
    assert!(holds);
    assert!(complete);
    Ok(())
}

#[test]
fn should_stay_uncertain_when_a_rescan_has_no_listing() -> TestResult {
    // given
    let (mut registry, store, created) = created_while_unwatched()?;

    // when: the rescan re-stats only what the index holds
    let pending = registry
        .reconcile_start(&root()?, None, |p| stat_of(p, created), || git(1))
        .map_err(err)?;
    registry.publish(&root()?, &store).map_err(err)?;
    let (complete, holds) = answer(&mut registry)?;

    // then: the new file is not in the answer, and the answer says it may be incomplete
    assert!(pending.is_empty());
    assert!(!holds);
    assert!(!complete);
    assert_eq!(registry.view(&root()?).map(|v| v.certain), Some(false));
    Ok(())
}

#[test]
fn should_refuse_publish_when_a_named_summary_is_not_stored() -> TestResult {
    // given: a build whose summary of A.java was recorded but is not in the store
    let (mut registry, store) = opened()?;
    let changed = seen(13, 1, 500);
    registry
        .deliver(Event::File {
            view: root()?,
            path: rel("A.java")?,
        })
        .map_err(err)?;
    registry
        .reconcile_start(&root()?, None, |_| changed, || git(1))
        .map_err(err)?;
    registry
        .summarised(&root()?, &rel("A.java")?, changed, 8, Stamp(1))
        .map_err(err)?;

    // when
    let refused = registry.publish(&root()?, &store);

    // then
    assert_eq!(refused, Err(ViewError::NotStored));
    assert_eq!(
        registry.view(&root()?).map(|v| v.published.generation.0),
        Some(1)
    );
    Ok(())
}
