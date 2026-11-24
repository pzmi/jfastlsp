//! Name resolution from syntax alone (IDX-10's definitions, `doc/tasks.md`): which declaration of
//! the pinned generation a name at a site binds to, following the Java scoping rules a summary
//! lets this crate follow, and nothing more.
//!
//! **What this proves: only what `prove` proves.** The scope rules below rank candidates;
//! `super::admit` answers every binding they find as `possible`, because two adversarial reviews
//! (javac as the oracle) found them wrong where Java's structure is re-derived from text: an
//! anonymous class behind a nested `new`, a `[` or a lambda block in its arguments; a comment
//! before an element key or a `case` label; a class header or `super(...)` resolved in the
//! body's scope; a type's own name taken before an inherited member type of that name; an
//! annotation before a type parameter clause. The syntax-tree resolver (`prove`, over the
//! indexer's parse of the request's file) proves a site from the tree's facts and this module's
//! member walks; both read a type name in the JLS order (inherited member types before the type's
//! own name, the second review's N4). Where the rules call a binding "exact" below, read "the one
//! candidate they offer".
//!
//! **The principle: this does not model Java.** When syntax cannot prove a binding, the answer
//! is `possible` with a reason, or nothing with the reason the target is unresolved. The rules
//! assume the code compiles.
//!
//! **What counts as evidence.** A binding is `exact` only when every place Java could take the
//! name from is in view and exactly one declares it:
//! - the enclosing types and their members, through supertypes that each resolve the same way
//!   (a *closed* hierarchy), with what Java does not inherit left out: private members, an
//!   interface's static methods, a package-private member unless every type down the walk is in
//!   its package; a supertype's method counts as overridden only when its parameter types are
//!   proved the same, else both are overloads (JLS 8.2, 8.4.8);
//! - the file's own types and its single-type imports (`TargetNotIndexed` when the import's
//!   type is not in view);
//! - a qualified type name by its first segment through the same scope; only a first segment
//!   that is no type may be a package, and then a view type of that name is only a candidate.
//!   A default-package type is never visible from a named package.
//!
//! Never `exact`, until what is named here is known:
//! - the file's package and its on-demand imports (`possible`): the package may hold types the
//!   snapshot lacks (a pending file, a gap, generated sources) and source sets are not told
//!   apart, so a test type may be offered to main code; the project model (PROJ) tightens this;
//! - a site inside an anonymous class, a local class or an enum constant's body (their members
//!   are not in the summary), found by scanning the member's text;
//! - a bare name that is an annotation's element key or a `case` label (`Unknown`), or that a
//!   local of the member may declare: a pattern variable, a lambda or catch parameter, a for or
//!   resource variable (`possible`);
//! - a receiver whose type syntax does not give (`possible`, by name and arity); `this`,
//!   `Outer.this` and a bare type name no variable can be are the receivers that are given.
//!
//! **Bounded ambiguity** (plan §2.2): once the owner type of a method is proved, the answer is
//! the overloads of that name among the owner's members, never a same-name member of another
//! type. One applicable overload by arity, in a closed hierarchy, is `exact`.
//!
//! **Bounded work.** Each lookup is a few binary searches (a type's members are indexed by
//! parent), a query makes at most [`STEP_BUDGET`] of them, and the deadline is read at the first
//! and every [`DEADLINE_EVERY`] lookups after: once it passes, the resolution is cut and the
//! answer says `deadline`.
//!
//! Pinned by `tests/queries.rs` (each rule over the fixture project, and the review's probes with
//! javac's binding as the oracle) and `resolve::tests`.

use core::fmt;
use core::ops::Deref;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use jfl_domain::{
    ByteRange, DeclId, DeclKind, Declaration, FileSummary, Modifiers, RefShape, ReferenceSite,
    RelPath, SyntaxKey,
};

use super::Deadline;
use super::scan::{self, Before, Scope};

mod prove;

/// The deepest supertype chain followed; a deeper one (or a cycle) is treated as leaving the
/// index, which is conservative.
const MAX_DEPTH: usize = 16;

/// The most lookups (a type's members, a type name) one query makes: hostile hierarchies (wide
/// and deep supertype clauses) cannot make it slow. Past it, every further lookup answers
/// "outside the index", which is conservative. Pinned by
/// `resolve::tests::should_stop_at_the_budget_when_a_hierarchy_is_hostile`.
pub(crate) const STEP_BUDGET: u32 = 20_000;

/// Object's methods: every class and interface has members of these names outside the index.
const OBJECT_METHODS: [&str; 9] = [
    "equals",
    "hashCode",
    "toString",
    "getClass",
    "notify",
    "notifyAll",
    "wait",
    "clone",
    "finalize",
];

/// `java.lang.Enum`'s methods and the ones every enum is given.
const ENUM_METHODS: [&str; 7] = [
    "name",
    "ordinal",
    "compareTo",
    "getDeclaringClass",
    "describeConstable",
    "values",
    "valueOf",
];

/// `java.lang.Enum`'s member types.
const ENUM_TYPES: [&str; 1] = ["EnumDesc"];

/// Java's primitive types and `void`.
const PRIMITIVES: [&str; 9] = [
    "boolean", "byte", "char", "short", "int", "long", "float", "double", "void",
];

/// Where a member walk is.
#[derive(Debug, Clone)]
struct Walk {
    /// How deep in the supertypes.
    depth: usize,
    /// Whether the type walked is a supertype of the lookup's owner (its members are inherited).
    inherited: bool,
    /// The package every type from the owner down to here is in; `None` once two differ.
    package: Option<String>,
    /// The types walked from the lookup's owner down to here, this one left out: each is a proper
    /// subtype of this one along the walk's own edges.
    below: Vec<Hit>,
}

/// A declaration of the snapshot: its file and its id there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Hit {
    /// The file, an index into the snapshot's files.
    pub(crate) file: u32,
    /// The declaration.
    pub(crate) decl: DeclId,
}

/// One indexed file of the pinned generation.
#[derive(Debug, Clone)]
pub(crate) struct FileEntry {
    /// Its path under the view's root.
    pub(crate) path: RelPath,
    /// The key of the bytes it summarises.
    pub(crate) key: SyntaxKey,
    /// Its summary, shared with the generation.
    pub(crate) summary: Arc<FileSummary>,
}

/// A pinned generation's summaries in path order, with every declaration by simple name (a
/// constructor under its type's) and the type declarations apart: built once per generation and
/// shared by its queries ([`super::QueryCache`]).
#[derive(Debug)]
pub(crate) struct Snapshot {
    files: Vec<FileEntry>,
    types: HashMap<Box<str>, Vec<Hit>>,
    names: HashMap<Box<str>, Vec<Hit>>,
    table: NameTable,
    /// Per file, `(parent, child)` for every declaration with a parent, by parent: a type's
    /// members are a binary search away, so a lookup's cost does not grow with its file.
    children: Vec<Vec<(DeclId, DeclId)>>,
    /// Per file, whether its types may hold members its summary lacks ([`generates_members`]).
    generated: Vec<bool>,
    /// How many files use a known annotation processor ([`uses_processor`]).
    processors: u32,
    /// How many files had a syntax error when summarised ([`FileSummary::syntax_errors`]).
    broken: u32,
}

/// The packages of the annotation processors references know of (IDX-11, the card's Decided
/// rule): a file that imports from one, or names one of its annotations qualified, may have
/// sources generated beside it (a mapper's implementation, a builder, a JPA metamodel, a
/// dependency-injection factory) that use its declarations and are not in the view before the
/// project model names the generated roots (M3). A heuristic towards incompleteness: a
/// processor it does not know is not seen, which PROJ's processor path closes.
pub(crate) const PROCESSOR_PACKAGES: [&str; 14] = [
    "lombok",
    "org.mapstruct",
    "org.immutables",
    "com.google.auto",
    "dagger",
    "javax.inject",
    "jakarta.inject",
    "io.micronaut",
    "javax.persistence",
    "jakarta.persistence",
    "org.hibernate.annotations",
    "com.querydsl",
    "io.soabase.recordbuilder",
    "org.inferred.freebuilder",
];

/// Whether `summary` uses a known annotation processor ([`PROCESSOR_PACKAGES`]): an import from
/// one of its packages, or an annotation named qualified in one.
///
/// Pinned by: `resolve::tests::should_see_a_processor_when_a_file_imports_or_names_one`.
pub(crate) fn uses_processor(summary: &FileSummary) -> bool {
    let known = |name: &str| {
        PROCESSOR_PACKAGES.iter().any(|p| {
            name.strip_prefix(p)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
        })
    };
    summary.imports().iter().any(|i| known(summary.str(i.name)))
        || summary
            .declarations()
            .any(|(_, d)| summary.list(d.annotations).any(known))
}

