//! `jfl refs` (IDX-11; plan §2.1, §2.2; ADR 0014): every place the target of a position is used,
//! over the pinned generation.
//!
//! **`possible` is a superset by name, never by proof.** The candidates are every reference site
//! of the view (the request's whole worktree before M3, plan §2.1) whose simple name is the
//! target's, read from each indexed summary's own sites (never from a name list: IDX-08's
//! store keeps one beside declarations-only summaries for the large tier, and it is not checked
//! against the sites, so nothing here reads it). Comments and string literals hold no site (the
//! extractor's rule), so they never appear. Only what Java rules out with no name resolution is
//! left out, each rule one line in [`compatible`]:
//! - a site whose shape cannot use the target's kind (a call of a type or a field, `new` of a
//!   method or a field, a type use or an annotation of a method);
//! - a call or creation whose argument count no parameter list of the target accepts (JLS
//!   15.12.2: no default arguments; a variable-arity method takes all but its last or more);
//! - any site outside the file of a `private` target (JLS 6.6.1: only within the top-level type
//!   around the declaration, which is in that file), and outside the package of a
//!   package-private one (no access modifier written or implied, [`package_private`]).
//!
//! A type is also named by the segments of an import before its last (`import a.Outer.Inner;`,
//! `import static a.Type.member;`), which the extractor keeps no site for: such an import of the
//! name is a candidate, its segments found in its text (javac binds them; the differential found
//! the gap).
//!
//! A site the syntax-tree resolver proves bound to the target (`resolve::prove`, the allowlist of
//! IDX-10, checked against javac) is `exact`; every other candidate is `possible` with its reason,
//! including a site proved bound to another declaration: the superset does not lean on the proofs.
//! A proof cut short by the deadline only leaves the site `possible`, so it costs precision, never
//! completeness.
//!
//! **`complete`** (plan §2.1) starts from the pin (every file of the worktree indexed and fresh,
//! no pending change, a watched view) and is withdrawn, each with its reason, when:
//! - the target is not one declaration (`TargetUnresolved`: the cursor's binding is not proved;
//!   every site of the name is then `possible`);
//! - the target may be used without its name (`ImplicitUses`, [`implicit`]): a method a type
//!   outside the index may declare too (a library supertype's, `Object`'s: implicit calls,
//!   callers outside the view), a record component or its accessor (record patterns), a
//!   constructor a zero-argument call reaches (a subclass's implicit `super()`) or of an enum
//!   (its constants), an annotation's `value` element;
//! - a known annotation processor is used in the view (`GeneratedSourcesUnknown`): what it
//!   generates is not indexed before the project model names the roots (the card's Decided rule);
//! - the target's top-level type is declared twice in the view (`TargetDeclaredTwice`): source
//!   sets are not told apart, so its `exact` sites are left `possible` too;
//! - the scan or the reads passed the deadline (`deadline`), a candidate's file moved on since the
//!   pin (`ModuleStale`), more sites matched than [`QueryConfig::references_limit`]
//!   (`ResultsCapped`), or `possible` was left out on request (`PossibleLeftOut`).
//!
//! A file of the view that had a syntax error when summarised (`FileSummary::syntax_errors`,
//! kept in the store too) withdraws it as well (`SyntaxErrors`): the parser's error recovery may
//! have lost its sites.
//!
//! Pinned by: `jfl-adapter-java`'s `tests/definitions/references.rs` (javac's bindings of the
//! fixture projects within `exact ∪ possible`, `exact` within them; and the completeness oracle of
//! ver §7.2) and `tests/queries.rs` (each reason, the goldens).

use std::collections::BTreeMap;
use std::sync::Arc;

use jfl_domain::{
    Arity, Completeness, DeclKind, Declaration, Envelope, EnvelopeParts, IncompleteReason,
    Location, Modifiers, ModuleName, Position, PositionEncoding, PossibleHit, Reason, RefShape,
    ReferenceSite, Target, TargetStatus, Why,
};

