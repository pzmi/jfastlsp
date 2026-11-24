//! The `SourceTree` port (plan §6.4): a worktree's `.java` files, listed with the exclusions of
//! plan §4.2, read with a bound, stat'ed, and watched for hints (plan §4.3).
//!
//! The exclusion rules are pure functions here, so the testkit fake and the filesystem adapter
//! apply the same rules and the `source_tree` contract suite (`jfl_testkit::contracts`) checks
//! that each does. What a listing could not see is reported ([`Listing::unlisted`]), never
//! dropped: a view built from an incomplete listing cannot answer `complete=true` (ADR 0014).
//! Nor is what it cannot index: other JVM languages' sources and Gradle build scripts are
//! classified by name ([`source_kind`], one allowlist) and counted ([`Listing::other`]), so an
//! answer they may contradict says so (IDX-16).

use core::future::Future;

use jfl_domain::{FileUri, RelPath};

/// The extension of the files a listing holds.
pub const JAVA_EXTENSION: &str = ".java";

/// Directory names that hold a build's output when they sit beside a build file (Gradle's
/// `build`, Maven's `target`, IntelliJ's `out`, Eclipse's `bin`). Only beside a build file: a
/// package may be called `build` (spring-framework's `org/springframework/build` is one).
pub const BUILD_OUTPUT_DIRS: [&str; 4] = ["build", "target", "out", "bin"];

/// The files that make a sibling [`BUILD_OUTPUT_DIRS`] directory a build's output.
pub const BUILD_FILES: [&str; 5] = [
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
    "pom.xml",
];

/// Directories that only ever hold tool state, wherever they are: Gradle's project cache.
pub const TOOL_STATE_DIRS: [&str; 1] = [".gradle"];

/// The most entries [`Listing::unlisted`] keeps; [`Listing::unlisted_count`] counts them all.
pub const MAX_REPORTED_UNLISTED: usize = 1024;

/// A JVM language other than Java whose sources can name Java declarations: not indexed in v1
/// (plan §1.4), so a view holding one cannot prove an answer whole (IDX-16, ADR 0014).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OtherLanguage {
    /// Kotlin/JVM.
    Kotlin,
    /// Groovy.
    Groovy,
    /// Scala.
    Scala,
    /// Clojure on the JVM.
    Clojure,
    /// Xtend.
    Xtend,
    /// JavaServer Pages and JSP tag files.
    Jsp,
    /// AspectJ: the one language here whose files can change what a Java name is proved bound
    /// to (inter-type declarations add members, `declare parents` supertypes).
    AspectJ,
    /// BeanShell.
    BeanShell,
    /// Groovy Server Pages (Grails).
    Gsp,
    /// jte templates (`.jte` compiles to Java, `.kte` to Kotlin).
    Jte,
}

/// The allowlist of other JVM languages' file extensions, matched without regard to ASCII case
/// (IDX-16). Each is here because files of that extension name Java declarations directly and
/// are compiled or run on the same classpath as the view's Java:
/// - `.kt`: Kotlin calls Java as its own code; Gradle's Kotlin plugin compiles a source set's
///   Kotlin and Java together, so each sees the other's declarations.
/// - `.kts`: Kotlin scripts other than Gradle's (those are [`BUILD_SCRIPT_SUFFIXES`]): a script's
///   classpath may hold the project's classes.
/// - `.groovy`, `.gvy`, `.gy`, `.gsh`: Groovy sources and scripts (`groovyc` and `groovysh` take
///   all four): Spock specifications under `src/test/groovy`, Groovy beans and the scripts a
///   framework loads at run time (spring-framework's test resources hold some).
/// - `.scala`, `.sc`: Scala sources, and Scala scripts and worksheets (Ammonite, scala-cli, Mill's
///   `build.sc`).
/// - `.clj`, `.cljc`: Clojure on the JVM names Java classes and members by interop; `.cljc`
///   files compile for the JVM too.
/// - `.xtend`: Xtend compiles to Java and calls it.
/// - `.jsp`, `.jspf`, `.jspx`: JSP pages and fragments compile to servlets; their imports and
///   scriptlets are Java. `.tag`, `.tagx`: JSP tag files, compiled the same way.
/// - `.aj`: AspectJ is compiled by `ajc` together with the Java it advises and calls it; its
///   inter-type declarations and `declare parents` also add members and supertypes to Java
///   classes, so even a proved binding can be wrong while one is in the view
///   ([`OtherSources::aspects_seen_from`]; spring-framework's `spring-aspects` holds some).
/// - `.bsh`: BeanShell scripts implement Java interfaces and call Java (spring-framework's
///   scripting tests load some).
/// - `.gsp`: Groovy Server Pages compile to Groovy classes whose imports and scriptlets call Java.
/// - `.jte`, `.kte`: jte templates compile to Java or Kotlin classes; their `@import` and
///   `@param` lines and expressions name Java declarations (junit-framework's build holds some).
///
/// Left out, each on purpose: `.cljs` (ClojureScript compiles to JavaScript); `.py`, `.rb`, `.js`
/// (Jython, JRuby and the JVM's JavaScript engines can call Java, but such files are almost
/// always CPython, Ruby or browser and Node code, and counting them would withdraw `complete`
/// from every repository with a tool script); `.xml`, `.properties`, `META-INF/services` and
/// other text that names a class in a string (a reflective use, which no language's syntax binds);
/// `.class` and `.jar` (compiled libraries: PROJ-03); templates that reach Java only through
/// reflective property paths (Thymeleaf and FreeMarker `.html`/`.ftl`, Velocity `.vm`,
/// Mustache), the same case as a string.
///
/// Pinned by: `source_tree::tests::should_classify_a_file_by_its_extension_when_a_listing_meets_it`.
pub const OTHER_JVM_EXTENSIONS: [(&str, OtherLanguage); 21] = [
    (".kt", OtherLanguage::Kotlin),
    (".kts", OtherLanguage::Kotlin),
    (".groovy", OtherLanguage::Groovy),
    (".gvy", OtherLanguage::Groovy),
    (".gy", OtherLanguage::Groovy),
    (".gsh", OtherLanguage::Groovy),
    (".scala", OtherLanguage::Scala),
    (".sc", OtherLanguage::Scala),
    (".clj", OtherLanguage::Clojure),
    (".cljc", OtherLanguage::Clojure),
    (".xtend", OtherLanguage::Xtend),
    (".jsp", OtherLanguage::Jsp),
    (".jspf", OtherLanguage::Jsp),
    (".jspx", OtherLanguage::Jsp),
    (".tag", OtherLanguage::Jsp),
    (".tagx", OtherLanguage::Jsp),
    (".aj", OtherLanguage::AspectJ),
    (".bsh", OtherLanguage::BeanShell),
    (".gsp", OtherLanguage::Gsp),
    (".jte", OtherLanguage::Jte),
    (".kte", OtherLanguage::Jte),
];