/// Whether the types of a file may hold members its source does not declare: Lombok rewrites the
/// classes it annotates inside javac (accessors, constructors, a `log` field, a `Fields` type),
/// so a file that imports it or names one of its annotations qualified is treated as having
/// members outside the index. Other annotation processors write new files, which are not in the
/// view and so are outside anyway.
///
/// Pinned by: `resolve::tests::should_count_members_outside_when_a_file_uses_lombok`.
fn generates_members(summary: &FileSummary) -> bool {
    let lombok = |name: &str| name == "lombok" || name.starts_with("lombok.");
    summary
        .imports()
        .iter()
        .any(|i| lombok(summary.str(i.name)))
        || summary
            .declarations()
            .any(|(_, d)| summary.list(d.annotations).any(lombok))
}

/// The distinct declaration names laid out in one text, each with its declarations: what a
/// workspace symbol scan reads front to back (a hash map's boxed keys cost a cache miss each:
/// 8.7 ms against 2 ms on the medium corpus, `just bench-queries`).
#[derive(Debug, Default)]
struct NameTable {
    text: String,
    /// Per name: its bytes in `text`, and its declarations in `hits`.
    names: Vec<(u32, u32, u32, u32)>,
    hits: Vec<Hit>,
}

/// One query's resolution over a [`Snapshot`], with its own lookup budget.
pub(crate) struct Resolver<'s> {
    snapshot: &'s Snapshot,
    /// Lookups left ([`STEP_BUDGET`]).
    steps: AtomicU32,
    /// Read every [`DEADLINE_EVERY`] lookups; once passed, the budget is spent.
    deadline: Option<&'s dyn Deadline>,
    /// Whether the deadline passed during the resolution.
    cut: AtomicBool,
}

impl fmt::Debug for Resolver<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Resolver")
            .field("steps", &self.steps)
            .field("cut", &self.cut)
            .finish_non_exhaustive()
    }
}

/// How many lookups pass between two readings of the deadline: each lookup is a few binary
/// searches, so the deadline is read every few microseconds of work.
const DEADLINE_EVERY: u32 = 64;

impl<'s> Resolver<'s> {
    /// A resolution over `snapshot` with a full budget and no deadline.
    #[cfg(test)]
    pub(crate) fn new(snapshot: &'s Snapshot) -> Self {
        Self::with_deadline(snapshot, None)
    }

    /// A resolution over `snapshot` that stops when `deadline` passes.
    pub(crate) fn with_deadline(
        snapshot: &'s Snapshot,
        deadline: Option<&'s dyn Deadline>,
    ) -> Self {
        Self {
            snapshot,
            steps: AtomicU32::new(STEP_BUDGET),
            deadline,
            cut: AtomicBool::new(false),
        }
    }

    /// Whether the deadline passed during the resolution: its outcome is then not an answer.
    pub(crate) fn cut(&self) -> bool {
        self.cut.load(Ordering::Relaxed)
    }
}

impl Deref for Resolver<'_> {
    type Target = Snapshot;

    fn deref(&self) -> &Snapshot {
        self.snapshot
    }
}

/// The kind of member a lookup wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Member {
    /// Methods (a record's components count as accessors).
    Method,
    /// Fields, enum constants and record components.
    Field,
    /// Member types.
    Type,
}

impl Member {
    fn accepts(self, kind: DeclKind) -> bool {
        match self {
            Self::Method => matches!(
                kind,
                DeclKind::Method | DeclKind::AnnotationElement | DeclKind::RecordComponent
            ),
            Self::Field => matches!(
                kind,
                DeclKind::Field | DeclKind::EnumConstant | DeclKind::RecordComponent
            ),
            Self::Type => kind.is_type(),
        }
    }
}

/// A type's members of one name and kind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Members {
    /// The members, the type's own first, then each supertype's in walk order; a method that one
    /// lower in the hierarchy overrides (same parameter types) is left out.
    pub(crate) hits: Vec<Hit>,
    /// How many of `hits` the type itself declares.
    pub(crate) own: usize,
    /// A type outside the index (a library supertype, an implicit one such as `Object`) may hold
    /// more members of this name.
    pub(crate) outside: bool,
}

/// What a type name binds to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TypeBinding {
    /// One declaration, with evidence.
    Found(Hit),
    /// Candidates of the view that it may bind to, none proved, and why.
    Unproved(Vec<Hit>, Unproved),
    /// Every place in the view was searched and none declares it: it is outside the index.
    Outside,
}

/// Why a binding is not proved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unproved {
    /// A supertype outside the index (or an anonymous class's) may declare the name first.
    Supertype,
    /// Several declarations of the view fit equally (duplicates, or a type variable).
    Ambiguous,
    /// An on-demand or module import that may name a type outside the index.
    Import,
    /// The name may be a type variable, a local class or a local variable (a pattern variable,
    /// a lambda parameter), which the index does not hold.
    Local,
    /// A type of the file's package: never proved before the project model (PROJ), since the
    /// package may hold types the snapshot lacks, and source sets are not told apart.
    SamePackage,
    /// Bound by the scope rules, which [`super::admit`] does not take as evidence.
    ScopeRules,
    /// The overloads of an owner the scope rules bound, which `admit` does not take as evidence.
    Overloads,
}

/// What a name at a site binds to: the answer before positions are converted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// One declaration, with evidence.
    Exact(Hit),
    /// The owner type is proved, the overload is not: every overload of the name among its
    /// members (bounded ambiguity), the applicable ones first. `outside`: a supertype outside the
    /// index may hold more, so the list may miss the target.
    Overloads {
        /// Every overload, in walk order.
        all: Vec<Hit>,
        /// The overloads applicable to the site's arity.
        applicable: Vec<Hit>,
        /// More may be declared outside the index.
        outside: bool,
    },
    /// Candidates, none proved, and why each could not be.
    Unproved(Vec<Hit>, Unproved),
    /// A receiver of unknown type: every member of the name in the view, by name (and arity, for
    /// a call), and whether the list was cut at the cap.
    Receiver {
        /// The candidates.
        hits: Vec<Hit>,
        /// More matched than the cap.
        capped: bool,
        /// A method reference: matched by name only.
        method_ref: bool,
    },
    /// Declared outside the index, if the code compiles.
    Outside,
    /// Nothing at all could be said.
    Unknown,
}

/// The site under a cursor.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Cursor {
    /// The name of a declaration.
    Declaration(DeclId),
    /// A reference site.
    Site(ReferenceSite),
}

/// Whether `offset` is in `range`, its end included (a cursor just past a name is on it).
fn touches(range: ByteRange, offset: u32) -> bool {
    range.start().get() <= offset && offset <= range.end().get()
}

fn len(range: ByteRange) -> u32 {
    range.end().get().saturating_sub(range.start().get())
}

/// The declaration name or reference site at `offset` of `summary`: a declaration's name first,
/// then the narrowest site.
pub(crate) fn cursor(summary: &FileSummary, offset: u32) -> Option<Cursor> {
    if let Some((id, _)) = summary
        .declarations()
        .filter(|(_, d)| touches(d.name_range, offset))
        .min_by_key(|(_, d)| len(d.name_range))
    {
        return Some(Cursor::Declaration(id));
    }
    summary
        .references()
        .iter()
        .filter(|r| touches(r.range, offset))
        .min_by_key(|r| (len(r.range), r.shape.code()))
        .map(|r| Cursor::Site(*r))
}

