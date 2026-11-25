//! Component tests of the view service (IDX-09): the shell over the registry, driven over the
//! testkit's fakes (a fake tree, a scripted repository, an in-memory store, a fake wall clock).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;

use jfl_application::source_tree::{
    FileStat, FsKind, ListMode, Listing, MTime, ReadBytes, SourceTree, SourceTreeError,
};
use jfl_application::syntax_indexer::{GrammarId, Indexed, SyntaxIndexError, SyntaxIndexer};
use jfl_application::{
    Answer, CountingClock, RepoProbeError, ServiceConfig, ServiceError, Summary, SummaryStore,
    ViewService, WallClock,
};
use jfl_domain::Language;
use jfl_domain::{FileUri, IncompleteReason, ModuleName, RelPath, SummaryBuilder};
use jfl_testkit::exec::block_on;
use jfl_testkit::fakes::clock::FakeWallClock;
use jfl_testkit::fakes::hasher::FnvHasher;
use jfl_testkit::fakes::repo_probe::FakeRepoProbe;
use jfl_testkit::fakes::source_tree::{FakeSourceTree, FakeWatch};
use jfl_testkit::fakes::summary_store::{Backing, MemorySummaryStore};

#[cfg(test)]
mod given {
    use super::*;

    /// Summarises any file to an empty summary, refuses one that starts with `BAD`, and counts calls.
    #[derive(Debug, Clone, Default)]
    pub(crate) struct Indexer(pub(crate) Arc<AtomicUsize>);

    impl SyntaxIndexer for Indexer {
        fn language(&self) -> Language {
            Language::Java
        }

        fn grammar(&self) -> GrammarId {
            GrammarId {
                name: "test-grammar",
                version: "1.0.0",
            }
        }

        fn summariser_version(&self) -> u32 {
            1
        }

        fn index(
            &self,
            bytes: Vec<u8>,
        ) -> impl Future<Output = Result<Indexed, SyntaxIndexError>> + Send {
            self.0.fetch_add(1, Ordering::Relaxed);
            core::future::ready(if bytes.starts_with(b"BAD") {
                Err(SyntaxIndexError::TooMuchWork)
            } else {
                Ok(Indexed {
                    summary: SummaryBuilder::new(Language::Java).finish(),
                    diagnostics: Vec::new(),
                    diagnostics_dropped: 0,
                })
            })
        }
    }

    pub(crate) type Service = ViewService<
        FakeSourceTree,
        FakeRepoProbe,
        Indexer,
        MemorySummaryStore,
        FakeWallClock,
        FnvHasher,
    >;

    /// One repository at `/w/m`, its files written to the tree and committed; the tree's root is `/w`.
    pub(crate) struct World {
        pub(crate) tree: FakeSourceTree,
        pub(crate) probe: FakeRepoProbe,
        pub(crate) indexer: Indexer,
        pub(crate) backing: Backing,
    }

    pub(crate) const MAIN: &str = "/w/m";
    pub(crate) const NESTED: &str = "/w/m/.claude/worktrees/n";

    pub(crate) fn uri(path: &str) -> FileUri {
        FileUri::from_path(path).unwrap()
    }

    pub(crate) fn rel(path: &str) -> RelPath {
        RelPath::new(path).unwrap()
    }

    impl World {
        pub(crate) fn new(files: &[(&str, &[u8])]) -> Self {
            Self::with_capacity(files, 1024)
        }

        pub(crate) fn with_capacity(files: &[(&str, &[u8])], hint_capacity: usize) -> Self {
            let tree = FakeSourceTree::new(uri("/w"), hint_capacity);
            let probe = FakeRepoProbe::new();
            for (path, bytes) in files {
                tree.write(&format!("m/{path}"), bytes).unwrap();
            }
            probe.init(&uri(MAIN), files).unwrap();
            Self {
                tree,
                probe,
                indexer: Indexer::default(),
                backing: Backing::new(),
            }
        }

        pub(crate) fn nested(&self, files: &[(&str, &[u8])]) {
            self.probe
                .add_worktree(&uri(MAIN), &uri(NESTED), "agent")
                .unwrap();
            self.tree
                .write("m/.claude/worktrees/n/.git", b"gitdir: x")
                .unwrap();
            for (path, bytes) in files {
                self.tree
                    .write(&format!("m/.claude/worktrees/n/{path}"), bytes)
                    .unwrap();
                self.probe
                    .write(&uri(&format!("{NESTED}/{path}")), bytes)
                    .unwrap();
            }
        }

        pub(crate) fn service(&self) -> Service {
            self.service_over(self.tree.clone())
        }

        /// A service that opens whole only views of at most `open_whole` files.
        pub(crate) fn deferring(&self, open_whole: usize) -> Service {
            let mut config = ServiceConfig::new("t1");
            config.open_whole = open_whole;
            self.service_with(self.tree.clone(), config)
        }

        /// A service over `tree` in place of the world's tree (a wrapper around it).
        pub(crate) fn service_over<T: SourceTree>(
            &self,
            tree: T,
        ) -> ViewService<T, FakeRepoProbe, Indexer, MemorySummaryStore, FakeWallClock, FnvHasher>
        {
            self.service_with(tree, ServiceConfig::new("t1"))
        }

        /// A service over `tree` whose clock is `clock`.
        pub(crate) fn service_clocked<T: SourceTree, C: WallClock + Clone>(
            &self,
            tree: T,
            clock: C,
        ) -> ViewService<T, FakeRepoProbe, Indexer, MemorySummaryStore, C, FnvHasher> {
            let store = MemorySummaryStore::open(
                self.backing.clone(),
                1 << 30,
                Arc::new(CountingClock::starting_at(1)),
            );
            ViewService::new(
                (tree, self.probe.clone(), self.indexer.clone(), store),
                clock,
                FnvHasher::default(),
                ServiceConfig::new("t1"),
            )
        }

        pub(crate) fn service_with<T: SourceTree>(
            &self,
            tree: T,
            config: ServiceConfig,
        ) -> ViewService<T, FakeRepoProbe, Indexer, MemorySummaryStore, FakeWallClock, FnvHasher>
        {
            let store = MemorySummaryStore::open(
                self.backing.clone(),
                1 << 30,
                Arc::new(CountingClock::starting_at(1)),
            );
            ViewService::new(
                (tree, self.probe.clone(), self.indexer.clone(), store),
                FakeWallClock::at(1_800_000_000),
                FnvHasher::default(),
                config,
            )
        }
    }