use super::resolve::{Cursor, Hit, Member, Outcome, Resolver, Snapshot};
use super::scan;
use super::{
    Bind, Bound, Deadline, FileText, Pinned, QueryCache, QueryConfig, QueryError, READS_IN_FLIGHT,
    Sources, bind, convert, provable,
};
use crate::view_service;

/// What a references query asks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReferencesQuery {
    /// Where: the declaration's name, or a use of it.
    pub position: Position,
    /// Leave `possible` out (`jfl refs --exact-only`); the answer then says how many were left
    /// out (`PossibleLeftOut`), since `exact` alone holds every reference only by chance.
    pub exact_only: bool,
    /// List the target's own declaration in `exact` too (LSP's `includeDeclaration`).
    pub include_declaration: bool,
}

/// A declaration's reference summary for hover (plan §2.2): `12 exact, 3 possible, complete`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReferenceCounts {
    /// References bound with evidence.
    pub exact: u32,
    /// Other candidates.
    pub possible: u32,
    /// Whether `exact ∪ possible` holds every reference.
    pub complete: bool,
}

impl ReferenceCounts {
    /// The counts of a references answer.
    #[must_use]
    pub fn of(envelope: &Envelope) -> Self {
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        Self {
            exact: count(envelope.exact().len()),
            possible: count(envelope.possible().len()),
            complete: envelope.complete(),
        }
    }
}

impl core::fmt::Display for ReferenceCounts {
    /// `12 exact, 3 possible, complete` or `… incomplete`.
    ///
    /// Pinned by: `tests/queries.rs::should_count_a_declaration_s_references_when_hovering_its_name`.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{} exact, {} possible, {}",
            self.exact,
            self.possible,
            if self.complete {
                "complete"
            } else {
                "incomplete"
            }
        )
    }
}

/// `jfl refs` (plan §2.2): the references of what the name at `position` of the request's file
/// binds to (module documentation).
///
/// Pinned by `tests/queries.rs` and `jfl-adapter-java`'s `tests/definitions/references.rs`.
pub async fn references<S: Sources, D: Deadline>(
    pinned: &Pinned<'_>,
    sources: &S,
    query: ReferencesQuery,
    encoding: PositionEncoding,
    config: &QueryConfig,
    deadline: &D,
    cache: &QueryCache,
) -> Result<Envelope, QueryError> {
    let position = query.position;
    match bind(pinned, sources, position, encoding, config, deadline, cache).await? {
        Bind::Bound(bound) => {
            let asked = Asked {
                query,
                counts_only: false,
            };
            Ok(
                collect(pinned, sources, encoding, config, asked, deadline, &bound)
                    .await
                    .envelope,
            )
        }
        Bind::Early(completeness, _) => Ok(Envelope::new(parts(pinned, encoding, completeness))),
    }
}

fn parts(
    pinned: &Pinned<'_>,
    encoding: PositionEncoding,
    completeness: Completeness,
) -> EnvelopeParts {
    EnvelopeParts {
        view: pinned.view(),
        target: None,
        exact: Vec::new(),
        possible: Vec::new(),
        completeness,
        freshness: pinned.freshness,
        position_encoding: encoding,
    }
}

/// How often the scan reads the deadline, in files.
const SCAN_DEADLINE_EVERY: usize = 256;

/// One candidate: its file in the snapshot and the site.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    file: u32,
    site: ReferenceSite,
    /// An import naming the target in a segment before its last (`import a.Outer.Inner;`,
    /// `import static a.Type.member;`): the extractor keeps only an import's last segment as a
    /// site, so `site.range` is the whole import and the segments are found in its text.
    import: bool,
}

/// What one candidate file gave: its text (`None`: moved on since the pin, or not read before
/// the deadline, `cut`), and per candidate of it (in order) the declaration the resolver proved.
struct Read {
    file: u32,
    text: Option<Arc<FileText>>,
    cut: bool,
    proofs: Vec<Option<Hit>>,
}