/// Gradle build scripts, Groovy (`.gradle`) and Kotlin (`.gradle.kts`) DSL: `build.gradle`,
/// `settings.gradle.kts`, a script plugin applied with `apply from:`, an init script.
///
/// A build script is compiled against the build's own classpath: Gradle's API, its plugins, its
/// `buildscript` dependencies, and the classes of `buildSrc` and of included builds; as a rule not
/// against the source sets of the build it configures, which that build compiles after its
/// scripts ran. So a script counts only for a target in build logic
/// ([`OtherSources::in_build_logic`]), by directory. **A known limitation until the project
/// model (M3):** build logic can compile sources from elsewhere (`buildSrc` with
/// `srcDir("../src/main/java")`, an included build whose source directories lie outside it,
/// `buildscript { classpath files(...) }` naming a project's output), and an included build
/// without a settings file of its own (which Gradle 8 refuses) is not recognised as one; a
/// script's use of such a declaration is then not counted.
pub const BUILD_SCRIPT_SUFFIXES: [&str; 2] = [".gradle", ".gradle.kts"];

/// The directory Gradle builds, by this fixed name, before the build beside it: its classes are on
/// that build's scripts' classpath and on no source set's.
pub const BUILD_SRC_DIR: &str = "buildSrc";

/// The files that make a directory a Gradle build's root; one below the view's root is an
/// included or nested build, whose classes may be on another build's scripts' classpath.
pub const SETTINGS_FILES: [&str; 2] = ["settings.gradle", "settings.gradle.kts"];

/// The most paths [`Listing::other`] keeps; [`Listing::other_count`] counts them all.
pub const MAX_LISTED_OTHER: usize = 1 << 16;

/// What a file is to a listing, by its name alone (IDX-16): no read, no parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceKind {
    /// A `.java` file: listed and indexed.
    Java,
    /// A source file of another JVM language ([`OTHER_JVM_EXTENSIONS`]): counted.
    Other(OtherLanguage),
    /// A Gradle build script ([`BUILD_SCRIPT_SUFFIXES`]): counted for build logic only.
    BuildScript,
}

/// What a file of this name is, `None` when no listing rule reads it. A name must hold more than
/// its extension (`.kt` alone is no Kotlin file, as `.java` alone is no Java file).
///
/// Pinned by: `source_tree::tests::should_classify_a_file_by_its_extension_when_a_listing_meets_it`.
#[must_use]
pub fn source_kind(name: &str) -> Option<SourceKind> {
    if is_java_file_name(name) {
        return Some(SourceKind::Java);
    }
    let has = |suffix: &str| {
        name.len()
            .checked_sub(suffix.len())
            .filter(|at| *at > 0)
            .and_then(|at| name.get(at..))
            .is_some_and(|end| end.eq_ignore_ascii_case(suffix))
    };
    if BUILD_SCRIPT_SUFFIXES.iter().any(|suffix| has(suffix)) {
        return Some(SourceKind::BuildScript);
    }
    OTHER_JVM_EXTENSIONS
        .iter()
        .find(|(extension, _)| has(extension))
        .map(|(_, language)| SourceKind::Other(*language))
}

/// Whether a listing counts a file of this name as another JVM language's source or a build
/// script ([`source_kind`]).
#[must_use]
pub fn is_other_source_name(name: &str) -> bool {
    matches!(
        source_kind(name),
        Some(SourceKind::Other(_) | SourceKind::BuildScript)
    )
}

/// The other JVM languages' sources and the build scripts of a view (IDX-16), from its last
/// listing: what may use a Java declaration, or declare what a Java name binds to, and is not
/// indexed. Counted per view, which before M3 is the module (plan §2.1).
///
/// Pinned by: `source_tree::tests::should_count_build_logic_only_for_a_path_in_it_when_other_sources_are_summed`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OtherSources {
    /// The listed paths, sorted: a hint naming one of them that still exists changes no count.
    paths: Vec<RelPath>,
    /// Sources outside `buildSrc`, and every file the listing counted without keeping its path:
    /// any of them may see any declaration of the view.
    sources: u32,
    /// Build scripts, and sources under a `buildSrc` directory: they see only build logic.
    build_logic: u32,
    /// AspectJ files outside `buildSrc` (counted in `sources` too).
    aspects: u32,
    /// AspectJ files under `buildSrc` (counted in `build_logic` too).
    build_logic_aspects: u32,
    /// The directories strictly below the root that hold a [`SETTINGS_FILES`] file, sorted.
    build_roots: Vec<RelPath>,
}

impl OtherSources {
    /// From the paths a listing kept (and, in the view service, the tracked ones unioned in),
    /// any of them repeated, and `unkept` more files counted without a path, which count as
    /// sources.
    #[must_use]
    pub fn new(listed: &[RelPath], unkept: u64) -> Self {
        let to_u32 = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);
        let mut paths = listed.to_vec();
        paths.sort_unstable();
        paths.dedup();
        let mut sources = 0_u32;
        let mut build_logic = 0_u32;
        let mut aspects = 0_u32;
        let mut build_logic_aspects = 0_u32;
        let mut build_roots = Vec::new();
        for path in &paths {
            let name = path.file_name();
            let kind = source_kind(name);
            let aspect = u32::from(kind == Some(SourceKind::Other(OtherLanguage::AspectJ)));
            if kind == Some(SourceKind::BuildScript)
                || path.components().any(|c| c == BUILD_SRC_DIR)
            {
                build_logic = build_logic.saturating_add(1);
                build_logic_aspects = build_logic_aspects.saturating_add(aspect);
            } else {
                sources = sources.saturating_add(1);
                aspects = aspects.saturating_add(aspect);
            }
            if SETTINGS_FILES.contains(&name)
                && let Some(dir) = path.parent()
            {
                build_roots.push(dir);
            }
        }
        sources = sources.saturating_add(to_u32(unkept));
        build_roots.sort_unstable();
        build_roots.dedup();
        Self {
            paths,
            sources,
            build_logic,
            aspects,
            build_logic_aspects,
            build_roots,
        }
    }

    /// Every file counted, whatever it can see: what may declare a symbol a workspace symbol
    /// search does not list.
    #[must_use]
    pub const fn total(&self) -> u32 {
        self.sources.saturating_add(self.build_logic)
    }

    /// How many AspectJ files can weave `path`'s declarations (as [`Self::seen_from`]): an
    /// inter-type declaration or `declare parents` can change even a proved binding there.
    #[must_use]
    pub fn aspects_seen_from(&self, path: &RelPath) -> u32 {
        if self.in_build_logic(path) {
            self.aspects.saturating_add(self.build_logic_aspects)
        } else {
            self.aspects
        }
    }

    /// Whether the last listing held `path` (so a change to it, while it exists, changes no
    /// count).
    #[must_use]
    pub fn holds(&self, path: &RelPath) -> bool {
        self.paths.binary_search(path).is_ok()
    }

    /// Whether `path` lies in build logic: under a `buildSrc` directory, or in an included or
    /// nested build (below a directory with a [`SETTINGS_FILES`] file other than the root).
    #[must_use]
    pub fn in_build_logic(&self, path: &RelPath) -> bool {
        path.components().any(|c| c == BUILD_SRC_DIR)
            || self.build_roots.iter().any(|root| {
                path.as_str()
                    .strip_prefix(root.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
            })
    }

    /// How many of these files can see a declaration in `path`, or declare what a name in
    /// `path` binds to: every source, and for a path in build logic the build scripts and
    /// `buildSrc`'s sources too.
    #[must_use]
    pub fn seen_from(&self, path: &RelPath) -> u32 {
        if self.in_build_logic(path) {
            self.sources.saturating_add(self.build_logic)
        } else {
            self.sources
        }
    }
}