    pub(crate) fn ask(service: &mut Service, file: &str) -> Answer {
        block_on(service.request(&uri(file))).unwrap()
    }

    /// The summary the answer's generation holds for `path`.
    pub(crate) fn summary_of(answer: &Answer, path: &str) -> Option<Summary> {
        answer
            .generation
            .index
            .get(&rel(path))
            .and_then(|e| e.summary.clone())
    }

    pub(crate) fn indexed(world: &World) -> usize {
        world.indexer.0.load(Ordering::Relaxed)
    }

    pub(crate) fn reasons(answer: &Answer) -> Vec<IncompleteReason> {
        answer.completeness.reasons().to_vec()
    }

    /// The paths of the answer's generation.
    pub(crate) fn paths(answer: &Answer) -> Vec<String> {
        answer
            .generation
            .index
            .iter()
            .filter(|(_, e)| e.seen.stat.is_some())
            .map(|(p, _)| p.as_str().to_owned())
            .collect()
    }

    /// The fake tree, whose first listing writes `m/Late.java` once it has answered: a scan that
    /// started before the watch would never hear of that file.
    pub(crate) struct LateWriter(pub(crate) FakeSourceTree, pub(crate) AtomicBool);

    impl SourceTree for LateWriter {
        type Watch = FakeWatch;

        async fn list(&self, root: &FileUri, mode: ListMode) -> Result<Listing, SourceTreeError> {
            let listing = self.0.list(root, mode).await;
            if !self.1.swap(true, Ordering::Relaxed) {
                self.0.write("m/Late.java", b"class Late {}").unwrap();
            }
            listing
        }
        async fn read(
            &self,
            root: &FileUri,
            path: &RelPath,
            limit: usize,
        ) -> Result<ReadBytes, SourceTreeError> {
            self.0.read(root, path, limit).await
        }
        async fn stat(&self, root: &FileUri, path: &RelPath) -> Result<FileStat, SourceTreeError> {
            self.0.stat(root, path).await
        }
        async fn watch(&self, root: &FileUri) -> Result<FakeWatch, SourceTreeError> {
            self.0.watch(root).await
        }
        async fn filesystem(&self, root: &FileUri) -> Result<FsKind, SourceTreeError> {
            self.0.filesystem(root).await
        }
        async fn write_cookie(
            &self,
            root: &FileUri,
            name: &RelPath,
        ) -> Result<(), SourceTreeError> {
            self.0.write_cookie(root, name).await
        }
    }

    /// The fake tree, recording the paths of every prefetch it is asked for, in order.
    pub(crate) struct Prefetches(
        pub(crate) FakeSourceTree,
        pub(crate) Arc<Mutex<Vec<Vec<String>>>>,
    );

    impl SourceTree for Prefetches {
        type Watch = FakeWatch;

        async fn list(&self, root: &FileUri, mode: ListMode) -> Result<Listing, SourceTreeError> {
            self.0.list(root, mode).await
        }
        async fn read(
            &self,
            root: &FileUri,
            path: &RelPath,
            limit: usize,
        ) -> Result<ReadBytes, SourceTreeError> {
            self.0.read(root, path, limit).await
        }
        async fn stat(&self, root: &FileUri, path: &RelPath) -> Result<FileStat, SourceTreeError> {
            self.0.stat(root, path).await
        }
        async fn prefetch(&self, root: &FileUri, paths: &[RelPath], limit: usize) {
            self.1
                .lock()
                .unwrap()
                .push(paths.iter().map(|p| p.as_str().to_owned()).collect());
            self.0.prefetch(root, paths, limit).await;
        }
        async fn watch(&self, root: &FileUri) -> Result<FakeWatch, SourceTreeError> {
            self.0.watch(root).await
        }
        async fn filesystem(&self, root: &FileUri) -> Result<FsKind, SourceTreeError> {
            self.0.filesystem(root).await
        }
        async fn write_cookie(
            &self,
            root: &FileUri,
            name: &RelPath,
        ) -> Result<(), SourceTreeError> {
            self.0.write_cookie(root, name).await
        }
    }

    /// The fake wall clock, counting the deadlines that were waited out: each resolves at its
    /// first poll, as the fake's do, and counts it.
    #[derive(Debug, Clone)]
    pub(crate) struct DeadlinesCounted(pub(crate) FakeWallClock, pub(crate) Arc<AtomicUsize>);

    impl WallClock for DeadlinesCounted {
        fn now(&self) -> MTime {
            self.0.now()
        }

        fn deadline(&self, after: Duration) -> impl Future<Output = ()> + Send {
            let _ = after;
            let expired = Arc::clone(&self.1);
            std::future::poll_fn(move |_| {
                expired.fetch_add(1, Ordering::Relaxed);
                Poll::Ready(())
            })
        }
    }

    /// The fake tree, whose first cookie is written while a rescan waits on the watch (FSEvents'
    /// event on the root): the cookie's events fold into that rescan and are never hinted alone.
    pub(crate) struct RescanAtCookie(pub(crate) FakeSourceTree, pub(crate) AtomicBool);

    impl SourceTree for RescanAtCookie {
        type Watch = FakeWatch;

        async fn list(&self, root: &FileUri, mode: ListMode) -> Result<Listing, SourceTreeError> {
            self.0.list(root, mode).await
        }
        async fn read(
            &self,
            root: &FileUri,
            path: &RelPath,
            limit: usize,
        ) -> Result<ReadBytes, SourceTreeError> {
            self.0.read(root, path, limit).await
        }
        async fn stat(&self, root: &FileUri, path: &RelPath) -> Result<FileStat, SourceTreeError> {
            self.0.stat(root, path).await
        }
        async fn watch(&self, root: &FileUri) -> Result<FakeWatch, SourceTreeError> {
            self.0.watch(root).await
        }
        async fn filesystem(&self, root: &FileUri) -> Result<FsKind, SourceTreeError> {
            self.0.filesystem(root).await
        }
        async fn write_cookie(
            &self,
            root: &FileUri,
            name: &RelPath,
        ) -> Result<(), SourceTreeError> {
            if !self.1.swap(true, Ordering::Relaxed) {
                self.0.event_on_root();
            }
            self.0.write_cookie(root, name).await
        }
    }
}
use given::{
    DeadlinesCounted, LateWriter, MAIN, NESTED, Prefetches, RescanAtCookie, World, ask, indexed,
    paths, reasons, rel, summary_of, uri,
};