/// The references of `bound`'s target (module documentation).
pub(super) async fn collect<S: Sources, D: Deadline>(
    pinned: &Pinned<'_>,
    sources: &S,
    encoding: PositionEncoding,
    config: &QueryConfig,
    asked: Asked,
    deadline: &D,
    bound: &Bound,
) -> Collected {
    let Asked { query, counts_only } = asked;
    let snapshot = &*bound.snapshot;
    let mut completeness = super::syntax_errors(pinned.answer.completeness.clone(), snapshot);
    // The target: the declaration under the cursor, or what a use is proved bound to.
    let target: Option<Hit> = match (bound.cursor, &bound.outcome) {
        (Cursor::Declaration(decl), _) => Some(Hit {
            file: bound.request_file,
            decl,
        }),
        (Cursor::Site(_), Outcome::Exact(hit)) => Some(*hit),
        (Cursor::Site(_), _) => None,
    };
    let name: String = match (target, bound.cursor) {
        (Some(hit), _) => match (snapshot.file(hit.file), snapshot.decl(hit)) {
            (Some(f), Some(d)) => f.summary.str(d.name).to_owned(),
            _ => {
                return Collected {
                    envelope: Envelope::new(parts(
                        pinned,
                        encoding,
                        completeness.and(IncompleteReason::TargetUnresolved),
                    )),
                    unread: 0,
                };
            }
        },
        (None, Cursor::Site(site)) => snapshot
            .file(bound.request_file)
            .map(|f| f.summary.str(site.name).to_owned())
            .unwrap_or_default(),
        (None, Cursor::Declaration(_)) => String::new(),
    };
    let declaration: Option<&Declaration> = target.and_then(|hit| snapshot.decl(hit));
    if target.is_none() {
        completeness = completeness.and(IncompleteReason::TargetUnresolved);
    }
    let resolver = Resolver::with_deadline(snapshot, None);
    // The scan: every site of the name the target's kind and parameters allow.
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut capped = false;
    let mut cut = false;
    let only_file = match (target, declaration) {
        (Some(hit), Some(d)) if d.modifiers.contains(Modifiers::PRIVATE) => Some(hit.file),
        _ => None,
    };
    let only_package: Option<&str> = match (target, declaration) {
        (Some(hit), Some(d)) if package_private(snapshot, hit, d) => Some(
            snapshot
                .file(hit.file)
                .and_then(|f| f.summary.package())
                .unwrap_or(""),
        ),
        _ => None,
    };
    'files: for (at, f) in snapshot.files().iter().enumerate() {
        if at % SCAN_DEADLINE_EVERY == SCAN_DEADLINE_EVERY - 1 && deadline.passed() {
            cut = true;
            break;
        }
        let Ok(file) = u32::try_from(at) else {
            break;
        };
        if only_file.is_some_and(|only| only != file)
            || only_package.is_some_and(|only| f.summary.package().unwrap_or("") != only)
        {
            continue;
        }
        for site in f.summary.references() {
            if f.summary.str(site.name) != name {
                continue;
            }
            if let (Some(hit), Some(d)) = (target, declaration)
                && !compatible(&resolver, hit, d, site)
            {
                continue;
            }
            if candidates.len() == config.references_limit {
                capped = true;
                break 'files;
            }
            candidates.push(Candidate {
                file,
                site: *site,
                import: false,
            });
        }
        // A type is also named by the qualifiers of imports (javac binds them).
        if declaration.is_none_or(|d| d.kind.is_type()) {
            for import in f.summary.imports() {
                let dotted = f.summary.str(import.name);
                if import.module || !dotted.split('.').any(|segment| segment == name) {
                    continue;
                }
                if candidates.len() == config.references_limit {
                    capped = true;
                    break 'files;
                }
                candidates.push(Candidate {
                    file,
                    site: ReferenceSite {
                        name: import.name,
                        range: import.range,
                        shape: RefShape::QualifiedName,
                        arity: Arity(0),
                    },
                    import: true,
                });
            }
        }
    }
    // Per file: its text (the request's own already read), and the proofs of its provable sites.
    // Counting for hover reads only the request's own file: the others' candidates are counted
    // as `possible` unread.
    let read_here = |file: u32| !counts_only || file == bound.request_file;
    let unread = candidates.iter().filter(|c| !read_here(c.file)).count();
    candidates.retain(|c| read_here(c.file));
    let mut by_file: BTreeMap<u32, Vec<ReferenceSite>> = BTreeMap::new();
    for c in &candidates {
        let sites = by_file.entry(c.file).or_default();
        if !c.import {
            sites.push(c.site);
        }
    }
    if let Some(hit) = target
        && read_here(hit.file)
    {
        by_file.entry(hit.file).or_default();
    }
    let root = &pinned.answer.view;
    let job = Job {
        root,
        snapshot,
        sources,
        deadline,
        bound,
        target,
    };
    let reads = view_service::bounded(READS_IN_FLIGHT, by_file, |(file, sites)| {
        job.read(file, sites)
    })
    .await;
    let mut texts: BTreeMap<u32, Arc<FileText>> = BTreeMap::new();
    let mut proofs: BTreeMap<u32, Vec<Option<Hit>>> = BTreeMap::new();
    let mut stale = 0_u32;
    for r in reads {
        match r.text {
            Some(text) => {
                texts.insert(r.file, text);
            }
            None if r.cut => cut = true,
            None => stale = stale.saturating_add(1),
        }
        proofs.insert(r.file, r.proofs);
    }
    let twice = target.is_some_and(|hit| snapshot.declared_twice(hit));
    // The candidates, located: exact when proved bound to the target, else possible.
    let mut exact: Vec<Location> = Vec::new();
    let mut possible: Vec<PossibleHit> = Vec::new();
    let mut seen: BTreeMap<u32, usize> = BTreeMap::new();
    let mut unlocated = false;
    for c in candidates.iter().filter(|c| c.import) {
        let Some(text) = texts.get(&c.file) else {
            continue;
        };
        let segments = scan::identifiers(&text.text, c.site.range, &name);
        // An import spelled with Unicode escapes is not found by its text: say so.
        unlocated |= segments.is_empty();
        for range in segments {
            if let Some(location) = locate(snapshot, root, &texts, c.file, range, encoding) {
                possible.push(PossibleHit {
                    location,
                    why: Why::with_detail(Reason::NameAndArity, "an import's qualifier"),
                });
            }
        }
    }
    for c in candidates.iter().filter(|c| !c.import) {
        let index = seen.entry(c.file).or_insert(0);
        let proof = proofs
            .get(&c.file)
            .and_then(|p| p.get(*index))
            .copied()
            .flatten();
        *index = index.saturating_add(1);
        let Some(location) = locate(snapshot, root, &texts, c.file, c.site.range, encoding) else {
            continue;
        };
        match (target, proof) {
            (Some(hit), Some(proved)) if proved == hit && !twice => exact.push(location),
            (Some(_), Some(_)) if !twice => possible.push(PossibleHit {
                location,
                why: Why::with_detail(
                    Reason::NameAndArity,
                    "the syntax-tree resolver binds it to another declaration",
                ),
            }),
            _ => {
                let this = texts.get(&c.file).is_some_and(|t| {
                    usize::try_from(c.site.range.start().get())
                        .is_ok_and(|at| scan::before(&t.text, at) == scan::Before::This)
                });
                possible.push(PossibleHit {
                    location,
                    why: why(&c.site, target.is_some(), this),
                });
            }
        }
    }
    let mut named: Option<Target> = None;
    if let (Some(hit), Some(d)) = (target, declaration) {
        let at = locate(snapshot, root, &texts, hit.file, d.name_range, encoding);
        if let (Some(at), Ok(symbol)) = (
            at,
            snapshot
                .file(hit.file)
                .map_or(Err(()), |f| f.summary.symbol_id(hit.decl).map_err(|_| ())),
        ) {
            named = Some(Target {
                symbol,
                status: TargetStatus::Exact,
            });
            if query.include_declaration {
                exact.push(at);
            }
        }
        if implicit(&resolver, hit, d) {
            completeness = completeness.and(IncompleteReason::ImplicitUses);
        }
        if twice {
            completeness = completeness.and(IncompleteReason::TargetDeclaredTwice);
        }
    }
    if unlocated {
        completeness = completeness.and(IncompleteReason::TargetUnresolved);
    }
    if snapshot.processor_files() > 0 {
        completeness = completeness.and(IncompleteReason::GeneratedSourcesUnknown {
            files: snapshot.processor_files(),
        });
    }
    if cut {
        completeness = completeness.deadline();
    }
    if stale > 0 {
        completeness = completeness.and(IncompleteReason::ModuleStale {
            module: ModuleName::worktree(),
            stale,
        });
    }
    if capped {
        completeness = completeness.and(IncompleteReason::ResultsCapped {
            limit: u32::try_from(config.references_limit).unwrap_or(u32::MAX),
        });
    }
    if query.exact_only && !possible.is_empty() {
        completeness = completeness.and(IncompleteReason::PossibleLeftOut {
            count: u32::try_from(possible.len()).unwrap_or(u32::MAX),
        });
        possible.clear();
    }
    Collected {
        envelope: Envelope::new(EnvelopeParts {
            target: named,
            exact,
            possible,
            ..parts(pinned, encoding, completeness)
        }),
        unread: u32::try_from(unread).unwrap_or(u32::MAX),
    }
}