impl Snapshot {
    /// A snapshot of `files`, in path order.
    pub(crate) fn new(files: Vec<FileEntry>) -> Self {
        let mut types: HashMap<Box<str>, Vec<Hit>> = HashMap::new();
        let mut names: HashMap<Box<str>, Vec<Hit>> = HashMap::new();
        for (file, f) in (0_u32..).zip(&files) {
            for (decl, d) in f.summary.declarations() {
                let name = f.summary.str(d.name);
                let hit = Hit { file, decl };
                if let Some(hits) = names.get_mut(name) {
                    hits.push(hit);
                } else {
                    names.insert(name.into(), vec![hit]);
                }
                if d.kind.is_type() {
                    if let Some(hits) = types.get_mut(name) {
                        hits.push(hit);
                    } else {
                        types.insert(name.into(), vec![hit]);
                    }
                }
            }
        }
        let mut table = NameTable::default();
        for (name, hits) in &names {
            let (Ok(start), Ok(len), Ok(first), Ok(count)) = (
                u32::try_from(table.text.len()),
                u32::try_from(name.len()),
                u32::try_from(table.hits.len()),
                u32::try_from(hits.len()),
            ) else {
                break;
            };
            table.text.push_str(name);
            table.hits.extend_from_slice(hits);
            table.names.push((start, len, first, count));
        }
        let children = files
            .iter()
            .map(|f| {
                let mut pairs: Vec<(DeclId, DeclId)> = f
                    .summary
                    .declarations()
                    .filter_map(|(id, d)| Some((d.parent?, id)))
                    .collect();
                pairs.sort_unstable();
                pairs
            })
            .collect();
        let generated = files
            .iter()
            .map(|f| generates_members(&f.summary))
            .collect();
        let processors = files.iter().filter(|f| uses_processor(&f.summary)).count();
        let broken = files.iter().filter(|f| f.summary.syntax_errors()).count();
        Self {
            files,
            types,
            names,
            table,
            children,
            generated,
            processors: u32::try_from(processors).unwrap_or(u32::MAX),
            broken: u32::try_from(broken).unwrap_or(u32::MAX),
        }
    }

    /// How many files had a syntax error when summarised: their sites may be lost, so no answer
    /// that needs every site of the view is complete while one is (the queries' completeness).
    pub(crate) const fn syntax_error_files(&self) -> u32 {
        self.broken
    }

    /// The files, in path order.
    pub(crate) fn files(&self) -> &[FileEntry] {
        &self.files
    }

    /// How many files use a known annotation processor ([`uses_processor`]).
    pub(crate) const fn processor_files(&self) -> u32 {
        self.processors
    }

    /// Whether the top-level type around `hit` has a qualified name another type declaration of
    /// the view has too (a second source set, a copy): which one a file sees is not known.
    ///
    /// Pinned by: `resolve::tests::should_say_declared_twice_when_two_files_declare_one_qualified_name`.
    pub(crate) fn declared_twice(&self, hit: Hit) -> bool {
        let Some(f) = self.file(hit.file) else {
            return true;
        };
        let mut top = hit.decl;
        for _ in 0..=MAX_DEPTH.saturating_mul(8) {
            match f.summary.declaration(top).and_then(|d| d.parent) {
                Some(parent) => top = parent,
                None => break,
            }
        }
        let Some(d) = f.summary.declaration(top) else {
            return true;
        };
        let top = Hit {
            file: hit.file,
            decl: top,
        };
        let qualified = self.qualified(top);
        self.types_named(f.summary.str(d.name))
            .iter()
            .any(|&other| other != top && self.qualified(other) == qualified)
    }

    /// Every declaration of the view by simple name, each name once.
    pub(crate) fn names(&self) -> impl Iterator<Item = (&str, &[Hit])> {
        let t = &self.table;
        t.names
            .iter()
            .filter_map(move |&(start, len, first, count)| {
                let at = |n: u32| usize::try_from(n).ok();
                let name = t.text.get(at(start)?..at(start.checked_add(len)?)?)?;
                let hits = t.hits.get(at(first)?..at(first.checked_add(count)?)?)?;
                Some((name, hits))
            })
    }

    /// The file of `hit`.
    pub(crate) fn file(&self, file: u32) -> Option<&FileEntry> {
        self.files.get(usize::try_from(file).ok()?)
    }

    /// The index of the file at `path`.
    pub(crate) fn file_at(&self, path: &RelPath) -> Option<u32> {
        let at = self.files.binary_search_by(|f| f.path.cmp(path)).ok()?;
        u32::try_from(at).ok()
    }

    /// The declaration `hit` names.
    pub(crate) fn decl(&self, hit: Hit) -> Option<&Declaration> {
        self.file(hit.file)?.summary.declaration(hit.decl)
    }

    /// The fully qualified name of a type declaration: package, enclosing types, name, dotted.
    pub(crate) fn qualified(&self, hit: Hit) -> String {
        let Some(f) = self.file(hit.file) else {
            return String::new();
        };
        let mut names = Vec::new();
        let mut next = Some(hit.decl);
        while let Some(id) = next {
            let Some(d) = f.summary.declaration(id) else {
                break;
            };
            names.push(f.summary.str(d.name));
            next = d.parent;
            if names.len() > MAX_DEPTH.saturating_mul(8) {
                break;
            }
        }
        if let Some(package) = f.summary.package() {
            names.push(package);
        }
        names.reverse();
        names.join(".")
    }

    /// The declarations of `file` whose range holds `offset`, innermost first.
    pub(crate) fn enclosing(&self, file: u32, offset: u32) -> Vec<DeclId> {
        let Some(f) = self.file(file) else {
            return Vec::new();
        };
        let mut chain: Vec<DeclId> = f
            .summary
            .declarations()
            .filter(|(_, d)| d.range.start().get() <= offset && offset < d.range.end().get())
            .map(|(id, _)| id)
            .collect();
        // Parents precede children, so the containing ones come outermost first.
        chain.reverse();
        chain
    }

    /// The declarations directly inside `owner`.
    fn children(&self, owner: Hit) -> impl Iterator<Item = Hit> + '_ {
        let pairs = usize::try_from(owner.file)
            .ok()
            .and_then(|f| self.children.get(f))
            .map_or(&[][..], Vec::as_slice);
        let from = pairs.partition_point(|&(p, _)| p < owner.decl);
        let to = pairs.partition_point(|&(p, _)| p <= owner.decl);
        pairs
            .get(from..to)
            .unwrap_or(&[])
            .iter()
            .map(move |&(_, decl)| Hit {
                file: owner.file,
                decl,
            })
    }

    /// The declarations directly inside `owner` named `name` that `member` accepts.
    fn declared(&self, owner: Hit, name: &str, member: Member) -> Vec<Hit> {
        let Some(f) = self.file(owner.file) else {
            return Vec::new();
        };
        self.children(owner)
            .filter(|&h| {
                f.summary
                    .declaration(h.decl)
                    .is_some_and(|d| member.accepts(d.kind) && f.summary.str(d.name) == name)
            })
            .collect()
    }

    /// Whether `file`'s types may hold members its summary lacks ([`generates_members`]).
    fn generated(&self, file: u32) -> bool {
        usize::try_from(file)
            .ok()
            .and_then(|f| self.generated.get(f))
            .copied()
            .unwrap_or(true)
    }

    /// The package of `file` (`""` for the default package).
    fn package(&self, file: u32) -> &str {
        self.file(file)
            .and_then(|f| f.summary.package())
            .unwrap_or("")
    }

    /// The parameter types of a callable (none for a record component: its accessor).
    fn params(&self, hit: Hit) -> Vec<&str> {
        match (self.file(hit.file), self.decl(hit)) {
            (Some(f), Some(d)) => f.summary.list(d.params).collect(),
            _ => Vec::new(),
        }
    }

    /// Every type declaration of the view named `name`.
    pub(crate) fn types_named(&self, name: &str) -> &[Hit] {
        self.types.get(name).map_or(&[], Vec::as_slice)
    }

    /// The type declarations of a named package whose qualified name is `qualified` (a type of
    /// the default package is never visible by a qualified name, JLS 7.4.2).
    fn types_qualified(&self, qualified: &str) -> Vec<Hit> {
        let simple = qualified.rsplit('.').next().unwrap_or(qualified);
        self.types_named(simple)
            .iter()
            .copied()
            .filter(|&h| !self.package(h.file).is_empty() && self.qualified(h) == qualified)
            .collect()
    }
}