#[test]
fn should_answer_complete_when_the_view_is_watched_and_nothing_changed() {
    // given
    let world = World::new(&[("A.java", b"class A {}"), ("B.java", b"class B {}")]);
    let mut service = world.service();

    // when
    let answer = ask(&mut service, "/w/m/A.java");

    // then
    assert!(answer.completeness.is_complete(), "{:?}", reasons(&answer));
    assert_eq!(answer.view, uri(MAIN));
    assert_eq!(answer.complete_view, uri(MAIN));
    assert_eq!(answer.cookie_round_trips.len(), 1);
    assert_eq!(answer.generation.index.len(), 2);
}

#[test]
fn should_bind_a_nested_worktree_file_to_its_own_view_when_the_parent_is_open() {
    // given
    let world = World::new(&[("A.java", b"class A {}")]);
    world.nested(&[("N.java", b"class N {}")]);
    let mut service = world.service();
    let parent = ask(&mut service, "/w/m/A.java");
    service.finish(parent.id).unwrap();

    // when
    let nested = ask(&mut service, "/w/m/.claude/worktrees/n/N.java");

    // then
    assert_eq!(nested.view, uri(NESTED));
    assert_eq!(nested.complete_view, uri(NESTED));
    assert!(nested.generation.index.contains_key(&rel("N.java")));
    let parent_view = service.registry().view(&uri(MAIN)).unwrap();
    assert!(
        parent_view
            .published
            .index
            .keys()
            .all(|p| !p.as_str().starts_with(".claude")),
        "{:?}",
        parent_view.published.index.keys().collect::<Vec<_>>()
    );
    assert!(nested.completeness.is_complete(), "{:?}", reasons(&nested));
}

#[test]
fn should_never_claim_complete_when_the_filesystem_cannot_be_watched() {
    // given
    let world = World::new(&[("A.java", b"class A {}")]);
    world.tree.set_filesystem(FsKind::Network);
    let mut service = world.service();

    // when
    let answer = ask(&mut service, "/w/m/A.java");

    // then
    assert!(!answer.completeness.is_complete());
    assert!(reasons(&answer).contains(&IncompleteReason::NotWatched));
    assert!(answer.cookie_round_trips.is_empty());
    assert_eq!(service.filesystem(&uri(MAIN)), Some(FsKind::Network));
}

#[test]
fn should_answer_incomplete_when_the_cookie_misses_its_deadline() {
    // given
    let world = World::new(&[("A.java", b"class A {}")]);
    world.tree.lose_cookies(true);
    let mut service = world.service();

    // when
    let answer = ask(&mut service, "/w/m/A.java");

    // then
    assert!(!answer.completeness.is_complete());
    assert!(
        reasons(&answer)
            .iter()
            .any(|r| matches!(r, IncompleteReason::PendingChanges { .. })),
        "{:?}",
        reasons(&answer)
    );
    assert!(answer.cookie_round_trips.is_empty());
}

#[test]
fn should_answer_incomplete_when_a_rescan_comes_in_place_of_the_cookie() {
    // given: the first cookie folds into a waiting Unnamed rescan, every time
    let world = World::new(&[("A.java", b"class A {}")]);
    let expired = Arc::new(AtomicUsize::new(0));
    let mut service = world.service_clocked(
        RescanAtCookie(world.tree.clone(), AtomicBool::new(false)),
        DeadlinesCounted(FakeWallClock::at(1_800_000_000), Arc::clone(&expired)),
    );

    // when
    let first = block_on(service.request(&uri("/w/m/A.java"))).unwrap();
    let first_expired = expired.load(Ordering::Relaxed);
    service.finish(first.id).unwrap();
    let second = block_on(service.request(&uri("/w/m/A.java"))).unwrap();

    // then: the rescan loses the cookie at once (no deadline waited out) and the answer is
    // incomplete; the rescan certifies the view again, so the next request is complete
    assert_eq!(first_expired, 0);
    assert!(!first.completeness.is_complete());
    assert!(
        reasons(&first)
            .iter()
            .any(|r| matches!(r, IncompleteReason::PendingChanges { .. })),
        "{:?}",
        reasons(&first)
    );
    assert!(first.cookie_round_trips.is_empty());
    assert!(second.completeness.is_complete(), "{:?}", reasons(&second));
    assert_eq!(second.cookie_round_trips.len(), 1);
}

#[test]
fn should_answer_from_the_new_content_when_a_write_lands_before_the_request() {
    // given
    let world = World::new(&[("A.java", b"class A {}")]);
    let mut service = world.service();
    let first = ask(&mut service, "/w/m/A.java");
    service.finish(first.id).unwrap();
    world.tree.write("m/A.java", b"class A { int x; }").unwrap();

    // when
    let answer = ask(&mut service, "/w/m/A.java");

    // then
    let old = first.generation.index.get(&rel("A.java")).unwrap();
    let new = answer.generation.index.get(&rel("A.java")).unwrap();
    assert_ne!(old.summary, new.summary);
    assert!(answer.generation.generation > first.generation.generation);
    assert!(answer.completeness.is_complete(), "{:?}", reasons(&answer));
    assert_eq!(answer.cookie_round_trips.len(), 2);
}

#[test]
fn should_read_a_file_created_during_an_overflow_when_the_rescan_lists_it() {
    // given
    let world = World::with_capacity(&[("A.java", b"class A {}")], 1);
    let mut service = world.service();
    let first = ask(&mut service, "/w/m/A.java");
    service.finish(first.id).unwrap();
    world.tree.write("m/B.java", b"class B {}").unwrap();
    world.tree.write("m/C.java", b"class C {}").unwrap();

    // when
    let answer = ask(&mut service, "/w/m/A.java");

    // then
    assert!(answer.generation.index.contains_key(&rel("C.java")));
    assert!(answer.generation.index.contains_key(&rel("B.java")));
    assert!(answer.completeness.is_complete(), "{:?}", reasons(&answer));
}