/// What [`collect`] is asked: the query, and whether only counts are wanted (hover), so that
/// only the request's own file is read and proved.
#[derive(Debug, Clone, Copy)]
pub(super) struct Asked {
    pub(super) query: ReferencesQuery,
    pub(super) counts_only: bool,
}

/// What [`collect`] found: the answer, and how many candidates it counted without reading their
/// files (counts only).
pub(super) struct Collected {
    pub(super) envelope: Envelope,
    pub(super) unread: u32,
}

impl ReferenceCounts {
    /// The counts of what [`collect`] found.
    pub(super) fn of_collected(collected: &Collected) -> Self {
        let counts = Self::of(&collected.envelope);
        Self {
            possible: counts.possible.saturating_add(collected.unread),
            ..counts
        }
    }
}

/// What every candidate file's job shares.
struct Job<'a, S, D> {
    root: &'a jfl_domain::FileUri,
    snapshot: &'a Snapshot,
    sources: &'a S,
    deadline: &'a D,
    bound: &'a Bound,
    target: Option<Hit>,
}

impl<S: Sources, D: Deadline> Job<'_, S, D> {
    /// Reads one candidate file's text and proves its provable sites against the target (one parse
    /// for all of them); the request's own file is already read.
    async fn read(&self, file: u32, sites: Vec<ReferenceSite>) -> Read {
        let Self {
            root,
            snapshot,
            sources,
            deadline,
            bound,
            target,
        } = *self;
        let none = vec![None; sites.len()];
        if deadline.passed() {
            return Read {
                file,
                text: None,
                cut: true,
                proofs: none,
            };
        }
        let text = if file == bound.request_file {
            Some(Arc::clone(&bound.own))
        } else {
            match snapshot.file(file) {
                Some(f) => sources.text(root, &f.path, &f.key).await,
                None => None,
            }
        };
        let Some(text) = text else {
            return Read {
                file,
                text: None,
                cut: false,
                proofs: none,
            };
        };
        // Only a site the allowlist could prove is parsed for, and only while the deadline allows.
        let provable: Vec<usize> = match target {
            Some(_) if !deadline.passed() => sites
                .iter()
                .enumerate()
                .filter(|(_, s)| provable(s, &text.text))
                .map(|(i, _)| i)
                .collect(),
            _ => Vec::new(),
        };
        let mut proofs = none;
        if !provable.is_empty() {
            let ranges = provable
                .iter()
                .filter_map(|&i| sites.get(i).map(|s| s.range))
                .collect();
            let facts = sources.sites_syntax(&text, ranges).await;
            for (&i, syntax) in provable.iter().zip(facts) {
                let (Some(syntax), Some(site), Some(slot)) =
                    (syntax, sites.get(i), proofs.get_mut(i))
                else {
                    continue;
                };
                // Each proof has its own lookup budget; a cut one proves nothing.
                let resolver = Resolver::with_deadline(snapshot, Some(deadline));
                let proved = resolver.prove(file, site, &syntax);
                if !resolver.cut() {
                    *slot = proved;
                }
            }
        }
        Read {
            file,
            text: Some(text),
            cut: false,
            proofs,
        }
    }
}

