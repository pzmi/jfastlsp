//! The `source_tree` contract (ver §5, plan §6.4): listing honours the exclusions of plan §4.2
//! and never follows a link (plan §11), a gap is reported and never dropped, stat reflects writes,
//! reads are bounded, and a watch hints changes and reports overflow. Checked the same way against
//! [`crate::fakes::source_tree::FakeSourceTree`] (`contracts::source_tree::tests` here) and against
//! `jfl-adapter-fs` in `crates/jfl-adapter-fs/tests/contract.rs`.
//!
//! Each case asks the [`Factory`] for a fresh, empty tree ([`Fixture`]), builds what it needs
//! through the fixture's own vocabulary (paths are `/`-separated, under the root), and drives the
//! port. A watch is read with `try_next` under [`crate::poll::wait_until`], never with a sleep.
//!
//! The suite's own tests run it against deliberately broken implementations and assert each fails
//! the case it should (`source_tree::tests`).

use std::time::Duration;

use jfl_application::source_tree::{
    Exclusion, FsKind, Hint, Hints, ListMode, Listing, RescanReason, Skipped, SourceTree,
    SourceTreeError, UnlistedReason, cookie_name,
};
use jfl_domain::{FileUri, RelPath};

use crate::contracts::{NonUtf8Name, non_utf8_names};
use crate::exec::block_on;
use crate::poll::wait_until;

/// How long a case waits for a hint from a real watcher: generous, since a loaded machine delays
/// inotify delivery; a passing case returns as soon as the hint arrives.
pub const HINT_DEADLINE: Duration = Duration::from_secs(20);

/// A fresh tree under test, with the means to change it.
pub trait Fixture {
    /// The implementation under test.
    type Tree: SourceTree;

    /// The implementation, answering for [`Fixture::root`].
    fn tree(&self) -> &Self::Tree;

    /// The root: an empty directory when the fixture is made.
    fn root(&self) -> &FileUri;

    /// Writes a file, creating its parents.
    fn write(&self, path: &str, bytes: &[u8]) -> Result<(), String>;

    /// Creates a directory and its parents.
    fn mkdir(&self, path: &str) -> Result<(), String>;

    /// Removes a file or a directory tree.
    fn remove(&self, path: &str) -> Result<(), String>;

    /// Renames a file or a directory, as `mv` within one filesystem does.
    fn rename(&self, from: &str, to: &str) -> Result<(), String>;

    /// A symbolic link at `path` to a directory outside the root that holds `Outside.java`.
    fn link_outside(&self, path: &str) -> Result<(), String>;

    /// A symbolic link at `path` to `target`, a path under the root.
    fn link_inside(&self, path: &str, target: &str) -> Result<(), String>;

    /// A `.java` file in `dir` whose name is not UTF-8, or the platform's refusal of the name
    /// ([`NonUtf8Name`]).
    fn non_utf8_java(&self, dir: &str) -> Result<NonUtf8Name, String>;

    /// Makes the directory `dir` unreadable (no effect for a superuser on a real filesystem).
    fn unreadable(&self, dir: &str) -> Result<(), String>;
}

/// Makes fresh fixtures.
pub trait Factory {
    /// The fixture it makes.
    type Fixture: Fixture;

    /// A fresh, empty tree whose watches queue at most `hint_capacity` paths.
    fn fixture(&self, hint_capacity: usize) -> Result<Self::Fixture, String>;
}

/// A case the implementation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The case's name.
    pub case: &'static str,
    /// What was wrong.
    pub detail: String,
}

/// The hint queue capacity of every case but the overflow one.
const ROOMY: usize = 1024;

type Case<F> = fn(&F, Duration) -> Result<(), String>;

/// Runs every case with [`HINT_DEADLINE`]; an empty list is a pass.
pub fn check<F: Factory>(factory: &F) -> Vec<Violation> {
    check_with(factory, HINT_DEADLINE)
}