#[test]
fn should_watch_again_and_rescan_when_the_watch_degrades() {
    // given
    let world = World::new(&[("A.java", b"class A {}")]);
    let mut service = world.service();
    let first = ask(&mut service, "/w/m/A.java");
    service.finish(first.id).unwrap();
    world.tree.break_watches();
    world.tree.write("m/D.java", b"class D {}").unwrap();

    // when
    let answer = ask(&mut service, "/w/m/A.java");

    // then
    assert!(answer.generation.index.contains_key(&rel("D.java")));
    let view = service.registry().view(&uri(MAIN)).unwrap();
    assert!(view.watched && view.certain);
    assert!(answer.completeness.is_complete(), "{:?}", reasons(&answer));
}

#[test]
fn should_index_a_tracked_file_under_build_output_when_the_listing_skips_it() {
    // given
    let world = World::new(&[
        ("build.gradle", b"plugins {}"),
        ("A.java", b"class A {}"),
        ("build/Gen.java", b"class Gen {}"),
    ]);
    let mut service = world.service();

    // when
    let answer = ask(&mut service, "/w/m/A.java");

    // then
    assert!(answer.generation.index.contains_key(&rel("build/Gen.java")));
    assert!(answer.completeness.is_complete(), "{:?}", reasons(&answer));
}

#[test]
fn should_rebuild_when_a_commit_moves_the_branch_and_head_stays() {
    // given
    let world = World::new(&[("A.java", b"class A {}")]);
    let mut service = world.service();
    let first = ask(&mut service, "/w/m/A.java");
    service.finish(first.id).unwrap();
    world
        .probe
        .write(&uri("/w/m/A.java"), b"class A {}")
        .unwrap();
    world.probe.stage(&uri(MAIN), "A.java").unwrap();
    world.probe.commit(&uri(MAIN)).unwrap();

    // when
    let answer = ask(&mut service, "/w/m/A.java");

    // then
    assert!(answer.generation.generation > first.generation.generation);
    assert_ne!(answer.generation.git, first.generation.git);
    assert!(answer.completeness.is_complete(), "{:?}", reasons(&answer));
}

#[test]
fn should_count_a_refused_file_when_the_indexer_refuses_it() {
    // given
    let world = World::new(&[("A.java", b"class A {}"), ("Bad.java", b"BAD")]);
    let mut service = world.service();

    // when
    let answer = ask(&mut service, "/w/m/A.java");

    // then
    assert!(matches!(
        answer
            .generation
            .index
            .get(&rel("Bad.java"))
            .and_then(|e| e.summary.as_ref()),
        Some(Summary::Gap)
    ));
    assert!(
        reasons(&answer).contains(&IncompleteReason::ModulePartlyIndexed {
            module: ModuleName::worktree(),
            indexed: 1,
            files: 2,
        })
    );
}

#[test]
fn should_free_a_generation_when_the_request_that_pinned_it_finishes() {
    // given
    let world = World::new(&[("A.java", b"class A {}")]);
    let mut service = world.service();
    let first = ask(&mut service, "/w/m/A.java");
    let pinned = Arc::downgrade(&first.generation);
    let id = first.id;
    drop(first);
    world.tree.write("m/A.java", b"class A { int y; }").unwrap();
    let second = ask(&mut service, "/w/m/A.java");
    let kept = pinned.upgrade().is_some();

    // when
    service.finish(id).unwrap();

    // then
    assert!(kept);
    assert!(pinned.upgrade().is_none());
    assert!(service.registry().request(id).is_none());
    drop(second);
}

#[test]
fn should_publish_when_the_store_lost_a_summary_the_generation_names() {
    // given
    let world = World::new(&[("A.java", b"class A {}"), ("B.java", b"class B {}")]);
    let mut service = world.service();
    let first = ask(&mut service, "/w/m/A.java");
    service.finish(first.id).unwrap();
    assert_eq!(block_on(service.write_back(usize::MAX)), 0);
    world.backing.wipe();
    world.tree.write("m/B.java", b"class B { int z; }").unwrap();

    // when
    let answer = ask(&mut service, "/w/m/A.java");

    // then
    assert_eq!(block_on(service.write_back(usize::MAX)), 0);
    assert_eq!(service.ports().2.stats().entries, 1);
    assert!(answer.generation.generation > first.generation.generation);
    assert!(answer.completeness.is_complete(), "{:?}", reasons(&answer));
}

#[test]
fn should_read_from_the_store_when_a_second_view_holds_the_same_content() {
    // given
    let world = World::new(&[("A.java", b"class A {}")]);
    world.nested(&[("A.java", b"class A {}")]);
    let mut service = world.service();
    let first = ask(&mut service, "/w/m/A.java");
    service.finish(first.id).unwrap();
    let indexed = world.indexer.0.load(Ordering::Relaxed);

    assert_eq!(block_on(service.write_back(usize::MAX)), 0);

    // when
    let nested = ask(&mut service, "/w/m/.claude/worktrees/n/A.java");

    // then
    assert_eq!(world.indexer.0.load(Ordering::Relaxed), indexed);
    assert!(nested.completeness.is_complete(), "{:?}", reasons(&nested));
}

#[test]
fn should_index_the_same_content_once_when_a_second_view_opens_before_the_write_back() {
    // given
    let world = World::new(&[("A.java", b"class A {}")]);
    world.nested(&[("A.java", b"class A {}")]);
    let mut service = world.service();
    let first = ask(&mut service, "/w/m/A.java");
    service.finish(first.id).unwrap();

    // when
    let nested = ask(&mut service, "/w/m/.claude/worktrees/n/A.java");

    // then
    assert_eq!(indexed(&world), 1);
    assert_eq!(service.ports().2.stats().entries, 0);
    assert!(nested.completeness.is_complete(), "{:?}", reasons(&nested));
}

#[test]
fn should_store_the_summaries_only_when_written_back() {
    // given
    let world = World::new(&[
        ("A.java", b"class A {}"),
        ("B.java", b"class B {}"),
        ("C.java", b"class C {}"),
    ]);
    let mut service = world.service();
    let answer = ask(&mut service, "/w/m/A.java");
    service.finish(answer.id).unwrap();

    // when
    let stored_before = service.ports().2.stats().entries;
    let left = block_on(service.write_back(1));
    let stored_after_one = service.ports().2.stats().entries;
    let left_at_end = block_on(service.write_back(usize::MAX));

    // then
    assert_eq!(stored_before, 0);
    assert_eq!((left, stored_after_one), (2, 1));
    assert_eq!(left_at_end, 0);
    assert_eq!(service.ports().2.stats().entries, 3);
}