/// What a listing returns for each file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListMode {
    /// Paths only: the walk reads directories and never stats a file.
    Paths,
    /// Paths and each file's [`FileStat`]: one more `lstat` per file.
    PathsAndStat,
}

/// A modification time: seconds and nanoseconds since the Unix epoch, as the filesystem keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MTime {
    /// Whole seconds; negative before 1970.
    pub secs: i64,
    /// Nanoseconds within the second, below 1e9.
    pub nanos: u32,
}

/// What a stat tells freshness (plan §4.3): enough to see a write, a replacement or a truncation
/// without reading the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileStat {
    /// Length in bytes.
    pub size: u64,
    /// Last modification.
    pub mtime: MTime,
    /// The inode number: an editor that saves by rename changes it.
    pub inode: u64,
}

/// One listed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedFile {
    /// The path under the root.
    pub path: RelPath,
    /// Its stat, with [`ListMode::PathsAndStat`].
    pub stat: Option<FileStat>,
}

/// Why a directory (or a symbolic link) was left out on purpose. Not a gap: these are the rules
/// of plan §4.2 and §11.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Exclusion {
    /// It holds a `.git` file: a linked worktree, which is its own view (plan §4.2).
    NestedWorktree,
    /// It holds a `.git` directory: a nested repository, its own view.
    NestedRepository,
    /// `.claude/worktrees/`, where agent worktrees live, skipped whatever it holds (plan §4.2).
    AgentWorktrees,
    /// A build's output ([`BUILD_OUTPUT_DIRS`] beside a [`BUILD_FILES`] file) or tool state
    /// ([`TOOL_STATE_DIRS`]).
    BuildOutput,
    /// A symbolic link to a directory or a `.java` file: never followed, so nothing outside the
    /// root is listed (plan §11) and nothing inside it twice.
    Symlink,
}

/// A path left out on purpose.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Skipped {
    /// The directory or link.
    pub path: RelPath,
    /// Which rule.
    pub why: Exclusion,
}

/// Why something under the root could not be listed: a gap in the listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnlistedReason {
    /// A directory could not be read (permissions, or it vanished during the walk).
    Unreadable,
    /// A `.java` file whose name is not UTF-8: it has no [`RelPath`].
    NonUtf8Name,
    /// A path longer than [`jfl_domain::path::MAX_PATH_LEN`].
    PathTooLong,
    /// A file whose stat failed (it vanished between the directory read and the stat).
    StatFailed,
    /// The listing reached its file cap and stopped.
    TooManyFiles,
    /// A directory on another filesystem than the root's (a mount point): not walked, since
    /// the root's filesystem type says nothing about whether a watch sees its changes.
    OtherFilesystem,
}

/// A gap in a listing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Unlisted {
    /// The nearest path that names it, when there is one (a non-UTF-8 name has none).
    pub path: Option<RelPath>,
    /// What went wrong.
    pub why: UnlistedReason,
}

/// A listing of a root's `.java` files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    /// Every listed file, sorted by path, each once.
    pub files: Vec<ListedFile>,
    /// What the exclusion rules left out, sorted.
    pub skipped: Vec<Skipped>,
    /// The gaps, sorted, at most [`MAX_REPORTED_UNLISTED`].
    pub unlisted: Vec<Unlisted>,
    /// How many gaps there were in all.
    pub unlisted_count: u64,
    /// Other JVM languages' sources and Gradle build scripts ([`is_other_source_name`]), under
    /// the same exclusion rules as `.java` files: never listed as files, sorted, at most
    /// [`MAX_LISTED_OTHER`] (IDX-16).
    pub other: Vec<RelPath>,
    /// How many there were in all, a name that is not a [`RelPath`] included.
    pub other_count: u64,
}

impl Listing {
    /// Whether the listing saw everything its rules let it see.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.unlisted_count == 0
    }

    /// Records a gap, keeping at most [`MAX_REPORTED_UNLISTED`] of them.
    pub fn record_unlisted(&mut self, gap: Unlisted) {
        self.unlisted_count = self.unlisted_count.saturating_add(1);
        if self.unlisted.len() < MAX_REPORTED_UNLISTED {
            self.unlisted.push(gap);
        }
    }

    /// Counts a file [`is_other_source_name`] names, keeping its path while fewer than
    /// [`MAX_LISTED_OTHER`] are kept (`None`: a name that is not a [`RelPath`], counted only).
    pub fn record_other(&mut self, path: Option<RelPath>) {
        self.other_count = self.other_count.saturating_add(1);
        if let Some(path) = path
            && self.other.len() < MAX_LISTED_OTHER
        {
            self.other.push(path);
        }
    }

    /// Sorts every part, so equal trees give equal listings.
    pub fn sort(&mut self) {
        self.files.sort_unstable_by(|a, b| a.path.cmp(&b.path));
        self.skipped.sort_unstable();
        self.unlisted.sort_unstable();
        self.other.sort_unstable();
    }
}

/// A bounded read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadBytes {
    /// At most the limit's bytes, from the start of the file.
    pub bytes: Vec<u8>,
    /// The file held more than the limit.
    pub truncated: bool,
}

/// Why a watcher failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WatchErrorKind {
    /// The system's limit on watches was reached (Linux `max_user_watches`).
    LimitReached,
    /// Any other I/O error.
    Io,
    /// The watcher reported something else.
    Other,
}

/// Why every path under the root may have changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RescanReason {
    /// The hint queue filled up; its paths were dropped and counted.
    QueueFull,
    /// The operating system's own event queue overflowed (inotify `IN_Q_OVERFLOW`, FSEvents'
    /// must-scan flag).
    WatcherOverflow,
    /// A change at a path with no name under the root: the root itself, a path outside it, or a
    /// name that is not UTF-8.
    Unnamed,
    /// The watcher reported an error, or a directory could not be watched: part of the tree may
    /// now be unwatched, so later changes there may go unhinted.
    WatcherError(WatchErrorKind),
    /// The watch ended (its backend stopped): nothing is hinted any more.
    WatchEnded,
}