/// The location of `range` of `file`, when its text was read.
fn locate(
    snapshot: &Snapshot,
    root: &jfl_domain::FileUri,
    texts: &BTreeMap<u32, Arc<FileText>>,
    file: u32,
    range: jfl_domain::ByteRange,
    encoding: PositionEncoding,
) -> Option<Location> {
    let f = snapshot.file(file)?;
    let text = texts.get(&file)?;
    Some(Location {
        uri: root.join(&f.path).ok()?,
        range: convert(text, range, encoding)?,
    })
}

/// Why a candidate is only `possible`; `this`: its receiver is `this`.
fn why(site: &ReferenceSite, resolved: bool, this: bool) -> Why {
    match (site.shape, resolved) {
        (RefShape::MethodRef, _) => Why::new(Reason::MethodReference),
        (_, false) => Why::with_detail(Reason::NameAndArity, "the target is not resolved"),
        (RefShape::QualifiedCall | RefShape::QualifiedName, true) if !this => {
            Why::new(Reason::ReceiverTypeUnknown)
        }
        (_, true) => Why::with_detail(Reason::NameAndArity, "not proved"),
    }
}

/// Whether `d` (`hit`) is accessible only from its own package (JLS 6.6.1): no access modifier,
/// and none implied: not a member of an interface or an annotation type (public), not an enum
/// constant, a record component (its accessor is public) or an annotation element. Its uses are
/// then all in files of that package.
///
/// Pinned by: `tests/queries.rs::should_keep_a_package_private_member_s_references_to_its_package_when_another_package_uses_the_name`.
fn package_private(snapshot: &Snapshot, hit: Hit, d: &Declaration) -> bool {
    let access = Modifiers::PUBLIC
        .with(Modifiers::PROTECTED)
        .with(Modifiers::PRIVATE);
    if d.modifiers.bits() & access.bits() != 0
        || matches!(
            d.kind,
            DeclKind::EnumConstant | DeclKind::RecordComponent | DeclKind::AnnotationElement
        )
    {
        return false;
    }
    match d.parent {
        None => true,
        Some(parent) => snapshot
            .decl(Hit {
                file: hit.file,
                decl: parent,
            })
            .is_some_and(|p| !matches!(p.kind, DeclKind::Interface | DeclKind::Annotation)),
    }
}