/// Two packages, `p` and `q`, in a view that opens whole only up to no files at all.
fn two_packages() -> (World, given::Service) {
    let world = World::new(&[
        ("p/A.java", b"class A {}"),
        ("p/B.java", b"class B {}"),
        ("q/C.java", b"class C {}"),
        ("q/D.java", b"class D {}"),
    ]);
    let service = world.deferring(0);
    (world, service)
}

#[test]
fn should_answer_partly_indexed_and_then_complete_when_the_open_defers_the_rest() {
    // given
    let (world, mut service) = two_packages();

    // when
    let first = ask(&mut service, "/w/m/p/A.java");
    service.finish(first.id).unwrap();
    let more = block_on(service.settle(usize::MAX)).unwrap();
    let after = ask(&mut service, "/w/m/p/A.java");

    // then
    assert!(!first.completeness.is_complete());
    assert!(
        reasons(&first).contains(&IncompleteReason::ModulePartlyIndexed {
            module: ModuleName::worktree(),
            indexed: 2,
            files: 4,
        }),
        "{:?}",
        reasons(&first)
    );
    assert_eq!(summary_of(&first, "q/C.java"), Some(Summary::Pending));
    assert!(matches!(
        summary_of(&first, "p/B.java"),
        Some(Summary::Indexed { .. })
    ));
    assert!(!more);
    assert_eq!(indexed(&world), 4);
    assert!(after.completeness.is_complete(), "{:?}", reasons(&after));
    assert!(matches!(
        summary_of(&after, "q/D.java"),
        Some(Summary::Indexed { .. })
    ));
    assert!(!block_on(service.settle(usize::MAX)).unwrap());
}

#[test]
fn should_publish_a_generation_per_slice_and_answer_between_them_when_settled_in_slices() {
    // given: a view whose open deferred `q/C.java` and `q/D.java`
    let (world, mut service) = two_packages();
    let first = ask(&mut service, "/w/m/p/A.java");
    service.finish(first.id).unwrap();
    let opened = first.generation.generation;

    // when: one slice of one file, a request on an indexed file, a request on the file still
    // deferred, and the next slice
    let more = block_on(service.settle(1)).unwrap();
    let between = ask(&mut service, "/w/m/p/A.java");
    service.finish(between.id).unwrap();
    let read_after_one = indexed(&world);
    let deferred = ask(&mut service, "/w/m/q/D.java");
    service.finish(deferred.id).unwrap();
    let more_after = block_on(service.settle(1)).unwrap();
    let after = ask(&mut service, "/w/m/p/A.java");

    // then: the slice published C alone, and the answer between the slices came from it,
    // incomplete while D waited; the request on D read D alone, which left the settle nothing
    assert!(more);
    assert_eq!(read_after_one, 3);
    assert_eq!(between.generation.generation.0, opened.0 + 1);
    assert!(matches!(
        summary_of(&between, "q/C.java"),
        Some(Summary::Indexed { .. })
    ));
    assert_eq!(summary_of(&between, "q/D.java"), Some(Summary::Pending));
    assert!(!between.completeness.is_complete());
    assert!(
        reasons(&between)
            .iter()
            .any(|r| matches!(r, IncompleteReason::PendingChanges { .. })),
        "{:?}",
        reasons(&between)
    );
    assert!(matches!(
        summary_of(&deferred, "q/D.java"),
        Some(Summary::Indexed { .. })
    ));
    assert!(!more_after);
    assert_eq!(indexed(&world), 4);
    assert!(after.completeness.is_complete(), "{:?}", reasons(&after));
}

#[test]
fn should_prefetch_the_slice_and_the_next_when_settling() {
    // given: a view whose open read `p/` and deferred `q/C.java`, `q/D.java` and `q/E.java`
    let world = World::new(&[
        ("p/A.java", b"class A {}"),
        ("q/C.java", b"class C {}"),
        ("q/D.java", b"class D {}"),
        ("q/E.java", b"class E {}"),
    ]);
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut config = ServiceConfig::new("t1");
    config.open_whole = 0;
    let mut service =
        world.service_with(Prefetches(world.tree.clone(), Arc::clone(&asked)), config);
    let first = block_on(service.request(&uri("/w/m/p/A.java"))).unwrap();
    service.finish(first.id).unwrap();
    let at_open = asked.lock().unwrap().clone();

    // when: the settle in slices of one file
    let mut steps = 0;
    while block_on(service.settle(1)).unwrap() {
        steps += 1;
    }
    let settled = block_on(service.request(&uri("/w/m/p/A.java"))).unwrap();

    // then: nothing was prefetched for the open's own reads; each slice prefetched its file and
    // the next slice's, and the view settled complete, every file indexed once
    assert!(at_open.is_empty(), "{at_open:?}");
    assert_eq!(steps, 2);
    assert_eq!(
        *asked.lock().unwrap(),
        [
            vec!["q/C.java", "q/D.java"],
            vec!["q/D.java", "q/E.java"],
            vec!["q/E.java"],
        ]
    );
    assert!(
        settled.completeness.is_complete(),
        "{:?}",
        reasons(&settled)
    );
    assert_eq!(indexed(&world), 4);
}

#[test]
fn should_leave_the_deferred_files_to_the_settle_when_a_request_finds_a_commit() {
    // given: a view whose open deferred `q/C.java` and `q/D.java`, and a commit no watcher sees
    // (a linked worktree's `HEAD` lives outside its root)
    let (world, mut service) = two_packages();
    let first = ask(&mut service, "/w/m/p/A.java");
    service.finish(first.id).unwrap();
    world
        .probe
        .write(&uri("/w/m/p/A.java"), b"class A {}")
        .unwrap();
    world.probe.stage(&uri(MAIN), "p/A.java").unwrap();
    world.probe.commit(&uri(MAIN)).unwrap();

    // when: a request's stat sees the commit, so it runs the rescan; then the settle's slices
    let answer = ask(&mut service, "/w/m/p/A.java");
    service.finish(answer.id).unwrap();
    let read_by_the_request = indexed(&world);
    let more = block_on(service.settle(1)).unwrap();
    let read_by_one_slice = indexed(&world);
    let more_after = block_on(service.settle(1)).unwrap();
    let settled = ask(&mut service, "/w/m/p/A.java");

    // then: the rescan published the new HEAD and read no deferred file, so the answer is
    // incomplete with both still pending; each slice then read one
    assert_ne!(answer.generation.git, first.generation.git);
    assert_eq!(read_by_the_request, 2);
    assert_eq!(summary_of(&answer, "q/C.java"), Some(Summary::Pending));
    assert_eq!(summary_of(&answer, "q/D.java"), Some(Summary::Pending));
    assert!(!answer.completeness.is_complete());
    assert!(more);
    assert_eq!(read_by_one_slice, 3);
    assert!(!more_after);
    assert_eq!(indexed(&world), 4);
    assert!(
        settled.completeness.is_complete(),
        "{:?}",
        reasons(&settled)
    );
}