impl RescanReason {
    const ALL: [Self; 7] = [
        Self::QueueFull,
        Self::WatcherOverflow,
        Self::Unnamed,
        Self::WatcherError(WatchErrorKind::LimitReached),
        Self::WatcherError(WatchErrorKind::Io),
        Self::WatcherError(WatchErrorKind::Other),
        Self::WatchEnded,
    ];

    fn bit(self) -> u8 {
        match self {
            Self::QueueFull => 1,
            Self::WatcherOverflow => 1 << 1,
            Self::Unnamed => 1 << 2,
            Self::WatcherError(WatchErrorKind::LimitReached) => 1 << 3,
            Self::WatcherError(WatchErrorKind::Io) => 1 << 4,
            Self::WatcherError(WatchErrorKind::Other) => 1 << 5,
            Self::WatchEnded => 1 << 6,
        }
    }

    /// Whether this reason means the watch may have stopped seeing part of the tree for good,
    /// not just lost some events: one rescan does not make the view safe again.
    #[must_use]
    pub fn degrades_the_watch(self) -> bool {
        matches!(self, Self::WatcherError(_) | Self::WatchEnded)
    }
}

/// Every reason a rescan was asked for since the last one was taken: none is ever hidden behind
/// another (a lost watch behind a full queue, say).
///
/// Pinned by: `hints::tests::should_keep_every_reason_when_rescans_pile_up`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct RescanReasons(u8);

impl RescanReasons {
    /// The set holding only `reason`.
    #[must_use]
    pub fn of(reason: RescanReason) -> Self {
        Self(reason.bit())
    }

    /// Adds `reason`.
    pub fn insert(&mut self, reason: RescanReason) {
        self.0 |= reason.bit();
    }

    /// Whether `reason` is in the set.
    #[must_use]
    pub fn contains(self, reason: RescanReason) -> bool {
        self.0 & reason.bit() != 0
    }

    /// The reasons, in a fixed order.
    pub fn iter(self) -> impl Iterator<Item = RescanReason> {
        RescanReason::ALL
            .into_iter()
            .filter(move |reason| self.contains(*reason))
    }

    /// Whether any reason [degrades the watch](RescanReason::degrades_the_watch).
    #[must_use]
    pub fn degrade_the_watch(self) -> bool {
        self.iter().any(RescanReason::degrades_the_watch)
    }
}

/// Everything may have changed: reconcile the whole view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rescan {
    /// Every reason since the last rescan was taken.
    pub reasons: RescanReasons,
    /// The path hints this rescan replaced.
    pub dropped: u64,
}

/// A watcher's hint (plan §4.3): a reason to reconcile early, never a fact; the reconcile decides
/// what changed. Hints are not a complete record of changes: a change no hint names and no rescan
/// covers (the gaps `jfl_adapter_fs::watch` lists) is seen only by a request's own stat of that
/// file or by the next full reconcile, so a view must not claim `complete=true` from hints alone
/// (IDX-09's obligations, in its card).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Hint {
    /// Something at this path changed: a file written, created, removed or renamed, or a
    /// directory created, removed or renamed. For a directory, anything under it may have changed
    /// too (a rename reports only the directory). A new directory's hint is handed out only once
    /// the directory is watched, so a scan of it on the hint misses nothing written later
    /// (watch, then scan).
    Changed(RelPath),
    /// Something may have changed anywhere under the root.
    Rescan(Rescan),
}

/// What a change names, as far as the watcher knows: the extension rule of [`hint_excluded`]
/// applies only to files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintSubject {
    /// A regular file (written, created, removed).
    File,
    /// A directory.
    Directory,
    /// Unknown: a rename, or an event that does not say.
    Unknown,
}

/// Why a `SourceTree` call failed. The values carry no part of a path (fnd §5 rule 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SourceTreeError {
    /// The root does not exist.
    #[error("the root does not exist")]
    RootNotFound,
    /// The root is not a directory.
    #[error("the root is not a directory")]
    RootNotADirectory,
    /// The file does not exist.
    #[error("the file does not exist")]
    NotFound,
    /// The path names something other than a regular file.
    #[error("the path is not a regular file")]
    NotAFile,
    /// The operating system refused access.
    #[error("permission denied")]
    PermissionDenied,
    /// Any other I/O error.
    #[error("I/O error: {0}")]
    Io(std::io::ErrorKind),
    /// The watcher could not be started.
    #[error("the watcher could not start")]
    WatchFailed(WatchErrorKind),
    /// The blocking job did not finish (the pool shut down).
    #[error("interrupted")]
    Interrupted,
}

impl From<crate::blocking::Interrupted> for SourceTreeError {
    fn from(_: crate::blocking::Interrupted) -> Self {
        Self::Interrupted
    }
}

/// What kind of filesystem a root lives on, as `statfs` names it: whether a watch on it can be
/// trusted to see every change (ADR 0037, `doc/formal-models.md`, IDX-09's obligation 4).
///
/// Only [`FsKind::Local`] is watched. The rule is an allowlist of local filesystems, never a list
/// of bad ones: a filesystem this list does not know (a new one, a stacked one) is
/// [`FsKind::Other`] and is opened unwatched, so its answers are incomplete rather than wrong.
///
/// Pinned by: `source_tree::tests::should_watch_only_a_known_local_filesystem_when_classifying`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FsKind {
    /// A local disk filesystem whose changes inotify or FSEvents report (ext4, xfs, btrfs,
    /// tmpfs, zfs, APFS, HFS+, …).
    Local,
    /// A network filesystem: changes made by another client raise no event here (NFS, SMB/CIFS,
    /// 9p, AFS, Ceph, …).
    Network,
    /// A FUSE filesystem (sshfs, virtiofs, macFUSE): the daemon behind it decides what is
    /// reported, often nothing.
    Fuse,
    /// An overlay filesystem: a change to a lower layer raises no event on the overlay.
    Overlay,
    /// Any other filesystem, or one whose type could not be read.
    Other,
}

/// Linux `statfs` magic numbers (`linux/magic.h`) of the local filesystems a watch can trust.
const LINUX_LOCAL: [u64; 20] = [
    0xEF53,      // ext2, ext3, ext4
    0x5846_5342, // xfs
    0x9123_683E, // btrfs
    0x0102_1994, // tmpfs
    0x8584_58F6, // ramfs
    0x2FC1_2FC1, // zfs
    0xF2F5_2010, // f2fs
    0xCA45_1A4E, // bcachefs
    0x5265_4973, // reiserfs
    0x3153_464A, // jfs
    0x3434,      // nilfs2
    0x482B,      // hfsplus
    0x2011_BAB0, // exfat
    0x4D44,      // vfat
    0x7366_746E, // ntfs3
    0x5346_544E, // ntfs
    0x1501_3346, // udf
    0x7371_7368, // squashfs
    0xE0F5_E1E2, // erofs
    0x9660,      // iso9660
];