/// Whether `site` may use the declaration `d` (`hit`) as far as its shape and argument count
/// tell, with no name resolution (module documentation). Every rule only removes what Java
/// cannot bind there; anything unsure stays.
///
/// Pinned by: `jfl-adapter-java`'s `tests/definitions/references.rs` (javac's bindings within
/// `exact ∪ possible`).
fn compatible(resolver: &Resolver<'_>, hit: Hit, d: &Declaration, site: &ReferenceSite) -> bool {
    use RefShape::{Annotation, Call, MethodRef, Name, New, QualifiedCall, QualifiedName, TypeUse};
    let shape = match d.kind {
        // A type: anything but a call (`T(…)` calls a method named like it).
        kind if kind.is_type() => !matches!(site.shape, Call | QualifiedCall),
        // A method: a call, a method reference, a static import's last segment.
        DeclKind::Method => matches!(site.shape, Call | QualifiedCall | MethodRef | QualifiedName),
        // A constructor: `new T(…)`, `this(…)`, `super(…)` (all `New`), `T::new`.
        DeclKind::Constructor => matches!(site.shape, New | MethodRef),
        // A field or an enum constant: a name, a field access, a static import, and a type use
        // (a method reference's qualifier `a.b::m` may parse as a type where Java reads fields).
        DeclKind::Field | DeclKind::EnumConstant => {
            matches!(site.shape, Name | QualifiedName | TypeUse)
        }
        // A record component (its field and accessor) or an annotation element (a key, a call).
        DeclKind::RecordComponent | DeclKind::AnnotationElement => {
            !matches!(site.shape, New | Annotation)
        }
        _ => true,
    };
    // A type's creation passes its constructor's arguments, not the type's: no count applies.
    shape && (d.kind.is_type() || !site.shape.has_arity() || resolver.applicable(hit, site.arity.0))
}