#[test]
fn should_read_the_requested_pending_file_and_leave_the_rest_when_a_request_finds_a_commit() {
    // given: a view whose open deferred `q/C.java` and `q/D.java`, and a commit no watcher sees
    let (world, mut service) = two_packages();
    let first = ask(&mut service, "/w/m/p/A.java");
    service.finish(first.id).unwrap();
    world
        .probe
        .write(&uri("/w/m/p/A.java"), b"class A {}")
        .unwrap();
    world.probe.stage(&uri(MAIN), "p/A.java").unwrap();
    world.probe.commit(&uri(MAIN)).unwrap();

    // when: a request on a file still pending sees the commit and runs the rescan inline; it needs
    // its own file, so the rescan may not leave it
    let answer = ask(&mut service, "/w/m/q/C.java");

    // then: the rescan read the requested file and left the other deferred one
    assert_ne!(answer.generation.git, first.generation.git);
    assert!(matches!(
        summary_of(&answer, "q/C.java"),
        Some(Summary::Indexed { .. })
    ));
    assert_eq!(summary_of(&answer, "q/D.java"), Some(Summary::Pending));
    assert_eq!(indexed(&world), 3);
    assert!(!answer.completeness.is_complete());
}

#[test]
fn should_read_one_slice_per_step_when_a_hint_asks_for_a_rescan_during_the_settle() {
    // given: a view settling one file a slice, its first slice read
    let (world, mut service) = two_packages();
    let first = ask(&mut service, "/w/m/p/A.java");
    service.finish(first.id).unwrap();
    assert!(block_on(service.settle(1)).unwrap());
    let before = indexed(&world);

    // when: a hint that names no Java file (as a `.git` change does) asks for a rescan, and the
    // settle steps on
    world.tree.write("m/build.gradle", b"plugins {}").unwrap();
    let more = block_on(service.settle(1)).unwrap();
    let read_by_the_rescan = indexed(&world) - before;
    let more_after = block_on(service.settle(1)).unwrap();
    let settled = ask(&mut service, "/w/m/p/A.java");

    // then: the step that ran the rescan read no deferred file and left one; the next read it
    assert_eq!(before, 3);
    assert!(more);
    assert_eq!(read_by_the_rescan, 0);
    assert!(!more_after);
    assert_eq!(indexed(&world), 4);
    assert!(
        settled.completeness.is_complete(),
        "{:?}",
        reasons(&settled)
    );
}

#[test]
fn should_read_a_file_created_during_the_settle_and_leave_the_deferred_ones_when_it_is_asked_for() {
    // given: a view whose open deferred `q/C.java` and `q/D.java`
    let (world, mut service) = two_packages();
    let first = ask(&mut service, "/w/m/p/A.java");
    service.finish(first.id).unwrap();

    // when: a file is created in the view and asked for: a part takes it, finds it unknown to
    // the generation and asks for a rescan, whose listing admits it
    world.tree.write("m/q/E.java", b"class E {}").unwrap();
    let answer = ask(&mut service, "/w/m/q/E.java");

    // then: the request read the new file alone, and the deferred ones wait for the settle
    assert_eq!(indexed(&world), 3);
    assert!(matches!(
        summary_of(&answer, "q/E.java"),
        Some(Summary::Indexed { .. })
    ));
    assert_eq!(summary_of(&answer, "q/C.java"), Some(Summary::Pending));
    assert_eq!(summary_of(&answer, "q/D.java"), Some(Summary::Pending));
    assert!(!answer.completeness.is_complete());
}

#[test]
fn should_read_only_the_requested_file_when_a_request_finds_it_pending() {
    // given
    let (world, mut service) = two_packages();
    let first = ask(&mut service, "/w/m/p/A.java");
    service.finish(first.id).unwrap();

    // when
    let answer = ask(&mut service, "/w/m/q/C.java");

    // then
    assert_eq!(indexed(&world), 3);
    assert!(matches!(
        summary_of(&answer, "q/C.java"),
        Some(Summary::Indexed { .. })
    ));
    assert_eq!(summary_of(&answer, "q/D.java"), Some(Summary::Pending));
    assert!(!answer.completeness.is_complete());
}

#[test]
fn should_settle_from_the_disk_when_a_pending_file_changes_before_settle() {
    // given
    let (world, mut service) = two_packages();
    let first = ask(&mut service, "/w/m/p/A.java");
    service.finish(first.id).unwrap();
    world
        .tree
        .write("m/q/C.java", b"class C { int longer; }")
        .unwrap();

    // when
    block_on(service.settle(usize::MAX)).unwrap();
    let answer = ask(&mut service, "/w/m/q/C.java");

    // then
    let entry = answer.generation.index.get(&rel("q/C.java")).unwrap();
    assert_eq!(entry.seen.stat.map(|s| s.size), Some(23));
    assert!(answer.completeness.is_complete(), "{:?}", reasons(&answer));
}

#[test]
fn should_drop_a_pending_file_from_the_count_when_it_is_deleted_before_settle() {
    // given
    let (world, mut service) = two_packages();
    let first = ask(&mut service, "/w/m/p/A.java");
    service.finish(first.id).unwrap();
    world.tree.remove("m/q/D.java").unwrap();

    // when
    block_on(service.settle(usize::MAX)).unwrap();
    let answer = ask(&mut service, "/w/m/p/A.java");

    // then
    assert_eq!(indexed(&world), 3);
    assert_eq!(summary_of(&answer, "q/D.java"), Some(Summary::Absent));
    assert!(answer.completeness.is_complete(), "{:?}", reasons(&answer));
}