impl Resolver<'_> {
    /// The members of `owner` named `name` that `member` accepts, through its closed hierarchy.
    pub(crate) fn members(&self, owner: Hit, name: &str, member: Member) -> Members {
        self.members_at(owner, name, member, 0)
    }

    /// One lookup of the budget; `false` once it is spent or the deadline passed.
    fn step(&self) -> bool {
        let Ok(left) = self
            .steps
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                left.checked_sub(1)
            })
        else {
            return false;
        };
        // The first lookup reads the deadline too: a query that starts late stops at once.
        if STEP_BUDGET.saturating_sub(left) % DEADLINE_EVERY == 0
            && self.deadline.is_some_and(|d| d.passed())
        {
            self.cut.store(true, Ordering::Relaxed);
            self.steps.store(0, Ordering::Relaxed);
            return false;
        }
        true
    }

    /// [`Snapshot::members`] from `depth` of a lookup already under way.
    fn members_at(&self, owner: Hit, name: &str, member: Member, depth: usize) -> Members {
        let mut out = Members::default();
        let mut seen = BTreeSet::new();
        let walk = Walk {
            depth,
            inherited: false,
            package: Some(self.package(owner.file).to_owned()),
            below: Vec::new(),
        };
        self.walk_members(owner, name, member, &walk, &mut seen, &mut out);
        out.own = out
            .hits
            .iter()
            .take_while(|h| {
                h.file == owner.file && self.decl(**h).and_then(|d| d.parent) == Some(owner.decl)
            })
            .count();
        out
    }

    /// Collects `owner`'s members named `name` into `out`, then its supertypes'. A supertype's
    /// member is collected only if Java inherits it (JLS 8.2, 8.4.8, 9.4.1): never a private one,
    /// never an interface's static method, and a package-private one only when every type from
    /// the lookup's owner down to it is in its package. A supertype's method is left out as
    /// overridden only when its parameter types are proved the same as a method already
    /// collected from a proper subtype on the same branch of the walk
    /// ([`Resolver::same_parameters`]); otherwise both are kept. A type that
    /// declares a field or member type of that name ends its branch, the member inherited or not.
    fn walk_members(
        &self,
        owner: Hit,
        name: &str,
        member: Member,
        walk: &Walk,
        seen: &mut BTreeSet<Hit>,
        out: &mut Members,
    ) {
        if walk.depth > MAX_DEPTH || !seen.insert(owner) || !self.step() {
            out.outside = true;
            return;
        }
        let (Some(f), Some(d)) = (self.file(owner.file), self.decl(owner)) else {
            out.outside = true;
            return;
        };
        if self.generated(owner.file) {
            out.outside = true;
        }
        let interface = matches!(d.kind, DeclKind::Interface | DeclKind::Annotation);
        let mut declared = self.declared(owner, name, member);
        // A field or member type declared here hides every one of that name further up this
        // branch, inherited or not (JLS 8.3, 8.5): javac's lookup stops at the first class that
        // declares the name and answers an access error when the member is not inherited, which
        // sends the search to the enclosing class (the slice-2 review's H1). Methods are not
        // hidden so: a non-inherited method of the same signature is a compile error.
        let hides = member != Member::Method && !declared.is_empty();
        if walk.inherited {
            declared.retain(|&h| {
                self.decl(h).is_some_and(|m| {
                    let private = m.modifiers.contains(Modifiers::PRIVATE);
                    let static_of_interface = interface
                        && m.kind == DeclKind::Method
                        && m.modifiers.contains(Modifiers::STATIC);
                    let package_private = !interface
                        && !m.modifiers.contains(Modifiers::PUBLIC)
                        && !m.modifiers.contains(Modifiers::PROTECTED)
                        && m.kind != DeclKind::EnumConstant;
                    !private && !static_of_interface && (!package_private || walk.package.is_some())
                })
            });
        }
        if member == Member::Method
            && declared.iter().any(|&h| {
                self.decl(h)
                    .is_some_and(|m| m.kind == DeclKind::Method && m.params.len == 0)
            })
        {
            // An explicit accessor stands for its record component.
            declared.retain(|&h| {
                self.decl(h)
                    .is_some_and(|m| m.kind != DeclKind::RecordComponent)
            });
        }
        for hit in declared {
            // Overridden only by a method of a proper subtype on this branch of the walk (JLS
            // 8.4.8.1): a same-signature method from another branch (a superclass's against an
            // interface's, or two interfaces') does not override it, and javac chooses between
            // the two by JLS 15.12.2.5, which the summary cannot prove, so both stay (the
            // slice-2 review's third pass).
            let overridden = walk.inherited
                && member == Member::Method
                && out.hits.iter().any(|&h| {
                    let owner = self
                        .decl(h)
                        .and_then(|d| d.parent)
                        .map(|decl| Hit { file: h.file, decl });
                    owner.is_some_and(|o| walk.below.contains(&o))
                        && self.same_parameters(h, hit, walk.depth)
                });
            if !overridden {
                out.hits.push(hit);
            }
        }
        if hides {
            return;
        }
        let supertypes: Vec<&str> = f.summary.list(d.supertypes).collect();
        // A type with no supertype clause still has implicit ones (Object, Enum, Record,
        // Annotation), and an enum, record or annotation has its implicit one beside any clause.
        if supertypes.is_empty()
            || matches!(
                d.kind,
                DeclKind::Enum | DeclKind::Record | DeclKind::Annotation
            )
        {
            out.outside |= implicit(d.kind, name, member);
        }
        let scope: Vec<DeclId> = self
            .enclosing_of(owner)
            .into_iter()
            .filter(|&id| id != owner.decl)
            .collect();
        let deeper = walk.depth.saturating_add(1);
        for written in supertypes {
            let up = match self.resolve_written(owner.file, &scope, written, deeper) {
                TypeBinding::Found(up) => up,
                TypeBinding::Unproved(hits, _) => {
                    // Not proved: its members are candidates, and more may come from elsewhere.
                    out.outside = true;
                    match hits.as_slice() {
                        [one] => *one,
                        _ => continue,
                    }
                }
                TypeBinding::Outside => {
                    out.outside = true;
                    continue;
                }
            };
            let package = self.package(up.file);
            let mut below = walk.below.clone();
            below.push(owner);
            let next = Walk {
                depth: deeper,
                inherited: true,
                package: walk.package.clone().filter(|p| p == package),
                below,
            };
            self.walk_members(up, name, member, &next, seen, out);
        }
    }

    /// Whether methods `a` and `b` are proved to take the same parameter types, so that one
    /// overrides the other: the types as written are equal, and each one is a primitive, or binds
    /// to the same view declaration from both files, or is the same import, or is written in two
    /// files of one package with the same on-demand imports and no single-type import of it.
    /// Anything less leaves both as overloads (`java.util.Date` against `java.sql.Date`).
    ///
    /// Pinned by: `tests/queries.rs::should_match_javac_or_stay_possible_when_the_review_s_probes_are_asked`
    /// (`def.probe.textual-override`).
    fn same_parameters(&self, a: Hit, b: Hit, depth: usize) -> bool {
        let (pa, pb) = (self.params(a), self.params(b));
        if pa != pb {
            return false;
        }
        let (Some(fa), Some(fb)) = (self.file(a.file), self.file(b.file)) else {
            return false;
        };
        let scope_a = self.enclosing_of(a);
        let scope_b = self.enclosing_of(b);
        pa.iter().all(|written| {
            let base = written.trim_end_matches("[]").trim_end_matches("...");
            let base = base.trim_end_matches("[]");
            if PRIMITIVES.contains(&base) {
                return true;
            }
            let ra = self.resolve_written(a.file, &scope_a, base, depth.saturating_add(1));
            let rb = self.resolve_written(b.file, &scope_b, base, depth.saturating_add(1));
            match (ra, rb) {
                (TypeBinding::Found(x), TypeBinding::Found(y)) => x == y,
                (TypeBinding::Outside, TypeBinding::Outside) => {
                    if base.contains('.') {
                        return true;
                    }
                    let single = |f: &FileEntry| {
                        f.summary
                            .imports()
                            .iter()
                            .find(|i| {
                                !i.on_demand
                                    && !i.module
                                    && f.summary.str(i.name).rsplit('.').next() == Some(base)
                            })
                            .map(|i| f.summary.str(i.name).to_owned())
                    };
                    let on_demand = |f: &FileEntry| -> Vec<String> {
                        let mut all: Vec<String> = f
                            .summary
                            .imports()
                            .iter()
                            .filter(|i| i.on_demand || i.module)
                            .map(|i| f.summary.str(i.name).to_owned())
                            .collect();
                        all.sort_unstable();
                        all
                    };
                    match (single(fa), single(fb)) {
                        (Some(x), Some(y)) => x == y,
                        (None, None) => {
                            fa.summary.package() == fb.summary.package()
                                && on_demand(fa) == on_demand(fb)
                        }
                        _ => false,
                    }
                }
                _ => false,
            }
        })
    }

    /// The declarations enclosing `hit`'s own, innermost first, `hit` included.
    fn enclosing_of(&self, hit: Hit) -> Vec<DeclId> {
        let mut chain = Vec::new();
        let mut next = Some(hit.decl);
        while let Some(id) = next {
            chain.push(id);
            next = self
                .decl(Hit {
                    file: hit.file,
                    decl: id,
                })
                .and_then(|d| d.parent);
            if chain.len() > MAX_DEPTH.saturating_mul(8) {
                break;
            }
        }
        chain
    }

    /// A type name as written in a supertype clause or a qualifier (`Base`, `java.io.Closeable`,
    /// `Map.Entry`), resolved from `scope` of `file`.
    fn resolve_written(
        &self,
        file: u32,
        scope: &[DeclId],
        written: &str,
        depth: usize,
    ) -> TypeBinding {
        if depth > MAX_DEPTH {
            return TypeBinding::Unproved(Vec::new(), Unproved::Supertype);
        }
        let Some((first, rest)) = written.split_once('.') else {
            return self.resolve_type(file, scope, &Scope::default(), written, depth);
        };
        // The first segment through the scope first (JLS 6.5.2): a type in scope there makes the
        // rest its member types; only a first segment that is no type can be a package.
        match self.resolve_type(file, scope, &Scope::default(), first, depth) {
            TypeBinding::Found(mut at) => {
                for segment in rest.split('.') {
                    let members = self.members_at(at, segment, Member::Type, depth);
                    match (members.hits.as_slice(), members.outside) {
                        ([one], false) => at = *one,
                        _ => return TypeBinding::Unproved(members.hits, Unproved::Supertype),
                    }
                }
                TypeBinding::Found(at)
            }
            TypeBinding::Unproved(_, why) => TypeBinding::Unproved(Vec::new(), why),
            TypeBinding::Outside => {
                let imported = self.file(file).is_some_and(|f| {
                    f.summary.imports().iter().any(|i| {
                        !i.is_static
                            && !i.on_demand
                            && !i.module
                            && f.summary.str(i.name).rsplit('.').next() == Some(first)
                    })
                });
                if imported {
                    // `Map.Entry` with `import java.util.Map`: a member of a type outside.
                    return TypeBinding::Outside;
                }
                // A package, or a type outside the index that an on-demand import or java.lang
                // offers: a view type of that qualified name is only a candidate.
                match self.types_qualified(written).as_slice() {
                    [] => TypeBinding::Outside,
                    many => TypeBinding::Unproved(many.to_vec(), Unproved::Import),
                }
            }
        }
    }

    /// The type a simple name binds to at a site of `file` inside `chain` (its enclosing
    /// declarations, innermost first), the scanner's `scope` around it, in Java's order: type
    /// variables and local classes, the enclosing types and their members, the file's types, its
    /// single-type imports, its package, its on-demand imports.
    pub(crate) fn resolve_type(
        &self,
        file: u32,
        chain: &[DeclId],
        scope: &Scope,
        name: &str,
        depth: usize,
    ) -> TypeBinding {
        let Some(f) = self.file(file) else {
            return TypeBinding::Unproved(Vec::new(), Unproved::Local);
        };
        if depth > MAX_DEPTH || !self.step() {
            return TypeBinding::Unproved(Vec::new(), Unproved::Supertype);
        }
        if scope.unknown || scope.local_types.iter().any(|t| t == name) {
            return TypeBinding::Unproved(Vec::new(), Unproved::Local);
        }
        if scope.anonymous {
            // An anonymous class around the site may inherit a member type of this name from a
            // supertype outside the index: whatever is found without it is only a candidate.
            let inner = Scope::default();
            let hits = match self.resolve_type(file, chain, &inner, name, depth) {
                TypeBinding::Found(h) => vec![h],
                TypeBinding::Unproved(h, _) => h,
                TypeBinding::Outside => Vec::new(),
            };
            return TypeBinding::Unproved(hits, Unproved::Supertype);
        }
        for (i, &id) in chain.iter().enumerate() {
            let hit = Hit { file, decl: id };
            let Some(d) = f.summary.declaration(id) else {
                continue;
            };
            let declared = f.summary.str(d.name);
            if scan::declares_type_parameter(f.summary.str(d.signature), declared, d.kind, name) {
                return TypeBinding::Unproved(Vec::new(), Unproved::Local);
            }
            if !d.kind.is_type() {
                continue;
            }
            let members = self.members_at(hit, name, Member::Type, depth);
            if members.own > 0 {
                // A declared member type hides every inherited one.
                return match members.hits.first() {
                    Some(own) if members.own == 1 => TypeBinding::Found(*own),
                    _ => TypeBinding::Unproved(members.hits, Unproved::Ambiguous),
                };
            }
            if members.outside {
                // A member type outside the index may shadow everything further out: what is
                // found further out is only a candidate.
                let rest = chain.get(i.saturating_add(1)..).unwrap_or(&[]);
                let mut all = members.hits;
                match self.resolve_type(file, rest, &Scope::default(), name, depth) {
                    TypeBinding::Found(h) => all.push(h),
                    TypeBinding::Unproved(h, _) => all.extend(h),
                    TypeBinding::Outside => {}
                }
                return TypeBinding::Unproved(all, Unproved::Supertype);
            }
            match members.hits.as_slice() {
                [] => {}
                [one] => return TypeBinding::Found(*one),
                many => return TypeBinding::Unproved(many.to_vec(), Unproved::Ambiguous),
            }
            if declared == name {
                return TypeBinding::Found(hit);
            }
        }
        // The file's own top-level types.
        if let Some(own) = self
            .types_named(name)
            .iter()
            .find(|h| h.file == file && self.decl(**h).is_some_and(|d| d.parent.is_none()))
        {
            return TypeBinding::Found(*own);
        }
        // Single-type imports, and a member type a single static import names.
        for import in f.summary.imports() {
            if import.on_demand || import.module {
                continue;
            }
            let imported = f.summary.str(import.name);
            if imported.rsplit('.').next() != Some(name) {
                continue;
            }
            if import.is_static {
                let Some((owner, _)) = imported.rsplit_once('.') else {
                    continue;
                };
                return match self.types_qualified(owner).as_slice() {
                    [one] => {
                        let members = self.members_at(*one, name, Member::Type, depth);
                        match (members.hits.as_slice(), members.outside) {
                            ([hit], false) => TypeBinding::Found(*hit),
                            _ => TypeBinding::Unproved(members.hits, Unproved::Supertype),
                        }
                    }
                    [] => TypeBinding::Outside,
                    many => TypeBinding::Unproved(many.to_vec(), Unproved::Ambiguous),
                };
            }
            return match self.types_qualified(imported).as_slice() {
                [one] => TypeBinding::Found(*one),
                [] => TypeBinding::Outside,
                many => TypeBinding::Unproved(many.to_vec(), Unproved::Ambiguous),
            };
        }
        // The package's top-level types.
        let package = f.summary.package();
        let same: Vec<Hit> = self
            .types_named(name)
            .iter()
            .copied()
            .filter(|&h| {
                self.file(h.file)
                    .is_some_and(|g| g.summary.package() == package)
                    && self.decl(h).is_some_and(|d| d.parent.is_none())
            })
            .collect();
        // Never exact before the project model (PROJ): the package may hold types the snapshot
        // lacks (a pending file, a gap, generated sources) that would be the ones meant, and a
        // view type found may be in a source set the file cannot see (a test type from main).
        if !same.is_empty() {
            return TypeBinding::Unproved(same, Unproved::SamePackage);
        }
        // On-demand imports (and `java.lang.*`, never in view): if the code compiles, at most one
        // offers the name.
        let mut offered = Vec::new();
        let mut modules = false;
        for import in f.summary.imports() {
            if import.module {
                modules = true;
                continue;
            }
            if !import.on_demand {
                continue;
            }
            let container = f.summary.str(import.name);
            offered.extend(self.types_qualified(&format!("{container}.{name}")));
        }
        offered.sort_unstable();
        offered.dedup();
        // Never exact either: a type of the file's own package beats an on-demand import, and
        // the package may hold one the snapshot lacks.
        match (offered.as_slice(), modules) {
            ([], false) => TypeBinding::Outside,
            (many, _) => TypeBinding::Unproved(many.to_vec(), Unproved::Import),
        }
    }

    /// Every member of the view named `name` that `member` accepts and, for a call's `arity`,
    /// that is applicable; at most `cap`, and whether more matched.
    pub(crate) fn every_member(
        &self,
        name: &str,
        member: Member,
        arity: Option<u8>,
        cap: usize,
    ) -> (Vec<Hit>, bool) {
        let mut hits = Vec::new();
        for &hit in self.names.get(name).map_or(&[][..], Vec::as_slice) {
            let Some(d) = self.decl(hit) else {
                continue;
            };
            if d.parent.is_none() || !member.accepts(d.kind) {
                continue;
            }
            if arity.is_some_and(|a| !self.applicable(hit, a)) {
                continue;
            }
            if hits.len() == cap {
                return (hits, true);
            }
            hits.push(hit);
        }
        (hits, false)
    }

    /// Whether a call passing `arity` arguments may call `hit`: as many parameters, or at least
    /// all but the last of a variable-arity method's.
    pub(crate) fn applicable(&self, hit: Hit, arity: u8) -> bool {
        let Some(d) = self.decl(hit) else {
            return false;
        };
        let params = if d.kind == DeclKind::RecordComponent {
            0
        } else {
            d.params.len
        };
        let arity = u32::from(arity);
        if d.modifiers.contains(Modifiers::VARARGS) {
            arity >= params.saturating_sub(1)
        } else {
            arity == params
        }
    }

    /// The overloads among `members` for a site passing `arity` arguments (`None`: a method
    /// reference, any arity): one applicable in a closed hierarchy is exact.
    fn overloads(&self, members: Members, arity: Option<u8>) -> Outcome {
        let applicable: Vec<Hit> = members
            .hits
            .iter()
            .copied()
            .filter(|&h| arity.is_none_or(|a| self.applicable(h, a)))
            .collect();
        match (applicable.as_slice(), members.outside) {
            ([one], false) => Outcome::Exact(*one),
            _ if members.hits.is_empty() => Outcome::Unknown,
            _ => Outcome::Overloads {
                all: members.hits,
                applicable,
                outside: members.outside,
            },
        }
    }

    /// The constructors of `owner` for a site passing `arity` arguments; a type that declares
    /// none is its own (implicit) constructor. Constructors are not inherited, so no supertype
    /// can add one.
    fn constructors(&self, owner: Hit, arity: Option<u8>) -> Outcome {
        let Some(f) = self.file(owner.file) else {
            return Outcome::Unknown;
        };
        let all: Vec<Hit> = self
            .children(owner)
            .filter(|h| {
                f.summary
                    .declaration(h.decl)
                    .is_some_and(|d| d.kind == DeclKind::Constructor)
            })
            .collect();
        let generated = self.generated(owner.file);
        if all.is_empty() {
            return if generated {
                Outcome::Unproved(vec![owner], Unproved::Supertype)
            } else {
                Outcome::Exact(owner)
            };
        }
        self.overloads(
            Members {
                hits: all,
                own: 0,
                outside: generated,
            },
            arity,
        )
    }

    /// What the name at `site` of `file` binds to; `text` is the file's text. At most `cap`
    /// candidates of an unknown receiver are listed.
    pub(crate) fn resolve_site(
        &self,
        file: u32,
        text: &str,
        site: &ReferenceSite,
        cap: usize,
    ) -> Outcome {
        let Some(f) = self.file(file) else {
            return Outcome::Unknown;
        };
        let name = f.summary.str(site.name);
        let span = (
            usize::try_from(site.range.start().get()).unwrap_or(usize::MAX),
            usize::try_from(site.range.end().get()).unwrap_or(usize::MAX),
        );
        if let Some(import) = f.summary.imports().iter().find(|i| {
            i.range.start().get() <= site.range.start().get()
                && site.range.end().get() <= i.range.end().get()
        }) {
            return self.resolve_import(f.summary.str(import.name), import.is_static, text, span);
        }
        let chain = self.enclosing(file, site.range.start().get());
        // The scanner reads the innermost member around the site (or its type, when no member
        // is around it).
        let region = chain
            .iter()
            .find(|&&id| f.summary.declaration(id).is_some_and(|d| !d.kind.is_type()))
            .or_else(|| chain.first())
            .and_then(|&id| f.summary.declaration(id))
            .map_or((0, text.len()), |d| {
                (
                    usize::try_from(d.range.start().get()).unwrap_or(0),
                    usize::try_from(d.range.end().get()).unwrap_or(usize::MAX),
                )
            });
        let mut scope = scan::scope(text, region.0, span.0);
        if chain.iter().any(|&id| {
            f.summary
                .declaration(id)
                .is_some_and(|d| d.kind == DeclKind::EnumConstant)
        }) {
            // An enum constant's body is an anonymous class (JLS 8.9.1): its members, which the
            // summary does not hold, come before every enclosing type's.
            scope.anonymous = true;
        }
        let at = At {
            file,
            scope,
            chain,
            text,
            region,
        };
        let arity = site.shape.has_arity().then_some(site.arity.0);
        match site.shape {
            RefShape::TypeUse | RefShape::Annotation => {
                binding_outcome(self.type_at(&at, span, name))
            }
            RefShape::New => match self.type_at(&at, span, name) {
                TypeBinding::Found(owner) => self.constructors(owner, arity),
                other => binding_outcome(other),
            },
            RefShape::MethodRef if scan::followed_by_method_ref(text, span.1) => {
                // `Type::new`: the site is the type; its constructors, any arity.
                match self.type_at(&at, span, name) {
                    TypeBinding::Found(owner) => self.constructors(owner, None),
                    other => binding_outcome(other),
                }
            }
            RefShape::Call => self.unqualified(&at, name, Member::Method, arity, cap),
            RefShape::Name => {
                if scan::is_element_key(text, span.0, span.1) || scan::is_case_label(text, span.0) {
                    // An annotation's element, or a constant of a switch selector's type: no
                    // field in scope, and syntax does not give the type.
                    return Outcome::Unknown;
                }
                let outcome = self.unqualified(&at, name, Member::Field, None, cap);
                if at.binds_locally(name) {
                    // A pattern variable (or another local the extractor kept) may be the one
                    // meant: what a field lookup finds is only a candidate.
                    return Outcome::Unproved(outcome_hits(outcome), Unproved::Local);
                }
                match outcome {
                    // No field of that name anywhere it could be: a type (or a package).
                    Outcome::Outside => binding_outcome(self.type_at(&at, span, name)),
                    other => other,
                }
            }
            RefShape::QualifiedCall | RefShape::QualifiedName | RefShape::MethodRef => {
                let member = if site.shape == RefShape::QualifiedName {
                    Member::Field
                } else {
                    Member::Method
                };
                let owner = match scan::before(text, span.0) {
                    Before::This if !at.scope.open() => at
                        .chain
                        .iter()
                        .find(|&&id| f.summary.declaration(id).is_some_and(|d| d.kind.is_type()))
                        .map(|&decl| Hit { file, decl }),
                    Before::QualifiedThis { start, end } => {
                        // `Outer.this`: the enclosing type the qualifier names (JLS 15.8.4).
                        let qualifier = text.get(start..end).unwrap_or("");
                        if at.scope.local_types.iter().any(|t| t == qualifier) {
                            None
                        } else {
                            at.chain
                                .iter()
                                .find(|&&id| {
                                    f.summary.declaration(id).is_some_and(|d| {
                                        d.kind.is_type() && f.summary.str(d.name) == qualifier
                                    })
                                })
                                .map(|&decl| Hit { file, decl })
                        }
                    }
                    Before::Ident { start, end } => self.type_receiver(&at, (start, end)),
                    _ => None,
                };
                let Some(owner) = owner else {
                    let (hits, capped) = self.every_member(name, member, arity, cap);
                    return Outcome::Receiver {
                        hits,
                        capped,
                        method_ref: site.shape == RefShape::MethodRef,
                    };
                };
                let members = self.members(owner, name, member);
                match member {
                    Member::Method => self.overloads(members, arity),
                    _ => field_outcome(members),
                }
            }
        }
    }

    /// A type name at `span`: simple, or qualified as written (`com.acme.Money`, `Map.Entry`).
    fn type_at(&self, at: &At<'_>, span: (usize, usize), name: &str) -> TypeBinding {
        if scan::before(at.text, span.0) == Before::Nothing {
            return self.resolve_type(at.file, &at.chain, &at.scope, name, 0);
        }
        match scan::dotted_ending_at(at.text, span.0, span.1) {
            Some(written) if written.contains('.') && !at.scope.open() => {
                self.resolve_written(at.file, &at.chain, &written, 0)
            }
            _ => TypeBinding::Unproved(Vec::new(), Unproved::Local),
        }
    }

    /// The type a receiver identifier at `span` names, when it can only be a type: the extractor
    /// kept it as a bare name (so it is no local or parameter), no field of that name can be in
    /// scope (Java takes a variable before a type), and the name binds to one type.
    fn type_receiver(&self, at: &At<'_>, span: (usize, usize)) -> Option<Hit> {
        let f = self.file(at.file)?;
        let start = u32::try_from(span.0).ok()?;
        let end = u32::try_from(span.1).ok()?;
        let site = f.summary.references().iter().find(|r| {
            r.range.start().get() == start
                && r.range.end().get() == end
                && r.shape == RefShape::Name
        })?;
        let name = f.summary.str(site.name);
        if at.binds_locally(name)
            || self.unqualified(at, name, Member::Field, None, 0) != Outcome::Outside
        {
            return None;
        }
        match self.resolve_type(at.file, &at.chain, &at.scope, name, 0) {
            TypeBinding::Found(hit) => Some(hit),
            _ => None,
        }
    }

    /// An unqualified method or field name: the innermost enclosing type with a member of that
    /// name is the owner, unless a type further in may hold one outside the index (or the site
    /// is in an anonymous class); then single static imports; then static on-demand imports.
    fn unqualified(
        &self,
        at: &At<'_>,
        name: &str,
        member: Member,
        arity: Option<u8>,
        cap: usize,
    ) -> Outcome {
        let Some(f) = self.file(at.file) else {
            return Outcome::Unknown;
        };
        let decide = |members: Members| match member {
            Member::Method => self.overloads(members, arity),
            _ => field_outcome(members),
        };
        let mut unsure = at.scope.open();
        let mut candidates = Vec::new();
        for &id in &at.chain {
            if !f.summary.declaration(id).is_some_and(|d| d.kind.is_type()) {
                continue;
            }
            let members = self.members(
                Hit {
                    file: at.file,
                    decl: id,
                },
                name,
                member,
            );
            if !members.hits.is_empty() && !unsure {
                return decide(members);
            }
            unsure |= members.outside;
            candidates.extend(members.hits);
        }
        for import in f.summary.imports() {
            if !import.is_static || import.on_demand {
                continue;
            }
            let imported = f.summary.str(import.name);
            let Some((owner, last)) = imported.rsplit_once('.') else {
                continue;
            };
            if last != name {
                continue;
            }
            let [one] = self.types_qualified(owner)[..] else {
                unsure = true;
                continue;
            };
            let members = self.members(one, name, member);
            if !members.hits.is_empty() && !unsure {
                return decide(members);
            }
            unsure |= members.outside;
            candidates.extend(members.hits);
        }
        for import in f.summary.imports() {
            if import.is_static && import.on_demand {
                unsure = true;
                for owner in self.types_qualified(f.summary.str(import.name)) {
                    candidates.extend(self.members(owner, name, member).hits);
                }
            }
        }
        candidates.sort_unstable();
        candidates.dedup();
        candidates.truncate(cap.max(1));
        match (candidates.is_empty(), unsure) {
            (true, false) => Outcome::Outside,
            (true, true) => Outcome::Unknown,
            (false, _) => Outcome::Unproved(candidates, Unproved::Supertype),
        }
    }

    /// A name inside an import: the type or member the import names up to that segment.
    fn resolve_import(
        &self,
        imported: &str,
        is_static: bool,
        text: &str,
        span: (usize, usize),
    ) -> Outcome {
        let Some(written) = scan::dotted_ending_at(text, span.0, span.1) else {
            return Outcome::Unknown;
        };
        let types = self.types_qualified(&written);
        match types.as_slice() {
            [one] => return Outcome::Exact(*one),
            [] => {}
            many => return Outcome::Unproved(many.to_vec(), Unproved::Ambiguous),
        }
        if is_static
            && written == imported
            && let Some((owner, name)) = written.rsplit_once('.')
            && let [one] = self.types_qualified(owner).as_slice()
        {
            let methods = self.members(*one, name, Member::Method);
            let fields = self.members(*one, name, Member::Field);
            let mut all = methods.hits.clone();
            all.extend(fields.hits.iter().copied());
            return match (all.as_slice(), methods.outside || fields.outside) {
                ([hit], false) => Outcome::Exact(*hit),
                ([], false) => Outcome::Outside,
                ([], true) => Outcome::Unknown,
                _ => Outcome::Overloads {
                    applicable: Vec::new(),
                    all,
                    outside: methods.outside || fields.outside,
                },
            };
        }
        if !is_static && written == imported {
            // The whole name of a type import no view type declares.
            return Outcome::Outside;
        }
        // A package: no declaration names it.
        Outcome::Unknown
    }
}