/// Runs every case, waiting at most `hint_deadline` for each awaited hint.
pub fn check_with<F: Factory>(factory: &F, hint_deadline: Duration) -> Vec<Violation> {
    let cases: [(&'static str, Case<F>); 22] = [
        ("lists_java_files_once", lists_java_files_once),
        ("lists_with_stat", lists_with_stat),
        ("skips_nested_worktree", skips_nested_worktree),
        ("skips_nested_repository", skips_nested_repository),
        ("skips_agent_worktrees", skips_agent_worktrees),
        ("skips_build_output", skips_build_output),
        ("honours_gitignore", honours_gitignore),
        ("never_follows_a_link", never_follows_a_link),
        ("reports_a_non_utf8_name", reports_a_non_utf8_name),
        (
            "reports_an_unreadable_directory",
            reports_an_unreadable_directory,
        ),
        ("refuses_a_bad_root", refuses_a_bad_root),
        ("stat_reflects_writes", stat_reflects_writes),
        ("read_bounded", read_bounded),
        (
            "read_refuses_what_is_not_a_file",
            read_refuses_what_is_not_a_file,
        ),
        ("watch_hints_a_write", watch_hints_a_write),
        ("watch_reports_queue_full", watch_reports_queue_full),
        (
            "watch_leaves_out_agent_worktrees",
            watch_leaves_out_agent_worktrees,
        ),
        (
            "watch_hints_a_dotted_directory_move",
            watch_hints_a_dotted_directory_move,
        ),
        (
            "watch_hints_a_new_dotted_directory",
            watch_hints_a_new_dotted_directory,
        ),
        (
            "watch_watches_a_new_directory_before_its_hint",
            watch_watches_a_new_directory_before_its_hint,
        ),
        ("reports_a_local_filesystem", reports_a_local_filesystem),
        (
            "cookie_follows_earlier_changes",
            cookie_follows_earlier_changes,
        ),
    ];
    cases
        .into_iter()
        .filter_map(|(case, run)| {
            run(factory, hint_deadline)
                .err()
                .map(|detail| Violation { case, detail })
        })
        .collect()
}

fn rel(path: &str) -> Result<RelPath, String> {
    RelPath::new(path).map_err(|e| format!("{path:?}: {e}"))
}

fn list(fixture: &impl Fixture, mode: ListMode) -> Result<Listing, String> {
    block_on(fixture.tree().list(fixture.root(), mode)).map_err(|e| format!("list failed: {e}"))
}

fn paths(listing: &Listing) -> Vec<&str> {
    listing.files.iter().map(|f| f.path.as_str()).collect()
}

fn skipped(listing: &Listing) -> Vec<(&str, Exclusion)> {
    listing
        .skipped
        .iter()
        .map(|Skipped { path, why }| (path.as_str(), *why))
        .collect()
}

fn expect_files(listing: &Listing, want: &[&str]) -> Result<(), String> {
    let got = paths(listing);
    if got != want {
        return Err(format!("listed {got:?}, want {want:?}"));
    }
    if !listing.is_complete() {
        return Err(format!("unexpected gaps: {:?}", listing.unlisted));
    }
    Ok(())
}

fn expect_skipped(listing: &Listing, want: &[(&str, Exclusion)]) -> Result<(), String> {
    let got = skipped(listing);
    if got != want {
        return Err(format!("skipped {got:?}, want {want:?}"));
    }
    Ok(())
}

fn lists_java_files_once<F: Factory>(factory: &F, _: Duration) -> Result<(), String> {
    let fx = factory.fixture(ROOMY)?;
    fx.write("a/A.java", b"class A {}")?;
    fx.write("a/b/B.java", b"class B {}")?;
    fx.write("C.java", b"class C {}")?;
    fx.write("a/notes.txt", b"not java")?;
    fx.write("a/D.javax", b"not java")?;
    fx.write("a/.java", b"no name")?;
    let listing = list(&fx, ListMode::Paths)?;
    expect_files(&listing, &["C.java", "a/A.java", "a/b/B.java"])?;
    if listing.files.iter().any(|f| f.stat.is_some()) {
        return Err("a Paths listing carried a stat".to_owned());
    }
    expect_skipped(&listing, &[])
}

fn lists_with_stat<F: Factory>(factory: &F, _: Duration) -> Result<(), String> {
    let fx = factory.fixture(ROOMY)?;
    fx.write("A.java", b"12345")?;
    fx.write("p/B.java", b"123")?;
    let listing = list(&fx, ListMode::PathsAndStat)?;
    expect_files(&listing, &["A.java", "p/B.java"])?;
    let sizes: Vec<Option<u64>> = listing
        .files
        .iter()
        .map(|f| f.stat.map(|s| s.size))
        .collect();
    if sizes != [Some(5), Some(3)] {
        return Err(format!("sizes {sizes:?}, want [Some(5), Some(3)]"));
    }
    let a = block_on(fx.tree().stat(fx.root(), &rel("A.java")?)).map_err(|e| e.to_string())?;
    if listing.files.first().and_then(|f| f.stat) != Some(a) {
        return Err(format!("listed stat differs from stat(): {a:?}"));
    }
    Ok(())
}

fn skips_nested_worktree<F: Factory>(factory: &F, _: Duration) -> Result<(), String> {
    let fx = factory.fixture(ROOMY)?;
    fx.write("main/M.java", b"class M {}")?;
    fx.write("linked/.git", b"gitdir: /elsewhere/.git/worktrees/linked\n")?;
    fx.write("linked/src/W.java", b"class W {}")?;
    let listing = list(&fx, ListMode::Paths)?;
    expect_files(&listing, &["main/M.java"])?;
    expect_skipped(&listing, &[("linked", Exclusion::NestedWorktree)])
}

fn skips_nested_repository<F: Factory>(factory: &F, _: Duration) -> Result<(), String> {
    let fx = factory.fixture(ROOMY)?;
    fx.write("M.java", b"class M {}")?;
    fx.mkdir("vendor/lib/.git")?;
    fx.write("vendor/lib/R.java", b"class R {}")?;
    fx.write("vendor/V.java", b"class V {}")?;
    let listing = list(&fx, ListMode::Paths)?;
    expect_files(&listing, &["M.java", "vendor/V.java"])?;
    expect_skipped(&listing, &[("vendor/lib", Exclusion::NestedRepository)])
}

fn skips_agent_worktrees<F: Factory>(factory: &F, _: Duration) -> Result<(), String> {
    let fx = factory.fixture(ROOMY)?;
    fx.write(".claude/worktrees/agent-a/src/A.java", b"class A {}")?;
    fx.write(".claude/notes/N.java", b"class N {}")?;
    fx.write("docs/worktrees/D.java", b"class D {}")?;
    let listing = list(&fx, ListMode::Paths)?;
    expect_files(&listing, &[".claude/notes/N.java", "docs/worktrees/D.java"])?;
    expect_skipped(
        &listing,
        &[(".claude/worktrees", Exclusion::AgentWorktrees)],
    )
}

fn skips_build_output<F: Factory>(factory: &F, _: Duration) -> Result<(), String> {
    let fx = factory.fixture(ROOMY)?;
    fx.write("build.gradle", b"")?;
    fx.write("build/generated/G.java", b"class G {}")?;
    fx.write(".gradle/cache/C.java", b"class C {}")?;
    fx.write("lib/pom.xml", b"<project/>")?;
    fx.write("lib/target/T.java", b"class T {}")?;
    fx.write("src/main/java/org/x/build/B.java", b"class B {}")?;
    fx.write("tools/out/O.java", b"class O {}")?;
    let listing = list(&fx, ListMode::Paths)?;
    expect_files(
        &listing,
        &["src/main/java/org/x/build/B.java", "tools/out/O.java"],
    )?;
    expect_skipped(
        &listing,
        &[
            (".gradle", Exclusion::BuildOutput),
            ("build", Exclusion::BuildOutput),
            ("lib/target", Exclusion::BuildOutput),
        ],
    )
}

fn honours_gitignore<F: Factory>(factory: &F, _: Duration) -> Result<(), String> {
    let fx = factory.fixture(ROOMY)?;
    fx.write(".gitignore", b"generated/\nSkip.java\n")?;
    fx.write("generated/G.java", b"class G {}")?;
    fx.write("a/generated/H.java", b"class H {}")?;
    fx.write("a/Skip.java", b"class Skip {}")?;
    fx.write("a/Keep.java", b"class Keep {}")?;
    fx.write("b/.gitignore", b"Local.java\n")?;
    fx.write("b/Local.java", b"class Local {}")?;
    fx.write("c/Local.java", b"class Local {}")?;
    let listing = list(&fx, ListMode::Paths)?;
    expect_files(&listing, &["a/Keep.java", "c/Local.java"])
}

fn never_follows_a_link<F: Factory>(factory: &F, _: Duration) -> Result<(), String> {
    let fx = factory.fixture(ROOMY)?;
    fx.write("a/A.java", b"class A {}")?;
    fx.link_outside("escape")?;
    fx.link_inside("alias", "a")?;
    fx.link_inside("Alias.java", "a/A.java")?;
    fx.write("readme.txt", b"text")?;
    fx.link_inside("readme-link", "readme.txt")?;
    let listing = list(&fx, ListMode::Paths)?;
    expect_files(&listing, &["a/A.java"])?;
    expect_skipped(
        &listing,
        &[
            ("Alias.java", Exclusion::Symlink),
            ("alias", Exclusion::Symlink),
            ("escape", Exclusion::Symlink),
        ],
    )
}

fn reports_a_non_utf8_name<F: Factory>(factory: &F, _: Duration) -> Result<(), String> {
    let fx = factory.fixture(ROOMY)?;
    fx.write("a/A.java", b"class A {}")?;
    let names = non_utf8_names(fx.non_utf8_java("a")?)?;
    let listing = list(&fx, ListMode::Paths)?;
    let gaps: Vec<(Option<&str>, UnlistedReason)> = listing
        .unlisted
        .iter()
        .map(|u| (u.path.as_ref().map(RelPath::as_str), u.why))
        .collect();
    let want: &[(Option<&str>, UnlistedReason)] = if names == 0 {
        &[]
    } else {
        &[(Some("a"), UnlistedReason::NonUtf8Name)]
    };
    if paths(&listing) != ["a/A.java"] || gaps != want || listing.unlisted_count != names {
        return Err(format!(
            "listed {:?} with gaps {gaps:?}, want a/A.java and the gaps {want:?}",
            paths(&listing)
        ));
    }
    Ok(())
}

/// A superuser reads a `chmod 000` directory, so the file may be listed; what must never happen
/// is that it is neither listed nor reported.
fn reports_an_unreadable_directory<F: Factory>(factory: &F, _: Duration) -> Result<(), String> {
    let fx = factory.fixture(ROOMY)?;
    fx.write("open/O.java", b"class O {}")?;
    fx.write("locked/L.java", b"class L {}")?;
    fx.unreadable("locked")?;
    let listing = list(&fx, ListMode::Paths)?;
    let listed = paths(&listing).contains(&"locked/L.java");
    let reported = listing.unlisted.iter().any(|u| {
        u.why == UnlistedReason::Unreadable
            && u.path.as_ref().map(RelPath::as_str) == Some("locked")
    });
    if !paths(&listing).contains(&"open/O.java") || listed == reported {
        return Err(format!(
            "listed {:?}, gaps {:?}: locked/L.java must be listed or its directory reported, \
             not both, not neither",
            paths(&listing),
            listing.unlisted
        ));
    }
    if reported == listing.is_complete() {
        return Err("is_complete() disagrees with the gaps".to_owned());
    }
    Ok(())
}

fn refuses_a_bad_root<F: Factory>(factory: &F, _: Duration) -> Result<(), String> {
    let fx = factory.fixture(ROOMY)?;
    fx.write("file.txt", b"text")?;
    let missing = fx
        .root()
        .join(&rel("missing")?)
        .map_err(|e| e.to_string())?;
    let file = fx
        .root()
        .join(&rel("file.txt")?)
        .map_err(|e| e.to_string())?;
    let got = [
        block_on(fx.tree().list(&missing, ListMode::Paths)).err(),
        block_on(fx.tree().list(&file, ListMode::Paths)).err(),
        block_on(fx.tree().watch(&missing)).err(),
        block_on(fx.tree().stat(&missing, &rel("A.java")?)).err(),
    ];
    let want = [
        Some(SourceTreeError::RootNotFound),
        Some(SourceTreeError::RootNotADirectory),
        Some(SourceTreeError::RootNotFound),
        Some(SourceTreeError::RootNotFound),
    ];
    if got != want {
        return Err(format!("refusals {got:?}, want {want:?}"));
    }
    Ok(())
}

fn stat_reflects_writes<F: Factory>(factory: &F, _: Duration) -> Result<(), String> {
    let fx = factory.fixture(ROOMY)?;
    let path = rel("p/A.java")?;
    let stat = |fx: &F::Fixture| block_on(fx.tree().stat(fx.root(), &path));
    fx.write("p/A.java", b"x")?;
    let first = stat(&fx).map_err(|e| format!("first stat: {e}"))?;
    fx.write("p/A.java", b"xyz")?;
    let second = stat(&fx).map_err(|e| format!("second stat: {e}"))?;
    fx.remove("p/A.java")?;
    let gone = stat(&fx);
    let dir = block_on(fx.tree().stat(fx.root(), &rel("p")?));
    if first.size != 1 || second.size != 3 {
        return Err(format!(
            "sizes {} then {}, want 1 then 3",
            first.size, second.size
        ));
    }
    if second.mtime < first.mtime {
        return Err(format!("mtime went back: {first:?} then {second:?}"));
    }
    if gone != Err(SourceTreeError::NotFound) || dir != Err(SourceTreeError::NotAFile) {
        return Err(format!("after removal {gone:?}, of a directory {dir:?}"));
    }
    Ok(())
}

fn read_bounded<F: Factory>(factory: &F, _: Duration) -> Result<(), String> {
    let fx = factory.fixture(ROOMY)?;
    let body: Vec<u8> = (0..100_u8).collect();
    fx.write("A.java", &body)?;
    let path = rel("A.java")?;
    let read = |limit| {
        block_on(fx.tree().read(fx.root(), &path, limit))
            .map(|r| (r.bytes, r.truncated))
            .map_err(|e| format!("read with limit {limit}: {e}"))
    };
    let cut = read(10)?;
    let exact = read(100)?;
    let roomy = read(1000)?;
    let none = read(0)?;
    let want_cut = (body.get(..10).unwrap_or_default().to_vec(), true);
    if cut != want_cut || exact != (body.clone(), false) || roomy != (body, false) {
        return Err(format!(
            "limit 10 gave {cut:?}, 100 {exact:?}, 1000 {roomy:?}"
        ));
    }
    if none != (Vec::new(), true) {
        return Err(format!("limit 0 gave {none:?}, want no bytes, truncated"));
    }
    Ok(())
}

fn read_refuses_what_is_not_a_file<F: Factory>(factory: &F, _: Duration) -> Result<(), String> {
    let fx = factory.fixture(ROOMY)?;
    fx.mkdir("dir")?;
    let got = [
        block_on(fx.tree().read(fx.root(), &rel("dir")?, 10)).err(),
        block_on(fx.tree().read(fx.root(), &rel("missing.java")?, 10)).err(),
    ];
    let want = [
        Some(SourceTreeError::NotAFile),
        Some(SourceTreeError::NotFound),
    ];
    if got != want {
        return Err(format!("refusals {got:?}, want {want:?}"));
    }
    Ok(())
}

/// Takes hints until `stop` says so or the deadline passes; returns every hint taken.
fn hints_until(
    watch: &mut impl Hints,
    deadline: Duration,
    reason: &str,
    mut stop: impl FnMut(&Hint) -> bool,
) -> (Vec<Hint>, bool) {
    let mut seen = Vec::new();
    let waited = wait_until(deadline, reason, || {
        while let Some(hint) = watch.try_next() {
            let done = stop(&hint);
            seen.push(hint);
            if done {
                return true;
            }
        }
        false
    });
    (seen, waited.is_ok())
}

fn watch_hints_a_write<F: Factory>(factory: &F, deadline: Duration) -> Result<(), String> {
    let fx = factory.fixture(ROOMY)?;
    fx.write("w/Old.java", b"class Old {}")?;
    let mut watch = block_on(fx.tree().watch(fx.root())).map_err(|e| format!("watch: {e}"))?;
    fx.write("w/A.java", b"class A {}")?;
    let written = Hint::Changed(rel("w/A.java")?);
    let (seen, ok) = hints_until(&mut watch, deadline, "a hint for w/A.java", |h| {
        *h == written
    });
    if !ok {
        return Err(format!("no hint for a write to w/A.java; saw {seen:?}"));
    }
    fx.remove("w/Old.java")?;
    let removed = Hint::Changed(rel("w/Old.java")?);
    let (seen, ok) = hints_until(&mut watch, deadline, "a hint for w/Old.java", |h| {
        *h == removed
    });
    if !ok {
        return Err(format!("no hint for removing w/Old.java; saw {seen:?}"));
    }
    Ok(())
}

/// The hint queue's own overflow: past its capacity, one `QueueFull` rescan replaces the paths. The
/// backend's overflow (inotify `IN_Q_OVERFLOW`) cannot be forced without root `sysctl`s; the
/// adapter's event mapping pins it (`jfl_adapter_fs::watch::tests`).
fn watch_reports_queue_full<F: Factory>(factory: &F, deadline: Duration) -> Result<(), String> {
    let capacity = 4;
    let fx = factory.fixture(capacity)?;
    fx.mkdir("d")?;
    let mut watch = block_on(fx.tree().watch(fx.root())).map_err(|e| format!("watch: {e}"))?;
    for n in 0..capacity.saturating_mul(4) {
        fx.write(&format!("d/F{n}.java"), b"class F {}")?;
    }
    let (seen, ok) = hints_until(
        &mut watch,
        deadline,
        "a rescan",
        |h| matches!(h, Hint::Rescan(r) if r.reasons.contains(RescanReason::QueueFull)),
    );
    if !ok {
        return Err(format!(
            "{} paths changed past a queue of {capacity} with no QueueFull rescan; saw {seen:?}",
            capacity.saturating_mul(4)
        ));
    }
    let changed = seen
        .iter()
        .filter(|h| matches!(h, Hint::Changed(_)))
        .count();
    if changed > capacity {
        return Err(format!("{changed} path hints from a queue of {capacity}"));
    }
    Ok(())
}

fn watch_leaves_out_agent_worktrees<F: Factory>(
    factory: &F,
    deadline: Duration,
) -> Result<(), String> {
    let fx = factory.fixture(ROOMY)?;
    fx.mkdir(".claude/worktrees/agent/src")?;
    fx.mkdir("w")?;
    let mut watch = block_on(fx.tree().watch(fx.root())).map_err(|e| format!("watch: {e}"))?;
    fx.write(".claude/worktrees/agent/src/A.java", b"class A {}")?;
    fx.write("w/B.java", b"class B {}")?;
    // A rescan covers w/B.java: a recursive watch (FSEvents) may fold the path into one, for an
    // event on a path with no name under the root, such as the root itself.
    let (seen, ok) = hints_until(
        &mut watch,
        deadline,
        "a hint for w/B.java or a rescan",
        |h| names_or_rescans(h, "w/B.java"),
    );
    if !ok {
        return Err(format!("no hint for w/B.java and no rescan; saw {seen:?}"));
    }
    let leaked: Vec<&Hint> = seen
        .iter()
        .filter(|h| matches!(h, Hint::Changed(p) if p.as_str().starts_with(".claude/worktrees")))
        .collect();
    if !leaked.is_empty() {
        return Err(format!("hinted inside .claude/worktrees: {leaked:?}"));
    }
    Ok(())
}

/// Whether `hint` names `path` or asks for a rescan.
fn names_or_rescans(hint: &Hint, path: &str) -> bool {
    match hint {
        Hint::Changed(p) => p.as_str() == path,
        Hint::Rescan(_) => true,
    }
}

/// A directory renamed with its files in it: inotify reports the rename on the directory only,
/// never on the files, so a hint must name the directory (either name) or ask for a rescan. A
/// dotted name (`java.base`, `org.eclipse.core`) is a directory like any other.
fn watch_hints_a_dotted_directory_move<F: Factory>(
    factory: &F,
    deadline: Duration,
) -> Result<(), String> {
    let fx = factory.fixture(ROOMY)?;
    fx.write("mod.a/src/A.java", b"class A {}")?;
    let mut watch = block_on(fx.tree().watch(fx.root())).map_err(|e| format!("watch: {e}"))?;
    fx.rename("mod.a", "mod.b")?;
    let (seen, ok) = hints_until(&mut watch, deadline, "a hint for mod.a or mod.b", |h| {
        names_or_rescans(h, "mod.a") || names_or_rescans(h, "mod.b")
    });
    if !ok {
        return Err(format!("no hint for renaming mod.a to mod.b; saw {seen:?}"));
    }
    Ok(())
}

/// A new directory with a dotted name, written into at once: its own hint is what makes the
/// consumer scan it, since the write may land before any watch on it exists.
fn watch_hints_a_new_dotted_directory<F: Factory>(
    factory: &F,
    deadline: Duration,
) -> Result<(), String> {
    let fx = factory.fixture(ROOMY)?;
    let mut watch = block_on(fx.tree().watch(fx.root())).map_err(|e| format!("watch: {e}"))?;
    fx.mkdir("x.y")?;
    fx.write("x.y/B.java", b"class B {}")?;
    let (seen, ok) = hints_until(&mut watch, deadline, "a hint for x.y", |h| {
        names_or_rescans(h, "x.y")
    });
    if !ok {
        return Err(format!("no hint for the new directory x.y; saw {seen:?}"));
    }
    Ok(())
}

/// Watch, then hint: once the hint for a new directory is taken, the directory is watched, so a
/// file written into it afterwards is hinted (the consumer scans the directory on its hint and
/// relies on the watch for what follows).
fn watch_watches_a_new_directory_before_its_hint<F: Factory>(
    factory: &F,
    deadline: Duration,
) -> Result<(), String> {
    let fx = factory.fixture(ROOMY)?;
    let mut watch = block_on(fx.tree().watch(fx.root())).map_err(|e| format!("watch: {e}"))?;
    for round in 0..20 {
        let dir = format!("n{round}");
        fx.mkdir(&format!("{dir}/deep"))?;
        let (seen, ok) = hints_until(&mut watch, deadline, "a hint for the new directory", |h| {
            names_or_rescans(h, &dir)
        });
        if !ok {
            return Err(format!("no hint for the new directory {dir}; saw {seen:?}"));
        }
        let file = format!("{dir}/deep/C.java");
        fx.write(&file, b"class C {}")?;
        let (seen, ok) = hints_until(&mut watch, deadline, "a hint for the file", |h| {
            names_or_rescans(h, &file)
        });
        if !ok {
            return Err(format!(
                "{file}, written after {dir}'s hint was taken, was not hinted; saw {seen:?}"
            ));
        }
    }
    Ok(())
}

/// The fixture's root is on a local filesystem (the adapter's under `CARGO_TARGET_TMPDIR`), which
/// `filesystem` names as such, so a view there is watched; a root that does not exist is refused.
fn reports_a_local_filesystem<F: Factory>(factory: &F, _: Duration) -> Result<(), String> {
    let fx = factory.fixture(ROOMY)?;
    let kind = block_on(fx.tree().filesystem(fx.root()));
    if kind != Ok(FsKind::Local) {
        return Err(format!(
            "the fixture root's filesystem: {kind:?}, not Local"
        ));
    }
    let missing = fx
        .root()
        .join(&rel("missing")?)
        .map_err(|e| e.to_string())?;
    if block_on(fx.tree().filesystem(&missing)).is_ok() {
        return Err("a root that does not exist was given a filesystem".to_owned());
    }
    Ok(())
}

/// A cookie (ADR 0037): written into the root, its hint comes through the watch after the hint of
/// every change made before it, and it leaves no file behind.
fn cookie_follows_earlier_changes<F: Factory>(
    factory: &F,
    deadline: Duration,
) -> Result<(), String> {
    let fx = factory.fixture(ROOMY)?;
    let mut watch = block_on(fx.tree().watch(fx.root())).map_err(|e| format!("watch: {e}"))?;
    fx.write("w/A.java", b"class A {}")?;
    let name = cookie_name("contract", 7, 0).ok_or("no cookie name")?;
    block_on(fx.tree().write_cookie(fx.root(), &name)).map_err(|e| format!("cookie: {e}"))?;
    let cookie = Hint::Changed(name.clone());
    let (seen, ok) = hints_until(&mut watch, deadline, "the cookie's hint", |h| *h == cookie);
    if !ok {
        return Err(format!("no hint for the cookie; saw {seen:?}"));
    }
    let before = seen.iter().take(seen.len().saturating_sub(1));
    if !before
        .clone()
        .any(|h| names_or_rescans(h, "w/A.java") || names_or_rescans(h, "w"))
    {
        return Err(format!(
            "the cookie's hint came before w/A.java's; saw {seen:?}"
        ));
    }
    match block_on(fx.tree().stat(fx.root(), &name)) {
        Err(SourceTreeError::NotFound) => Ok(()),
        other => Err(format!("the cookie file is left behind: {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use jfl_application::source_tree::{FileStat, ReadBytes};

    use super::*;
    use crate::fakes::source_tree::{FakeSourceTree, FakeWatch};

    const FAST: Duration = Duration::from_millis(50);

    /// A tree under test: the fake, or a broken wrapper around it.
    trait Wrap: SourceTree + Sized {
        fn wrap(fake: FakeSourceTree) -> Self;
    }

    struct FakeFixture<T> {
        fake: FakeSourceTree,
        tree: T,
    }

    impl<T: Wrap> Fixture for FakeFixture<T> {
        type Tree = T;

        fn tree(&self) -> &T {
            &self.tree
        }
        fn root(&self) -> &FileUri {
            self.fake.root()
        }
        fn write(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
            self.fake.write(path, bytes)
        }
        fn mkdir(&self, path: &str) -> Result<(), String> {
            self.fake.mkdir(path)
        }
        fn remove(&self, path: &str) -> Result<(), String> {
            self.fake.remove(path)
        }
        fn rename(&self, from: &str, to: &str) -> Result<(), String> {
            self.fake.rename(from, to)
        }
        fn link_outside(&self, path: &str) -> Result<(), String> {
            self.fake.link(path, true)
        }
        fn link_inside(&self, path: &str, target: &str) -> Result<(), String> {
            let to_dir = !target.ends_with(".java") && !target.ends_with(".txt");
            self.fake.link(path, to_dir)
        }
        fn non_utf8_java(&self, dir: &str) -> Result<NonUtf8Name, String> {
            self.fake.non_utf8_java(dir).map(|()| NonUtf8Name::Written)
        }
        fn unreadable(&self, dir: &str) -> Result<(), String> {
            self.fake.unreadable(dir)
        }
    }

    struct Fakes<T>(core::marker::PhantomData<T>);

    impl<T: Wrap> Factory for Fakes<T> {
        type Fixture = FakeFixture<T>;

        fn fixture(&self, hint_capacity: usize) -> Result<FakeFixture<T>, String> {
            let root = FileUri::from_path("/fake/root").map_err(|e| e.to_string())?;
            let fake = FakeSourceTree::new(root, hint_capacity);
            Ok(FakeFixture {
                tree: T::wrap(fake.clone()),
                fake,
            })
        }
    }

    fn run<T: Wrap>() -> Vec<&'static str> {
        check_with(&Fakes::<T>(core::marker::PhantomData), FAST)
            .into_iter()
            .map(|v| v.case)
            .collect()
    }

    impl Wrap for FakeSourceTree {
        fn wrap(fake: FakeSourceTree) -> Self {
            fake
        }
    }

    /// Delegates every call but those a broken implementation overrides.
    macro_rules! broken {
        ($name:ident { $($body:tt)* }) => {
            struct $name(FakeSourceTree);
            impl Wrap for $name {
                fn wrap(fake: FakeSourceTree) -> Self {
                    Self(fake)
                }
            }
            impl SourceTree for $name {
                $($body)*
                delegate!(cookie);
            }
        };
    }

    macro_rules! delegate {
        (list) => {
            async fn list(
                &self,
                root: &FileUri,
                mode: ListMode,
            ) -> Result<Listing, SourceTreeError> {
                self.0.list(root, mode).await
            }
        };
        (read) => {
            async fn read(
                &self,
                root: &FileUri,
                path: &RelPath,
                limit: usize,
            ) -> Result<ReadBytes, SourceTreeError> {
                self.0.read(root, path, limit).await
            }
        };
        (stat) => {
            async fn stat(
                &self,
                root: &FileUri,
                path: &RelPath,
            ) -> Result<FileStat, SourceTreeError> {
                self.0.stat(root, path).await
            }
        };
        (watch) => {
            type Watch = FakeWatch;
            async fn watch(&self, root: &FileUri) -> Result<FakeWatch, SourceTreeError> {
                self.0.watch(root).await
            }
        };
        (cookie) => {
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
        };
    }

    // Lists what the rules skip: each skipped directory is listed again as a root of its own.
    broken!(ListsNested {
        delegate!(read);
        delegate!(stat);
        delegate!(watch);
        async fn list(&self, root: &FileUri, mode: ListMode) -> Result<Listing, SourceTreeError> {
            let mut listing = self.0.list(root, mode).await?;
            for skip in core::mem::take(&mut listing.skipped) {
                let Ok(sub) = root.join(&skip.path) else { continue };
                if let Ok(inner) = self.0.list(&sub, mode).await {
                    for mut file in inner.files {
                        if let Ok(path) = skip.path.join(&file.path) {
                            file.path = path;
                            listing.files.push(file);
                        }
                    }
                }
            }
            listing.sort();
            Ok(listing)
        }
    });

    // Drops the gaps: an incomplete listing that says it is complete.
    broken!(HidesGaps {
        delegate!(read);
        delegate!(stat);
        delegate!(watch);
        async fn list(&self, root: &FileUri, mode: ListMode) -> Result<Listing, SourceTreeError> {
            let mut listing = self.0.list(root, mode).await?;
            listing.unlisted.clear();
            listing.unlisted_count = 0;
            Ok(listing)
        }
    });

    // Reads the whole file whatever the limit.
    broken!(UnboundedRead {
        delegate!(list);
        delegate!(stat);
        delegate!(watch);
        async fn read(&self, root: &FileUri, path: &RelPath, _limit: usize)
            -> Result<ReadBytes, SourceTreeError> {
            self.0.read(root, path, usize::MAX).await
        }
    });

    /// Caches the first stat of each path.
    struct StaleStat(FakeSourceTree, Mutex<HashMap<String, FileStat>>);

    impl Wrap for StaleStat {
        fn wrap(fake: FakeSourceTree) -> Self {
            Self(fake, Mutex::default())
        }
    }

    impl SourceTree for StaleStat {
        delegate!(cookie);
        delegate!(list);
        delegate!(read);
        delegate!(watch);
        async fn stat(&self, root: &FileUri, path: &RelPath) -> Result<FileStat, SourceTreeError> {
            if let Some(stat) = self.1.lock().unwrap().get(path.as_str()) {
                return Ok(*stat);
            }
            let stat = self.0.stat(root, path).await?;
            self.1
                .lock()
                .unwrap()
                .insert(path.as_str().to_owned(), stat);
            Ok(stat)
        }
    }

    /// A watch that never hints.
    struct Deaf {
        queue: jfl_application::HintQueue,
        _sink: jfl_application::HintSink,
    }

    impl Hints for Deaf {
        fn try_next(&mut self) -> Option<Hint> {
            self.queue.try_next()
        }
        fn next(&mut self) -> impl Future<Output = Option<Hint>> + Send {
            self.queue.next()
        }
    }

    broken!(DeafWatch {
        delegate!(list);
        delegate!(read);
        delegate!(stat);
        type Watch = Deaf;
        async fn watch(&self, root: &FileUri) -> Result<Deaf, SourceTreeError> {
            let _ = self.0.watch(root).await?;
            let (sink, queue) = jfl_application::hint_queue(8);
            Ok(Deaf {
                queue,
                _sink: sink,
            })
        }
    });

    /// A watch that swallows rescans.
    struct NoRescan(FakeWatch);

    impl Hints for NoRescan {
        fn try_next(&mut self) -> Option<Hint> {
            loop {
                match self.0.try_next()? {
                    Hint::Rescan(_) => {}
                    changed => return Some(changed),
                }
            }
        }
        fn next(&mut self) -> impl Future<Output = Option<Hint>> + Send {
            self.0.next()
        }
    }

    broken!(SilentOverflow {
        delegate!(list);
        delegate!(read);
        delegate!(stat);
        type Watch = NoRescan;
        async fn watch(&self, root: &FileUri) -> Result<NoRescan, SourceTreeError> {
            Ok(NoRescan(self.0.watch(root).await?))
        }
    });

    // Hints every change, `.claude/worktrees/` included.
    struct LeakyWatch(FakeSourceTree);

    impl Wrap for LeakyWatch {
        fn wrap(fake: FakeSourceTree) -> Self {
            Self(fake)
        }
    }

    /// Adds a hint for every agent-worktree write the fake left out, ahead of the real ones.
    struct Leaks(FakeWatch, bool);

    impl Hints for Leaks {
        fn try_next(&mut self) -> Option<Hint> {
            if !self.1 {
                self.1 = true;
                return RelPath::new(".claude/worktrees/agent/src/A.java")
                    .ok()
                    .map(Hint::Changed);
            }
            self.0.try_next()
        }
        fn next(&mut self) -> impl Future<Output = Option<Hint>> + Send {
            self.0.next()
        }
    }

    impl SourceTree for LeakyWatch {
        type Watch = Leaks;
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
        async fn watch(&self, root: &FileUri) -> Result<Leaks, SourceTreeError> {
            Ok(Leaks(self.0.watch(root).await?, false))
        }
        delegate!(cookie);
    }

    /// Takes the port's defaults: no filesystem type, no cookie.
    struct Defaults(FakeSourceTree);

    impl Wrap for Defaults {
        fn wrap(fake: FakeSourceTree) -> Self {
            Self(fake)
        }
    }

    impl SourceTree for Defaults {
        delegate!(list);
        delegate!(read);
        delegate!(stat);
        delegate!(watch);
    }

    /// Filters hints through a rule; `LateWatch` and `DottedDirsDropped` below.
    struct Filtered<R>(FakeWatch, R);

    trait Filter: Send {
        fn keep(&mut self, hint: &Hint) -> bool;
    }

    impl<R: Filter> Hints for Filtered<R> {
        fn try_next(&mut self) -> Option<Hint> {
            loop {
                let hint = self.0.try_next()?;
                if self.1.keep(&hint) {
                    return Some(hint);
                }
            }
        }
        fn next(&mut self) -> impl Future<Output = Option<Hint>> + Send {
            self.0.next()
        }
    }

    /// The extension rule applied to every path, as IDX-06's first version did: a dotted
    /// directory is never hinted.
    #[derive(Default)]
    struct DottedDirsDropped;

    impl Filter for DottedDirsDropped {
        fn keep(&mut self, hint: &Hint) -> bool {
            match hint {
                Hint::Changed(path) => {
                    let name = path.file_name();
                    name.ends_with(".java") || !name.get(1..).is_some_and(|r| r.contains('.'))
                }
                Hint::Rescan(_) => true,
            }
        }
    }

    /// Hands out a new directory's hint before watching it: the first file written into it is lost.
    #[derive(Default)]
    struct LateWatch(Vec<String>);

    impl Filter for LateWatch {
        fn keep(&mut self, hint: &Hint) -> bool {
            let Hint::Changed(path) = hint else {
                return true;
            };
            let path = path.as_str();
            if let Some(at) = self
                .0
                .iter()
                .position(|dir| path.starts_with(&format!("{dir}/")) && path.ends_with(".java"))
            {
                self.0.swap_remove(at);
                return false;
            }
            if !path.contains('/') && !path.ends_with(".java") {
                self.0.push(path.to_owned());
            }
            true
        }
    }

    macro_rules! filtered {
        ($name:ident, $filter:ty) => {
            struct $name(FakeSourceTree);
            impl Wrap for $name {
                fn wrap(fake: FakeSourceTree) -> Self {
                    Self(fake)
                }
            }
            impl SourceTree for $name {
                delegate!(cookie);
                delegate!(list);
                delegate!(read);
                delegate!(stat);
                type Watch = Filtered<$filter>;
                async fn watch(&self, root: &FileUri) -> Result<Self::Watch, SourceTreeError> {
                    Ok(Filtered(self.0.watch(root).await?, <$filter>::default()))
                }
            }
        };
    }

    filtered!(DropsDottedDirs, DottedDirsDropped);
    filtered!(HintsBeforeWatching, LateWatch);

    #[test]
    fn should_fail_the_dotted_directory_cases_when_dotted_names_are_dropped() {
        // given
        let suite_on_broken = || run::<DropsDottedDirs>();

        // when
        let failed = suite_on_broken();

        // then
        assert_eq!(
            failed,
            [
                "watch_hints_a_dotted_directory_move",
                "watch_hints_a_new_dotted_directory"
            ]
        );
    }

    #[test]
    fn should_fail_the_order_case_when_a_directory_is_hinted_before_it_is_watched() {
        // given
        let suite_on_broken = || run::<HintsBeforeWatching>();

        // when
        let failed = suite_on_broken();

        // then
        assert_eq!(failed, ["watch_watches_a_new_directory_before_its_hint"]);
    }

    #[test]
    fn should_fail_the_filesystem_and_cookie_cases_when_the_defaults_are_kept() {
        // given
        let suite_on_broken = || run::<Defaults>();

        // when
        let failed = suite_on_broken();

        // then
        assert_eq!(
            failed,
            [
                "reports_a_local_filesystem",
                "cookie_follows_earlier_changes"
            ]
        );
    }

    #[test]
    fn should_pass_the_source_tree_contract_when_the_fake_answers() {
        // given
        let factory = Fakes::<FakeSourceTree>(core::marker::PhantomData);

        // when
        let violations = check(&factory);

        // then
        assert_eq!(violations, []);
    }

    #[test]
    fn should_fail_the_exclusion_cases_when_skipped_directories_are_listed() {
        // given
        let suite_on_broken = || run::<ListsNested>();

        // when
        let failed = suite_on_broken();

        // then
        for case in [
            "skips_nested_worktree",
            "skips_nested_repository",
            "skips_agent_worktrees",
            "skips_build_output",
        ] {
            assert!(failed.contains(&case), "{case} not failed: {failed:?}");
        }
    }

    #[test]
    fn should_fail_the_gap_cases_when_gaps_are_hidden() {
        // given
        let suite_on_broken = || run::<HidesGaps>();

        // when
        let failed = suite_on_broken();

        // then
        assert_eq!(
            failed,
            ["reports_a_non_utf8_name", "reports_an_unreadable_directory"]
        );
    }

    #[test]
    fn should_fail_read_bounded_when_the_limit_is_ignored() {
        // given
        let suite_on_broken = || run::<UnboundedRead>();

        // when
        let failed = suite_on_broken();

        // then
        assert_eq!(failed, ["read_bounded"]);
    }

    #[test]
    fn should_fail_stat_reflects_writes_when_a_stat_is_cached() {
        // given
        let suite_on_broken = || run::<StaleStat>();

        // when
        let failed = suite_on_broken();

        // then
        assert!(failed.contains(&"stat_reflects_writes"), "{failed:?}");
    }

    #[test]
    fn should_fail_the_watch_cases_when_the_watch_never_hints() {
        // given
        let suite_on_broken = || run::<DeafWatch>();

        // when
        let failed = suite_on_broken();

        // then
        assert_eq!(
            failed,
            [
                "watch_hints_a_write",
                "watch_reports_queue_full",
                "watch_leaves_out_agent_worktrees",
                "watch_hints_a_dotted_directory_move",
                "watch_hints_a_new_dotted_directory",
                "watch_watches_a_new_directory_before_its_hint",
                "cookie_follows_earlier_changes"
            ]
        );
    }

    #[test]
    fn should_fail_watch_reports_queue_full_when_a_rescan_is_swallowed() {
        // given
        let suite_on_broken = || run::<SilentOverflow>();

        // when
        let failed = suite_on_broken();

        // then
        assert_eq!(failed, ["watch_reports_queue_full"]);
    }

    #[test]
    fn should_fail_watch_leaves_out_agent_worktrees_when_they_are_hinted() {
        // given
        let suite_on_broken = || run::<LeakyWatch>();

        // when
        let failed = suite_on_broken();

        // then
        assert_eq!(failed, ["watch_leaves_out_agent_worktrees"]);
    }
}