#[test]
fn should_keep_at_most_the_cap_for_the_store_when_more_summaries_wait() {
    // given
    let world = World::new(&[
        ("A.java", b"class A {}"),
        ("B.java", b"class B {}"),
        ("C.java", b"class C {}"),
    ]);
    let mut config = ServiceConfig::new("t1");
    config.unwritten_cap = 2;
    let mut service = world.service_with(world.tree.clone(), config);
    let answer = ask(&mut service, "/w/m/A.java");
    service.finish(answer.id).unwrap();

    // when
    let left = block_on(service.write_back(usize::MAX));

    // then
    assert_eq!(left, 0);
    assert_eq!(service.ports().2.stats().entries, 2);
    assert!(answer.completeness.is_complete(), "{:?}", reasons(&answer));
}

#[test]
fn should_read_every_pending_file_when_sync_runs_before_settle() {
    // given
    let (world, mut service) = two_packages();
    let first = ask(&mut service, "/w/m/p/A.java");
    service.finish(first.id).unwrap();

    // when
    block_on(service.sync(&uri("/w/m/p/A.java"))).unwrap();
    let answer = ask(&mut service, "/w/m/p/A.java");

    // then
    assert_eq!(indexed(&world), 4);
    assert!(answer.completeness.is_complete(), "{:?}", reasons(&answer));
    assert!(!block_on(service.settle(usize::MAX)).unwrap());
}

#[test]
fn should_refuse_the_request_when_the_worktree_cannot_be_resolved() {
    // given
    let world = World::new(&[("A.java", b"class A {}")]);
    let io = RepoProbeError::Io(std::io::ErrorKind::Other);
    world.probe.fail_with(Some(io));
    let mut service = world.service();

    // when
    let refused = block_on(service.request(&uri("/w/m/A.java")));
    let outside = {
        world.probe.fail_with(None);
        block_on(service.request(&uri("/elsewhere/A.java")))
    };

    // then
    assert_eq!(refused.err(), Some(ServiceError::Probe(io)));
    assert_eq!(outside.err(), Some(ServiceError::NotInWorktree));
}

#[test]
fn should_return_from_sync_when_a_rescan_started_after_it_has_published() {
    // given
    let world = World::new(&[("A.java", b"class A {}")]);
    world.tree.lose_cookies(true);
    let mut service = world.service();
    let first = ask(&mut service, "/w/m/A.java");
    service.finish(first.id).unwrap();

    // when
    let generation = block_on(service.sync(&uri("/w/m/A.java"))).unwrap();

    // then
    assert!(generation > first.generation.generation);
    let view = service.registry().view(&uri(MAIN)).unwrap();
    assert!(view.certain);
}

#[test]
fn should_keep_a_nested_worktree_file_out_of_the_parent_when_it_is_edited() {
    // given
    let world = World::new(&[("A.java", b"class A {}")]);
    world.tree.mkdir("m/sub/.git").unwrap();
    world.tree.write("m/sub/X.java", b"class X {}").unwrap();
    let mut service = world.service();
    let first = ask(&mut service, "/w/m/A.java");
    service.finish(first.id).unwrap();
    world
        .tree
        .write("m/sub/X.java", b"class X { int a; }")
        .unwrap();

    // when
    let answer = ask(&mut service, "/w/m/A.java");

    // then
    assert_eq!(paths(&answer), ["A.java"]);
    assert!(answer.completeness.is_complete(), "{:?}", reasons(&answer));
}

#[test]
fn should_keep_a_new_worktree_out_of_the_parent_when_it_is_created_under_it() {
    // given
    let world = World::new(&[("A.java", b"class A {}")]);
    let mut service = world.service();
    let first = ask(&mut service, "/w/m/A.java");
    service.finish(first.id).unwrap();
    world.tree.write("m/wt/.git", b"gitdir: elsewhere").unwrap();
    world.tree.write("m/wt/Y.java", b"class Y {}").unwrap();

    // when
    let answer = ask(&mut service, "/w/m/A.java");

    // then
    assert_eq!(paths(&answer), ["A.java"]);
    assert!(answer.completeness.is_complete(), "{:?}", reasons(&answer));
}

#[test]
fn should_keep_an_ignored_file_out_of_the_view_when_it_is_hinted_or_asked_for() {
    // given
    let world = World::new(&[("A.java", b"class A {}")]);
    world.tree.write("m/.gitignore", b"gen/").unwrap();
    let mut service = world.service();
    let first = ask(&mut service, "/w/m/A.java");
    service.finish(first.id).unwrap();
    world.tree.write("m/gen/G.java", b"class G {}").unwrap();

    // when
    let hinted = ask(&mut service, "/w/m/A.java");
    service.finish(hinted.id).unwrap();
    let asked = ask(&mut service, "/w/m/gen/G.java");

    // then
    assert_eq!(paths(&hinted), ["A.java"]);
    assert_eq!(paths(&asked), ["A.java"]);
    assert!(hinted.completeness.is_complete(), "{:?}", reasons(&hinted));
}

#[test]
fn should_drop_a_nested_view_s_files_from_the_parent_when_the_nested_view_opens() {
    // given
    let world = World::new(&[("A.java", b"class A {}"), ("later/Z.java", b"class Z {}")]);
    let mut service = world.service();
    let before = ask(&mut service, "/w/m/A.java");
    service.finish(before.id).unwrap();
    world
        .probe
        .add_worktree(&uri(MAIN), &uri("/w/m/later"), "later")
        .unwrap();
    world.tree.write("m/later/.git", b"gitdir: x").unwrap();
    let nested = ask(&mut service, "/w/m/later/Z.java");
    service.finish(nested.id).unwrap();

    // when
    let after = ask(&mut service, "/w/m/A.java");

    // then
    assert!(paths(&before).contains(&"later/Z.java".to_owned()));
    assert_eq!(nested.view, uri("/w/m/later"));
    assert_eq!(paths(&after), ["A.java"]);
    assert!(after.completeness.is_complete(), "{:?}", reasons(&after));
}