/// A site's file, the declarations around it (innermost first), what the scanner found around
/// it, and the file's text.
#[derive(Debug)]
struct At<'t> {
    file: u32,
    chain: Vec<DeclId>,
    scope: Scope,
    text: &'t str,
    /// The innermost member around the site (its bytes), or its type when no member is.
    region: (usize, usize),
}

impl At<'_> {
    /// Whether `name` may be a local declared in the site's member (`scan::may_bind_locally`).
    fn binds_locally(&self, name: &str) -> bool {
        scan::may_bind_locally(self.text, self.region.0, self.region.1, name)
    }
}

/// The declarations an outcome names, whatever it says of them.
fn outcome_hits(outcome: Outcome) -> Vec<Hit> {
    match outcome {
        Outcome::Exact(hit) => vec![hit],
        Outcome::Overloads { all, .. } => all,
        Outcome::Unproved(hits, _) | Outcome::Receiver { hits, .. } => hits,
        Outcome::Outside | Outcome::Unknown => Vec::new(),
    }
}

/// A field (or member type) lookup's outcome: the first declared hides the rest.
fn field_outcome(members: Members) -> Outcome {
    match (members.hits.as_slice(), members.own, members.outside) {
        ([one, ..], 1, _) | ([one], _, false) => Outcome::Exact(*one),
        ([], _, false) => Outcome::Outside,
        ([], _, true) => Outcome::Unknown,
        _ => Outcome::Unproved(members.hits, Unproved::Supertype),
    }
}