/// Linux `statfs` magic numbers of the network filesystems.
const LINUX_NETWORK: [u64; 11] = [
    0x6969,      // nfs
    0x517B,      // smb
    0xFE53_4D42, // smb2
    0xFF53_4D42, // cifs
    0x0102_1997, // v9fs (9p)
    0x0BD0_0BD0, // lustre
    0x00C3_6400, // ceph
    0x5346_414F, // afs
    0x6B41_4653, // kafs
    0x7375_7245, // coda
    0x564C,      // ncp
];

impl FsKind {
    /// Whether a watch on this filesystem is trusted: only a local one.
    #[must_use]
    pub fn watchable(self) -> bool {
        self == Self::Local
    }

    /// The kind of a Linux `statfs` `f_type`.
    #[must_use]
    pub fn from_linux_magic(magic: u64) -> Self {
        match magic {
            0x6573_5546 => Self::Fuse,
            0x794C_7630 => Self::Overlay,
            m if LINUX_NETWORK.contains(&m) => Self::Network,
            m if LINUX_LOCAL.contains(&m) => Self::Local,
            _ => Self::Other,
        }
    }

    /// The kind of a BSD or macOS `statfs` `f_fstypename`.
    #[must_use]
    pub fn from_bsd_name(name: &str) -> Self {
        match name {
            "apfs" | "hfs" | "ufs" | "zfs" | "tmpfs" | "msdos" | "exfat" | "cd9660" => Self::Local,
            "nfs" | "smbfs" | "afpfs" | "webdav" | "ftp" => Self::Network,
            n if n.contains("fuse") => Self::Fuse,
            _ => Self::Other,
        }
    }
}

/// The name of a cookie file (ADR 0037): `.jfl-cookie-<owner>-<stamp>` in the view's root, where
/// `owner` names the daemon that writes it (so a second daemon's cookies, another user's on a
/// shared worktree, are never taken for this one's) and `stamp` the cookie's place on the clock;
/// `-<attempt>` follows when an earlier attempt found a file of that name already there (one is
/// never removed to make room). The name passes [`hint_excluded`] (one leading dot, no other)
/// and is never listed (not `.java`). `None` when `owner` holds anything but ASCII letters and
/// digits.
///
/// Pinned by: `source_tree::tests::should_read_back_a_cookie_stamp_when_the_owner_wrote_it`.
#[must_use]
pub fn cookie_name(owner: &str, stamp: u64, attempt: u32) -> Option<RelPath> {
    if !is_owner(owner) {
        return None;
    }
    let name = if attempt == 0 {
        format!("{COOKIE_PREFIX}{owner}-{stamp}")
    } else {
        format!("{COOKIE_PREFIX}{owner}-{stamp}-{attempt}")
    };
    RelPath::new(&name).ok()
}

fn is_owner(owner: &str) -> bool {
    !owner.is_empty() && owner.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// Whether `name`, a file name in a view's root, is exactly a cookie of any daemon
/// ([`cookie_name`]'s shape): what a sweep of stale cookies may remove, and nothing else.
///
/// Pinned by: `source_tree::tests::should_read_back_a_cookie_stamp_when_the_owner_wrote_it`.
#[must_use]
pub fn is_cookie_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix(COOKIE_PREFIX) else {
        return false;
    };
    let mut parts = rest.split('-');
    let (Some(owner), Some(stamp)) = (parts.next(), parts.next()) else {
        return false;
    };
    let attempt_ok = parts.next().is_none_or(is_digits);
    is_owner(owner) && is_digits(stamp) && attempt_ok && parts.next().is_none()
}

/// What every cookie file's name starts with.
pub const COOKIE_PREFIX: &str = ".jfl-cookie-";

/// Whether `path` is a cookie file of any daemon: never a source change.
#[must_use]
pub fn is_cookie(path: &RelPath) -> bool {
    path.as_str().starts_with(COOKIE_PREFIX) && !path.as_str().contains('/')
}

/// The stamp of `path` when it is a cookie `owner` wrote ([`cookie_name`], any attempt).
#[must_use]
pub fn cookie_stamp(owner: &str, path: &RelPath) -> Option<u64> {
    if !is_cookie_name(path.as_str()) {
        return None;
    }
    let rest = path.as_str().strip_prefix(COOKIE_PREFIX)?;
    let after = rest.strip_prefix(owner)?.strip_prefix('-')?;
    after.split('-').next()?.parse().ok()
}

/// A stream of [`Hint`]s from one watch; dropping it stops the watch.
pub trait Hints: Send {
    /// The next hint, if one is waiting. May block briefly to watch a new directory before its
    /// hint is handed out: call it off the async runtime.
    fn try_next(&mut self) -> Option<Hint>;

    /// The next hint, waiting for one; `None` once the watch has ended and its last hint (a
    /// [`RescanReason::WatchEnded`] rescan) was taken. Cancel-safe: a hint is taken only when the
    /// future resolves.
    fn next(&mut self) -> impl Future<Output = Option<Hint>> + Send;
}

/// A worktree's source files (plan §6.4). Every call takes the worktree root; paths below it are
/// [`RelPath`]s, which cannot leave it.
///
/// What any implementation must do is the `source_tree` contract suite
/// (`jfl_testkit::contracts::source_tree`).
pub trait SourceTree: Send + Sync {
    /// The live watch [`SourceTree::watch`] returns.
    type Watch: Hints;

    /// Lists the `.java` regular files under `root`, applying `.gitignore` and [`exclusion`].
    /// Symbolic links are never followed. A tracked file that matches a `.gitignore` pattern is
    /// not listed, though Git still tracks it: a caller that must know every tracked file unions
    /// the listing with the repository's. Cancel-safe (it only reads).
    fn list(
        &self,
        root: &FileUri,
        mode: ListMode,
    ) -> impl Future<Output = Result<Listing, SourceTreeError>> + Send;

    /// Reads at most `limit` bytes of the regular file `root/path`. Cancel-safe.
    fn read(
        &self,
        root: &FileUri,
        path: &RelPath,
        limit: usize,
    ) -> impl Future<Output = Result<ReadBytes, SourceTreeError>> + Send;

    /// Stats the regular file `root/path`. Cancel-safe.
    fn stat(
        &self,
        root: &FileUri,
        path: &RelPath,
    ) -> impl Future<Output = Result<FileStat, SourceTreeError>> + Send;