#[test]
fn should_hear_of_a_file_written_during_the_open_scan_when_the_watch_came_first() {
    // given
    let world = World::new(&[("A.java", b"class A {}")]);
    let mut service = world.service_over(LateWriter(world.tree.clone(), AtomicBool::new(false)));

    // when
    let answer = block_on(service.request(&uri("/w/m/A.java"))).unwrap();

    // then
    assert!(world.tree.exists("m/Late.java"));
    assert_eq!(paths(&answer), ["A.java", "Late.java"]);
    assert!(answer.completeness.is_complete(), "{:?}", reasons(&answer));
}

#[test]
fn should_rescan_when_a_hint_names_no_java_file() {
    // given
    let world = World::new(&[("A.java", b"class A {}")]);
    let mut service = world.service();
    let first = ask(&mut service, "/w/m/A.java");
    service.finish(first.id).unwrap();
    world.tree.write("m/build.gradle", b"plugins {}").unwrap();

    // when
    let answer = ask(&mut service, "/w/m/A.java");

    // then
    assert!(answer.generation.generation > first.generation.generation);
    assert!(answer.completeness.is_complete(), "{:?}", reasons(&answer));
}

#[test]
fn should_count_other_language_files_again_when_one_is_created_or_removed() {
    // given
    let world = World::new(&[("A.java", b"class A {}")]);
    let mut service = world.service();
    let first = ask(&mut service, "/w/m/A.java");
    service.finish(first.id).unwrap();
    let a = jfl_domain::RelPath::new("A.java").unwrap();

    // when: a Kotlin file created, edited, then removed
    world.tree.write("m/src/L.kt", b"fun f() {}").unwrap();
    let created = ask(&mut service, "/w/m/A.java");
    service.finish(created.id).unwrap();
    world.tree.write("m/src/L.kt", b"fun g() {}").unwrap();
    let edited = ask(&mut service, "/w/m/A.java");
    service.finish(edited.id).unwrap();
    world.tree.remove("m/src/L.kt").unwrap();
    let removed = ask(&mut service, "/w/m/A.java");

    // then: a create or remove rescans and counts again; an edit costs no rescan
    assert_eq!(first.other_sources.seen_from(&a), 0);
    assert!(created.generation.generation > first.generation.generation);
    assert_eq!(created.other_sources.seen_from(&a), 1);
    assert_eq!(edited.generation.generation, created.generation.generation);
    assert_eq!(edited.other_sources.seen_from(&a), 1);
    assert!(removed.generation.generation > edited.generation.generation);
    assert_eq!(removed.other_sources.seen_from(&a), 0);
}

#[test]
fn should_count_a_tracked_other_language_file_when_the_listing_cannot_see_it() {
    // given: a Kotlin file the index tracks under a `.gitignore`d directory, and one under a
    // build's output directory, which the listing skips
    let world = World::new(&[
        ("A.java", b"class A {}"),
        (".gitignore", b"gen/\n"),
        ("gen/L.kt", b"fun f() {}"),
        ("build.gradle", b"plugins {}"),
        ("build/K.kt", b"fun k() {}"),
    ]);
    let mut service = world.service();
    let a = jfl_domain::RelPath::new("A.java").unwrap();

    // when
    let answer = ask(&mut service, "/w/m/A.java");

    // then: both tracked files count; the build script counts for build logic only
    assert_eq!(answer.other_sources.seen_from(&a), 2);
    assert_eq!(answer.other_sources.total(), 3);
}

#[test]
fn should_ignore_another_daemon_s_cookie_when_it_is_hinted() {
    // given
    let world = World::new(&[("A.java", b"class A {}")]);
    let mut service = world.service();
    let first = ask(&mut service, "/w/m/A.java");
    service.finish(first.id).unwrap();
    world.tree.write("m/.jfl-cookie-other9-2", b"").unwrap();

    // when
    let answer = ask(&mut service, "/w/m/A.java");

    // then
    assert_eq!(answer.generation.generation, first.generation.generation);
    assert!(answer.completeness.is_complete(), "{:?}", reasons(&answer));
}

#[test]
fn should_sweep_stale_cookies_and_keep_fresh_ones_when_a_view_opens() {
    // given
    let world = World::new(&[("A.java", b"class A {}")]);
    world.tree.write("m/.jfl-cookie-gone1-4", b"").unwrap();
    world.tree.write("m/.jfl-cookie-live2-5", b"").unwrap();
    world
        .tree
        .set_mtime("m/.jfl-cookie-live2-5", 1_800_000_000)
        .unwrap();
    world.tree.write("m/.jfl-cookie-notacookie", b"").unwrap();
    let mut service = world.service();

    // when
    let answer = ask(&mut service, "/w/m/A.java");

    // then
    assert!(!world.tree.exists("m/.jfl-cookie-gone1-4"));
    assert!(world.tree.exists("m/.jfl-cookie-live2-5"));
    assert!(world.tree.exists("m/.jfl-cookie-notacookie"));
    assert!(answer.completeness.is_complete(), "{:?}", reasons(&answer));
}

#[test]
fn should_take_the_next_name_and_keep_the_file_when_a_cookie_name_is_taken() {
    // given
    let world = World::new(&[("A.java", b"class A {}")]);
    world.tree.write("m/.jfl-cookie-t1-1", b"theirs").unwrap();
    world
        .tree
        .set_mtime("m/.jfl-cookie-t1-1", 1_800_000_000)
        .unwrap();
    let mut service = world.service();

    // when
    let answer = ask(&mut service, "/w/m/A.java");

    // then
    assert!(world.tree.exists("m/.jfl-cookie-t1-1"));
    assert_eq!(answer.cookie_round_trips.len(), 1);
    assert!(answer.completeness.is_complete(), "{:?}", reasons(&answer));
}

#[test]
fn should_answer_partly_indexed_when_a_directory_is_another_filesystem() {
    // given
    let world = World::new(&[("A.java", b"class A {}")]);
    world.tree.mount("m/mnt").unwrap();
    world.tree.write("m/mnt/R.java", b"class R {}").unwrap();
    let mut service = world.service();

    // when
    let answer = ask(&mut service, "/w/m/A.java");

    // then
    assert_eq!(paths(&answer), ["A.java"]);
    assert!(
        reasons(&answer).contains(&IncompleteReason::ModulePartlyIndexed {
            module: ModuleName::worktree(),
            indexed: 1,
            files: 2,
        })
    );
}