fn binding_outcome(binding: TypeBinding) -> Outcome {
    match binding {
        TypeBinding::Found(hit) => Outcome::Exact(hit),
        TypeBinding::Unproved(hits, why) => Outcome::Unproved(hits, why),
        TypeBinding::Outside => Outcome::Outside,
    }
}

/// Whether a type of `kind` with no supertype clause has members named `name` outside the index:
/// `Object`'s methods, `Enum`'s, a record's `Record` methods, an annotation's `annotationType`.
fn implicit(kind: DeclKind, name: &str, member: Member) -> bool {
    match member {
        Member::Method => {
            OBJECT_METHODS.contains(&name)
                || (kind == DeclKind::Enum && ENUM_METHODS.contains(&name))
                || (kind == DeclKind::Annotation && name == "annotationType")
        }
        Member::Type => kind == DeclKind::Enum && ENUM_TYPES.contains(&name),
        Member::Field => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jfl_domain::{ByteOffset, DeclarationInput, Digest, SummaryBuilder, SyntaxInputs};

    struct Zero;

    impl jfl_domain::KeyHasher for Zero {
        fn update(&mut self, _: &[u8]) {}

        fn finish(self) -> Digest {
            Digest::from_bytes([0; 32])
        }
    }

    fn range(start: u32, end: u32) -> ByteRange {
        ByteRange::new(ByteOffset::new(start), ByteOffset::new(end)).unwrap()
    }

    /// One file of package `p` declaring each `(name, supertypes)` as a top-level class.
    fn snapshot(classes: &[(&str, &[&str])]) -> Snapshot {
        let mut builder = SummaryBuilder::new();
        builder.package("p").unwrap();
        for (i, (name, supertypes)) in (0_u32..).zip(classes) {
            builder
                .declaration(&DeclarationInput {
                    kind: DeclKind::Class,
                    modifiers: Modifiers::empty(),
                    parent: None,
                    name,
                    name_range: range(i.saturating_mul(10), i.saturating_mul(10).saturating_add(1)),
                    range: range(i.saturating_mul(10), i.saturating_mul(10).saturating_add(9)),
                    signature: name,
                    params: &[],
                    supertypes,
                    permits: &[],
                    annotations: &[],
                    javadoc: None,
                })
                .unwrap();
        }
        let key = SyntaxKey::derive(
            &SyntaxInputs {
                file: Digest::from_bytes([0; 32]),
                grammar: "g",
                grammar_version: "1",
                summariser_version: 1,
            },
            Zero,
        );
        Snapshot::new(vec![FileEntry {
            path: RelPath::new("p/A.java").unwrap(),
            key,
            summary: Arc::new(builder.finish()),
        }])
    }

    #[test]
    fn should_end_and_call_the_members_outside_when_supertypes_form_a_cycle() {
        // given: A extends B, B extends A (refused by javac, possible in a file being edited)
        let snapshot = snapshot(&[("A", &["B"]), ("B", &["A"])]);
        let resolver = Resolver::new(&snapshot);
        let a = Hit {
            file: 0,
            decl: DeclId(0),
        };
        // when
        let members = resolver.members(a, "m", Member::Method);
        // then
        assert!(members.hits.is_empty());
        assert!(members.outside);
    }

    #[test]
    fn should_stop_at_the_budget_when_a_hierarchy_is_hostile() {
        // given: a resolution whose budget is spent
        let snapshot = snapshot(&[("A", &[]), ("B", &["A"])]);
        let resolver = Resolver::new(&snapshot);
        resolver.steps.store(0, Ordering::Relaxed);
        let b = Hit {
            file: 0,
            decl: DeclId(1),
        };
        // when
        let members = resolver.members(b, "x", Member::Field);
        let binding = resolver.resolve_type(0, &[], &Scope::default(), "A", 0);
        // then: every further lookup answers "outside the index", never a binding
        assert!(members.outside);
        assert_eq!(
            binding,
            TypeBinding::Unproved(Vec::new(), Unproved::Supertype)
        );
        // and with a budget the same lookups resolve
        let fresh = Resolver::new(&snapshot);
        assert!(!fresh.members(b, "x", Member::Field).outside);
        assert_eq!(
            fresh.resolve_type(0, &[], &Scope::default(), "A", 0),
            TypeBinding::Found(Hit {
                file: 0,
                decl: DeclId(0)
            })
        );
    }

    /// A deadline that passes at its `after`-th reading, counting them.
    struct Readings {
        after: usize,
        read: std::sync::atomic::AtomicUsize,
    }

    impl Deadline for Readings {
        fn passed(&self) -> bool {
            self.read.fetch_add(1, Ordering::Relaxed).saturating_add(1) >= self.after
        }
    }

    #[test]
    fn should_stop_at_the_deadline_when_a_file_holds_a_hostile_hierarchy() {
        // given: 3,000 classes in one file, each extending the next four (a lookup could walk them
        // all), and a deadline that passes at its first reading
        let names: Vec<String> = (0..3_000).map(|i| format!("C{i}")).collect();
        let supers: Vec<Vec<&str>> = (0..3_000_usize)
            .map(|i| {
                (1..=4)
                    .filter_map(|k| names.get(i.saturating_add(k)).map(String::as_str))
                    .collect()
            })
            .collect();
        let classes: Vec<(&str, &[&str])> = names
            .iter()
            .zip(&supers)
            .map(|(n, s)| (n.as_str(), s.as_slice()))
            .collect();
        let snapshot = snapshot(&classes);
        let deadline = Readings {
            after: 1,
            read: std::sync::atomic::AtomicUsize::new(0),
        };
        let resolver = Resolver::with_deadline(&snapshot, Some(&deadline));
        let first = Hit {
            file: 0,
            decl: DeclId(0),
        };
        // when
        let members = resolver.members(first, "m", Member::Method);
        // then: it stopped at that reading, DEADLINE_EVERY lookups in, and says so
        assert!(resolver.cut());
        assert!(members.outside);
        assert_eq!(deadline.read.load(Ordering::Relaxed), 1);
        assert_eq!(
            STEP_BUDGET.saturating_sub(resolver.steps.load(Ordering::Relaxed)),
            STEP_BUDGET
        );
    }

    /// A file at `path` of `package` (`None`: the default package) with these single-type
    /// imports, declaring each class; `Outer$Inner` declares `Inner` inside `Outer`.
    fn file(path: &str, package: Option<&str>, imports: &[&str], classes: &[&str]) -> FileEntry {
        let mut builder = SummaryBuilder::new();
        if let Some(package) = package {
            builder.package(package).unwrap();
        }
        for import in imports {
            builder.import(import, false, false, range(0, 1)).unwrap();
        }
        let mut ids: Vec<(&str, DeclId)> = Vec::new();
        for (i, class) in (0_u32..).zip(classes) {
            let (parent, name) = match class.split_once('$') {
                Some((outer, inner)) => (
                    ids.iter().find(|(n, _)| *n == outer).map(|(_, id)| *id),
                    inner,
                ),
                None => (None, *class),
            };
            let at = i.saturating_mul(10).saturating_add(100);
            let id = builder
                .declaration(&DeclarationInput {
                    kind: DeclKind::Class,
                    modifiers: Modifiers::PUBLIC.with(Modifiers::STATIC),
                    parent,
                    name,
                    name_range: range(at, at.saturating_add(1)),
                    range: range(at, at.saturating_add(9)),
                    signature: name,
                    params: &[],
                    supertypes: &[],
                    permits: &[],
                    annotations: &[],
                    javadoc: None,
                })
                .unwrap();
            ids.push((name, id));
        }
        let key = SyntaxKey::derive(
            &SyntaxInputs {
                file: Digest::from_bytes([0; 32]),
                grammar: "g",
                grammar_version: "1",
                summariser_version: 1,
            },
            Zero,
        );
        FileEntry {
            path: RelPath::new(path).unwrap(),
            key,
            summary: Arc::new(builder.finish()),
        }
    }

    #[test]
    fn should_never_offer_a_default_package_type_when_a_name_is_qualified() {
        // given: a default-package Map with Entry, and a file of package q importing nothing
        let snapshot = Snapshot::new(vec![
            file("Map.java", None, &[], &["Map", "Map$Entry"]),
            file("q/User.java", Some("q"), &[], &["User"]),
        ]);
        let resolver = Resolver::new(&snapshot);
        // when
        let binding = resolver.resolve_written(1, &[], "Map.Entry", 0);
        // then
        assert_eq!(binding, TypeBinding::Outside);
    }

    #[test]
    fn should_bind_outside_when_the_first_segment_is_an_imported_type() {
        // given: a view type `Map.Entry` (package Map), and a file that imports java.util.Map
        let snapshot = Snapshot::new(vec![
            file("Map/Entry.java", Some("Map"), &[], &["Entry"]),
            file("q/User.java", Some("q"), &["java.util.Map"], &["User"]),
        ]);
        let resolver = Resolver::new(&snapshot);
        // when
        let binding = resolver.resolve_written(1, &[], "Map.Entry", 0);
        // then: java.util.Map.Entry, outside the index, never the view's type
        assert_eq!(binding, TypeBinding::Outside);
    }

    #[test]
    fn should_count_members_outside_when_a_file_uses_lombok() {
        // given: two classes declaring no member `log`, one in a file that imports Lombok
        let snapshot = Snapshot::new(vec![
            file("p/Plain.java", Some("p"), &[], &["Plain"]),
            file(
                "p/Logged.java",
                Some("p"),
                &["lombok.extern.slf4j.Slf4j"],
                &["Logged"],
            ),
        ]);
        let resolver = Resolver::new(&snapshot);
        let plain = Hit {
            file: 0,
            decl: DeclId(0),
        };
        let logged = Hit {
            file: 1,
            decl: DeclId(0),
        };
        // when
        let fields = [
            resolver.members(plain, "log", Member::Field).outside,
            resolver.members(logged, "log", Member::Field).outside,
        ];
        let constructors = resolver.constructors(logged, Some(0));
        // then: `@Slf4j` may add `log`, `@AllArgsConstructor` a constructor
        assert_eq!(fields, [false, true]);
        assert_eq!(
            constructors,
            Outcome::Unproved(vec![logged], Unproved::Supertype)
        );
    }

    #[test]
    fn should_see_a_processor_when_a_file_imports_or_names_one() {
        // given: an import of a known processor's package, one of a package that only starts
        // like one, and an on-demand import of a known package
        let files = [
            file("p/A.java", Some("p"), &["org.mapstruct.Mapper"], &["A"]),
            file("p/B.java", Some("p"), &["lombokish.Thing"], &["B"]),
            file("p/C.java", Some("p"), &["jakarta.persistence"], &["C"]),
            file("p/D.java", Some("p"), &[], &["D"]),
        ];
        // when
        let seen: Vec<bool> = files.iter().map(|f| uses_processor(&f.summary)).collect();
        let snapshot = Snapshot::new(files.to_vec());
        // then
        assert_eq!(seen, [true, false, true, false]);
        assert_eq!(snapshot.processor_files(), 2);
    }

    #[test]
    fn should_say_declared_twice_when_two_files_declare_one_qualified_name() {
        // given: `p.A` in two files (two source sets), `p.B` once, with a member type
        let snapshot = Snapshot::new(vec![
            file("main/p/A.java", Some("p"), &[], &["A"]),
            file("main/p/B.java", Some("p"), &[], &["B", "B$In"]),
            file("test/p/A.java", Some("p"), &[], &["A"]),
        ]);
        let hit = |file, decl| Hit {
            file,
            decl: DeclId(decl),
        };
        // when
        let twice = [
            snapshot.declared_twice(hit(0, 0)),
            snapshot.declared_twice(hit(1, 0)),
            snapshot.declared_twice(hit(1, 1)),
            snapshot.declared_twice(hit(2, 0)),
        ];
        // then
        assert_eq!(twice, [true, false, false, true]);
    }
}