    /// Starts watching `root`: what the watch covers is established when the future resolves, so
    /// a caller watches first and lists second. Later changes are hinted, except those
    /// [`hint_excluded`] leaves out; a new directory's hint is handed out only once it is watched;
    /// whatever the watch cannot cover is a [`Rescan`]. Cancel-safe: a dropped future's watch is
    /// dropped with it.
    fn watch(
        &self,
        root: &FileUri,
    ) -> impl Future<Output = Result<Self::Watch, SourceTreeError>> + Send;

    /// The kind of filesystem `root` lives on (`statfs`). The default, for an implementation
    /// that cannot tell, is [`FsKind::Other`]: a view it serves is never watched, so never
    /// claims `complete` (ADR 0037). Cancel-safe.
    fn filesystem(
        &self,
        root: &FileUri,
    ) -> impl Future<Output = Result<FsKind, SourceTreeError>> + Send {
        let _ = root;
        core::future::ready(Ok(FsKind::Other))
    }

    /// Removes the stale cookies in `root`: regular files directly in it whose name is exactly a
    /// cookie's ([`is_cookie_name`], any daemon's) and whose mtime is before `before`; never
    /// through a link, never anything else. A daemon stopped between creating a cookie and
    /// removing it leaves one, which `git status` would show and `git add -A` commit. Returns
    /// how many were removed. The default removes none. Cancel-safe.
    fn sweep_cookies(
        &self,
        root: &FileUri,
        before: MTime,
    ) -> impl Future<Output = Result<u64, SourceTreeError>> + Send {
        let _ = (root, before);
        core::future::ready(Ok(0))
    }

    /// Creates the file `root/name` and removes it again: a cookie (ADR 0037), whose events reach
    /// a live watch of `root` through the same in-order stream as every change made before it, as
    /// its hint or folded into a rescan that waits when they arrive, never neither (the
    /// `source_tree` contract's `cookie_follows_earlier_changes`).
    /// `name` is a [`cookie_name`]. A file already of that name is left alone and the call fails
    /// with `Io(AlreadyExists)`; the caller tries the next attempt's name. The default refuses
    /// (`Io(Unsupported)`): the cookie is then lost and the answer incomplete, never wrong.
    fn write_cookie(
        &self,
        root: &FileUri,
        name: &RelPath,
    ) -> impl Future<Output = Result<(), SourceTreeError>> + Send {
        let _ = (root, name);
        core::future::ready(Err(SourceTreeError::Io(std::io::ErrorKind::Unsupported)))
    }
}

/// What sits at `<dir>/.git`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitEntry {
    /// A file: `dir` is a linked worktree.
    File,
    /// A directory: `dir` is a repository.
    Dir,
}

/// What `lstat` found at `<dir>/.git`, symbolic links not followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GitMarker {
    /// A directory.
    Dir,
    /// A regular file.
    File,
    /// A symbolic link, dangling or not.
    Symlink,
    /// Anything else: a FIFO, a socket, a device.
    Other,
}

impl GitMarker {
    /// The marker for an entry of this type (from `std::fs::FileType`, which this crate may not
    /// name).
    #[must_use]
    pub fn of(is_dir: bool, is_file: bool, is_symlink: bool) -> Self {
        match (is_dir, is_file, is_symlink) {
            (true, _, _) => Self::Dir,
            (_, true, _) => Self::File,
            (_, _, true) => Self::Symlink,
            _ => Self::Other,
        }
    }
}

/// Whether a directory holding `found` at `.git` is a nested root, and of which kind: **any**
/// entry counts, whatever its type and wherever a link points (plan §4.2). The one rule the
/// listing (`jfl-adapter-fs`, which skips such a directory) and root resolution
/// (`jfl-adapter-git`, which binds a request below it to it, never to the parent) share, so a file
/// a view never lists is never answered from that view. A directory is a repository; anything else
/// a linked worktree (or a broken one, which resolution refuses).
///
/// Pinned by: `source_tree::tests::should_call_any_git_entry_a_nested_root_when_one_is_found`.
#[must_use]
pub fn nested_root(found: Option<GitMarker>) -> Option<GitEntry> {
    found.map(|marker| match marker {
        GitMarker::Dir => GitEntry::Dir,
        GitMarker::File | GitMarker::Symlink | GitMarker::Other => GitEntry::File,
    })
}

/// The rule for one directory below the root (never the root itself): `None` to walk into it.
///
/// `parent` is its parent's name (`None` when the parent is the root, whose name is not part of
/// the tree); `git` says what `<dir>/.git` is, and `parent_has_build_file` whether a
/// [`BUILD_FILES`] file sits beside it. Both are asked only when the cheaper name rules have not
/// decided, so an adapter stats as little as it can.
///
/// The name `.git` itself is reported as `None` by this function: the caller never walks into a
/// `.git` directory and never lists a `.git` file, and neither is a skipped directory worth
/// reporting (see [`is_git_metadata`]).
///
/// Pinned by: `source_tree::tests::should_skip_a_directory_when_a_rule_of_plan_4_2_applies` and
/// the `source_tree` contract suite.
pub fn exclusion(
    parent: Option<&str>,
    name: &str,
    git: impl FnOnce() -> Option<GitEntry>,
    parent_has_build_file: impl FnOnce() -> bool,
) -> Option<Exclusion> {
    if TOOL_STATE_DIRS.contains(&name) {
        return Some(Exclusion::BuildOutput);
    }
    if parent == Some(".claude") && name == "worktrees" {
        return Some(Exclusion::AgentWorktrees);
    }
    match git() {
        Some(GitEntry::File) => return Some(Exclusion::NestedWorktree),
        Some(GitEntry::Dir) => return Some(Exclusion::NestedRepository),
        None => {}
    }
    if BUILD_OUTPUT_DIRS.contains(&name) && parent_has_build_file() {
        return Some(Exclusion::BuildOutput);
    }
    None
}

/// Whether `name` is Git's own metadata (`.git`), which a walk never enters or lists.
#[must_use]
pub fn is_git_metadata(name: &str) -> bool {
    name == ".git"
}

/// Whether a file of this name is listed.
#[must_use]
pub fn is_java_file_name(name: &str) -> bool {
    name.len() > JAVA_EXTENSION.len() && name.ends_with(JAVA_EXTENSION)
}