/// Whether the target may be used without its name (module documentation).
///
/// **No oracle sees these uses**: javac's recording (`tools/javac-bindings`) and the completeness
/// oracle see only uses that write a name, and an implicit `super()`, a record pattern's accessor
/// call or a library's call has no tree with a position. Each rule is pinned by a component test
/// of `tests/queries.rs` instead, and a bytecode oracle (javac's `invoke*` targets against
/// `exact ∪ possible`) is on the IDX-11 card's Left:
/// - a method or record component whose type's hierarchy leaves the index, or that has one of
///   `Object`'s names: `should_say_implicit_uses_when_a_method_may_be_called_without_its_name`;
/// - a constructor a zero-argument call reaches (no parameters, or variable arity), private
///   included (a nested subclass's implicit `super()`, JLS 6.6.1, 8.8.7), and an enum's:
///   `should_list_this_super_and_method_references_when_a_constructor_s_references_are_asked`
///   and `should_say_implicit_uses_when_an_implicit_super_may_call_the_constructor`;
/// - a record component, and a method declared as one's accessor (a record pattern calls them,
///   JLS 14.30.2): `should_say_implicit_uses_when_a_record_pattern_may_call_the_accessor`;
/// - an annotation's `value` element (`@A(x)`):
///   `should_say_implicit_uses_when_a_method_may_be_called_without_its_name`.
fn implicit(resolver: &Resolver<'_>, hit: Hit, d: &Declaration) -> bool {
    let private = d.modifiers.contains(Modifiers::PRIVATE);
    let owner = d.parent.map(|decl| Hit {
        file: hit.file,
        decl,
    });
    let owner_kind = owner.and_then(|o| resolver.decl(o)).map(|o| o.kind);
    let name = resolver
        .file(hit.file)
        .map_or("", |f| f.summary.str(d.name));
    match d.kind {
        DeclKind::RecordComponent => true,
        DeclKind::Method
            if owner_kind == Some(DeclKind::Record) && d.params.len == 0 && {
                // An explicit accessor: a component of the record has its name.
                owner.is_some_and(|o| {
                    resolver
                        .members(o, name, Member::Field)
                        .hits
                        .iter()
                        .any(|&m| {
                            resolver
                                .decl(m)
                                .is_some_and(|c| c.kind == DeclKind::RecordComponent)
                        })
                })
            } =>
        {
            true
        }
        DeclKind::Method if !private && !d.modifiers.contains(Modifiers::STATIC) => {
            owner.is_none_or(|owner| resolver.members(owner, name, Member::Method).outside)
        }
        DeclKind::Constructor => owner_kind == Some(DeclKind::Enum) || resolver.applicable(hit, 0),
        DeclKind::AnnotationElement => name == "value",
        _ => false,
    }
}