/// Whether a change at `path` is left out of the hints: under a `.git` directory, under
/// `.claude/worktrees/`, or, for a [`HintSubject::File`] only, a file whose name has an extension
/// no listing or exclusion rule reads (a `.class` file a build writes, an editor's swap file).
/// `.java` files, other JVM languages' sources and build scripts ([`source_kind`]: their count
/// is the view's, IDX-16), `.gitignore` and [`BUILD_FILES`] are always hinted, and so is every directory
/// and every change of unknown kind, whatever its name: `java.base` and `org.eclipse.core` are
/// directories.
///
/// Pinned by: `source_tree::tests::should_leave_out_a_hint_when_its_path_is_never_listed` and the
/// `source_tree` contract suite's dotted-directory cases.
#[must_use]
pub fn hint_excluded(path: &RelPath, subject: HintSubject) -> bool {
    let mut previous = "";
    for component in path.components() {
        if is_git_metadata(component) || (previous == ".claude" && component == "worktrees") {
            return true;
        }
        previous = component;
    }
    if subject != HintSubject::File {
        return false;
    }
    let name = path.file_name();
    if source_kind(name).is_some() || name == ".gitignore" || BUILD_FILES.contains(&name) {
        return false;
    }
    name.get(1..).is_some_and(|rest| rest.contains('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(path: &str) -> RelPath {
        RelPath::new(path).unwrap()
    }

    #[test]
    fn should_classify_a_file_by_its_extension_when_a_listing_meets_it() {
        // given
        let named = [
            ("A.java", Some(SourceKind::Java)),
            ("Ledger.kt", Some(SourceKind::Other(OtherLanguage::Kotlin))),
            (
                "tool.main.kts",
                Some(SourceKind::Other(OtherLanguage::Kotlin)),
            ),
            (
                "Spec.groovy",
                Some(SourceKind::Other(OtherLanguage::Groovy)),
            ),
            ("run.gsh", Some(SourceKind::Other(OtherLanguage::Groovy))),
            ("Main.scala", Some(SourceKind::Other(OtherLanguage::Scala))),
            ("build.sc", Some(SourceKind::Other(OtherLanguage::Scala))),
            ("core.cljc", Some(SourceKind::Other(OtherLanguage::Clojure))),
            ("Gen.xtend", Some(SourceKind::Other(OtherLanguage::Xtend))),
            ("index.jsp", Some(SourceKind::Other(OtherLanguage::Jsp))),
            ("field.tagx", Some(SourceKind::Other(OtherLanguage::Jsp))),
            ("Audit.aj", Some(SourceKind::Other(OtherLanguage::AspectJ))),
            (
                "Calc.bsh",
                Some(SourceKind::Other(OtherLanguage::BeanShell)),
            ),
            ("list.gsp", Some(SourceKind::Other(OtherLanguage::Gsp))),
            ("page.jte", Some(SourceKind::Other(OtherLanguage::Jte))),
            ("page.kte", Some(SourceKind::Other(OtherLanguage::Jte))),
            ("page.ftl", None),
            ("page.vm", None),
            ("Shout.KT", Some(SourceKind::Other(OtherLanguage::Kotlin))),
            ("build.gradle", Some(SourceKind::BuildScript)),
            ("settings.gradle.kts", Some(SourceKind::BuildScript)),
            ("conventions.gradle", Some(SourceKind::BuildScript)),
            (".kt", None),
            (".gradle", None),
            ("app.cljs", None),
            ("tool.py", None),
            ("beans.xml", None),
            ("A.class", None),
            ("A.javax", None),
            ("Makefile", None),
            ("é.kt", Some(SourceKind::Other(OtherLanguage::Kotlin))),
        ];
        // when
        let kinds: Vec<Option<SourceKind>> = named.iter().map(|(n, _)| source_kind(n)).collect();
        // then
        let want: Vec<Option<SourceKind>> = named.iter().map(|(_, k)| *k).collect();
        assert_eq!(kinds, want);
        assert!(OTHER_JVM_EXTENSIONS.iter().all(|(e, _)| e.starts_with('.')));
    }

    #[test]
    fn should_count_build_logic_only_for_a_path_in_it_when_other_sources_are_summed() {
        // given: Kotlin beside the main sources, Groovy in buildSrc, build scripts, and an
        // included build's settings file; one more file counted without a path
        let listed = [
            rel("app/src/main/kotlin/a/Ledger.kt"),
            rel("app/src/main/kotlin/a/Ledger.kt"),
            rel("buildSrc/src/main/groovy/Lock.groovy"),
            rel("build.gradle"),
            rel("settings.gradle"),
            rel("build-logic/settings.gradle.kts"),
            rel("build-logic/build.gradle.kts"),
        ];
        let others = OtherSources::new(&listed, 1);
        let main = rel("app/src/main/java/a/Tally.java");
        let build_src = rel("buildSrc/src/main/java/b/Plugin.java");
        let included = rel("build-logic/src/main/java/c/Conventions.java");
        let lookalike = rel("build-logic-docs/A.java");
        // when
        let counts = [
            others.seen_from(&main),
            others.seen_from(&build_src),
            others.seen_from(&included),
            others.seen_from(&lookalike),
        ];
        // then: the main path sees the Kotlin file and the one without a path; build logic sees all
        assert_eq!(counts, [2, 7, 7, 2]);
        assert!(others.holds(&rel("build.gradle")));
        assert!(!others.holds(&rel("app/src/main/kotlin/a/Other.kt")));
        assert_eq!(others.total(), 7);
        assert_eq!(OtherSources::new(&[], 0).seen_from(&build_src), 0);
        let aspects = OtherSources::new(
            &[
                rel("app/src/main/aspect/Audit.aj"),
                rel("buildSrc/src/main/aspect/Weave.aj"),
            ],
            0,
        );
        assert_eq!(aspects.aspects_seen_from(&main), 1);
        assert_eq!(aspects.aspects_seen_from(&build_src), 2);
        assert_eq!(others.aspects_seen_from(&build_src), 0);
    }

    #[test]
    fn should_watch_only_a_known_local_filesystem_when_classifying() {
        // given
        let linux = [
            0xEF53,
            0x9123_683E,
            0x0102_1994,
            0x6969,
            0xFF53_4D42,
            0x6573_5546,
        ];
        let bsd = ["apfs", "hfs", "nfs", "smbfs", "macfuse", "devfs"];

        // when
        let linux = linux.map(FsKind::from_linux_magic);
        let bsd = bsd.map(FsKind::from_bsd_name);

        // then
        assert_eq!(
            linux,
            [
                FsKind::Local,
                FsKind::Local,
                FsKind::Local,
                FsKind::Network,
                FsKind::Network,
                FsKind::Fuse
            ]
        );
        assert_eq!(FsKind::from_linux_magic(0x794C_7630), FsKind::Overlay);
        assert_eq!(FsKind::from_linux_magic(0x1234_5678), FsKind::Other);
        assert_eq!(
            bsd,
            [
                FsKind::Local,
                FsKind::Local,
                FsKind::Network,
                FsKind::Network,
                FsKind::Fuse,
                FsKind::Other
            ]
        );
        assert!(FsKind::Local.watchable());
        assert!(!FsKind::Other.watchable() && !FsKind::Overlay.watchable());
    }

    #[test]
    fn should_read_back_a_cookie_stamp_when_the_owner_wrote_it() {
        // given
        let name = cookie_name("d42", 17, 0);

        // when
        let stamp = name.as_ref().and_then(|n| cookie_stamp("d42", n));
        let other = name.as_ref().and_then(|n| cookie_stamp("d4", n));

        // then
        assert_eq!(
            name.as_ref().map(RelPath::as_str),
            Some(".jfl-cookie-d42-17")
        );
        assert_eq!(stamp, Some(17));
        assert_eq!(other, None);
        assert!(name.as_ref().is_some_and(is_cookie));
        assert!(name.is_some_and(|n| !hint_excluded(&n, HintSubject::File)));
        assert_eq!(cookie_name("d.1", 1, 0), None);
        let retry = cookie_name("d42", 17, 2);
        assert_eq!(
            retry.as_ref().map(RelPath::as_str),
            Some(".jfl-cookie-d42-17-2")
        );
        assert_eq!(
            retry.as_ref().and_then(|n| cookie_stamp("d42", n)),
            Some(17)
        );
        assert!(is_cookie_name(".jfl-cookie-d42-17") && is_cookie_name(".jfl-cookie-x-1-2"));
        let not = [
            ".jfl-cookie-",
            ".jfl-cookie-d42",
            ".jfl-cookie-d.4-1",
            ".jfl-cookie-d-1-2-3",
            ".jfl-cookie-d-1x",
        ];
        assert!(not.iter().all(|n| !is_cookie_name(n)), "{not:?}");
        assert_eq!(cookie_stamp("d42", &rel(".jfl-cookie-d42-")), None);
        assert!(!is_cookie(&rel("A.java")));
    }

    #[test]
    fn should_skip_a_directory_when_a_rule_of_plan_4_2_applies() {
        // given
        let no_git = || None;
        let no_build = || false;

        // when
        let cases = [
            exclusion(Some("x"), ".gradle", no_git, no_build),
            exclusion(Some(".claude"), "worktrees", no_git, no_build),
            exclusion(Some("x"), "agent", || Some(GitEntry::File), no_build),
            exclusion(Some("x"), "vendored", || Some(GitEntry::Dir), no_build),
            exclusion(None, "build", no_git, || true),
            exclusion(Some("x"), "target", no_git, || true),
        ];

        // then
        assert_eq!(
            cases,
            [
                Some(Exclusion::BuildOutput),
                Some(Exclusion::AgentWorktrees),
                Some(Exclusion::NestedWorktree),
                Some(Exclusion::NestedRepository),
                Some(Exclusion::BuildOutput),
                Some(Exclusion::BuildOutput),
            ]
        );
    }

    #[test]
    fn should_call_any_git_entry_a_nested_root_when_one_is_found() {
        // given
        let found = [
            None,
            Some(GitMarker::of(true, false, false)),
            Some(GitMarker::of(false, true, false)),
            Some(GitMarker::of(false, false, true)),
            Some(GitMarker::of(false, false, false)),
        ];

        // when
        let roots = found.map(nested_root);

        // then
        assert_eq!(
            roots,
            [
                None,
                Some(GitEntry::Dir),
                Some(GitEntry::File),
                Some(GitEntry::File),
                Some(GitEntry::File)
            ]
        );
    }

    #[test]
    fn should_walk_a_directory_when_a_build_name_has_no_build_file_beside_it() {
        // given
        let spring_package = ("springframework", "build");

        // when
        let verdict = exclusion(Some(spring_package.0), spring_package.1, || None, || false);
        let worktrees_elsewhere = exclusion(Some("docs"), "worktrees", || None, || false);

        // then
        assert_eq!(verdict, None);
        assert_eq!(worktrees_elsewhere, None);
    }

    #[test]
    fn should_not_ask_for_a_stat_when_a_name_rule_decides() {
        // given
        let mut asked = false;

        // when
        let verdict = exclusion(
            Some(".claude"),
            "worktrees",
            || {
                asked = true;
                None
            },
            || false,
        );

        // then
        assert_eq!(verdict, Some(Exclusion::AgentWorktrees));
        assert!(!asked);
    }

    #[test]
    fn should_leave_out_a_hint_when_its_path_is_never_listed() {
        // given
        let left_out = [
            (".git/index", HintSubject::File),
            ("sub/.git", HintSubject::Directory),
            (".claude/worktrees/a/src", HintSubject::Unknown),
            ("build/classes/A.class", HintSubject::File),
            ("src/.A.java.swp", HintSubject::File),
        ];
        let kept = [
            ("src/A.java", HintSubject::File),
            ("src", HintSubject::Directory),
            (".gitignore", HintSubject::File),
            ("lib/build.gradle.kts", HintSubject::File),
            ("lib/conventions.gradle", HintSubject::File),
            ("src/main/kotlin/Ledger.kt", HintSubject::File),
            ("src/test/groovy/Spec.groovy", HintSubject::File),
            ("Makefile", HintSubject::File),
            ("modules/java.base", HintSubject::Directory),
            ("org.eclipse.core", HintSubject::Unknown),
            ("build/classes/A.class", HintSubject::Unknown),
        ];

        // when
        let out: Vec<bool> = left_out
            .iter()
            .map(|(p, s)| hint_excluded(&rel(p), *s))
            .collect();
        let kept: Vec<bool> = kept
            .iter()
            .map(|(p, s)| hint_excluded(&rel(p), *s))
            .collect();

        // then
        assert!(out.iter().all(|x| *x), "{out:?}");
        assert!(kept.iter().all(|x| !*x), "{kept:?}");
    }

    #[test]
    fn should_say_a_watch_is_degraded_when_a_watcher_error_is_among_the_reasons() {
        // given
        let mut reasons = RescanReasons::of(RescanReason::QueueFull);
        let before = reasons.degrade_the_watch();

        // when
        reasons.insert(RescanReason::WatcherError(WatchErrorKind::LimitReached));

        // then
        assert!(!before);
        assert!(reasons.degrade_the_watch());
        assert_eq!(
            reasons.iter().collect::<Vec<_>>(),
            [
                RescanReason::QueueFull,
                RescanReason::WatcherError(WatchErrorKind::LimitReached)
            ]
        );
    }

    #[test]
    fn should_list_a_file_when_its_name_ends_in_java() {
        // given
        let names = ["A.java", ".java", "A.javax", "A.JAVA", "java"];

        // when
        let listed: Vec<bool> = names.iter().map(|n| is_java_file_name(n)).collect();

        // then
        assert_eq!(listed, [true, false, false, false, false]);
    }

    #[test]
    fn should_keep_every_gap_counted_when_more_than_the_cap_are_recorded() {
        // given
        let mut listing = Listing::default();

        // when
        for _ in 0..MAX_REPORTED_UNLISTED + 5 {
            listing.record_unlisted(Unlisted {
                path: None,
                why: UnlistedReason::Unreadable,
            });
        }

        // then
        assert_eq!(listing.unlisted.len(), MAX_REPORTED_UNLISTED);
        assert_eq!(listing.unlisted_count, 1029);
        assert!(!listing.is_complete());
    }
}
