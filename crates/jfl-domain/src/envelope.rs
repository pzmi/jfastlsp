//! The result envelope (plan §2.1, ADR 0014) and the completeness computation.
//!
//! Every navigation answer is one [`Envelope`]: `exact` hits bound to the target with evidence,
//! `possible` hits (a sound superset of the rest, each with its [`Why`]), and a completeness
//! statement. **`complete=true` is never wrong**: it means every module that can see the target's
//! module is indexed and fresh at the pinned generation, so `exact ∪ possible` holds every
//! reference.
//!
//! The type makes the promise structural:
//!
//! - [`Completeness`] is either complete or carries a non-empty list of reasons; nothing else is
//!   representable, so `incomplete_because` is never empty when `complete` is false.
//! - The only way to obtain a complete value is [`completeness`], which checks every visible
//!   module against the [`IndexState`] at the pinned generation. Every other constructor
//!   ([`Completeness::because`], [`Completeness::and`], [`Completeness::deadline`]) can only make
//!   an answer *less* complete.
//! - An [`Envelope`] always carries one, so an answer with no hits and no completeness statement
//!   cannot be built.
//! - A complete value records the generation it was checked at, and [`Envelope::new`] demotes it
//!   when the view pinned another generation or changes are pending (`pending_events > 0`).
//! - A module not yet listed has no [`ModuleIndex`]; an empty one is [`ModuleIndex::empty`], so
//!   "not listed" never passes for "empty".
//!
//! Ordering (the card's rule, plan §2.1): `exact` and `possible` are each sorted by URI then
//! range, duplicates removed, and a location in `exact` is removed from `possible` (evidence
//! wins). The result depends only on the *set* of inputs, never their order
//! (`tests/properties.rs::should_build_the_same_envelope_when_inputs_are_shuffled`).
//!
//! Ranges are in the carrier's [`PositionEncoding`], which travels with the envelope; the query
//! engine converts its byte ranges with each file's [`LineIndex`](crate::LineIndex) at assembly.
//! The order is the same in every encoding, as conversion is monotonic within a file.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;

use crate::symbol::SymbolId;
use crate::text::{PositionEncoding, Range};
use crate::uri::FileUri;

/// A view's index generation (plan §4.5): strictly increasing per view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Generation(pub u64);

/// A Git object id: the `HEAD` commit of a view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObjectId {
    /// A SHA-1 repository's id.
    Sha1([u8; 20]),
    /// A SHA-256 repository's id.
    Sha256([u8; 32]),
}

/// Why an object id is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ObjectIdError {
    /// Not 40 or 64 hexadecimal digits.
    #[error("object id is not 40 or 64 hexadecimal digits")]
    Malformed,
}

impl ObjectId {
    /// Parses 40 or 64 hexadecimal digits, as `git rev-parse` prints them.
    ///
    /// Pinned by: `envelope::tests::should_parse_an_object_id_when_it_is_40_or_64_hex_digits`.
    pub fn parse(hex: &str) -> Result<Self, ObjectIdError> {
        fn fill(hex: &str, out: &mut [u8]) -> Result<(), ObjectIdError> {
            let (pairs, _) = hex.as_bytes().as_chunks::<2>();
            for (byte, pair) in out.iter_mut().zip(pairs) {
                let text = core::str::from_utf8(pair).map_err(|_| ObjectIdError::Malformed)?;
                *byte = u8::from_str_radix(text, 16).map_err(|_| ObjectIdError::Malformed)?;
            }
            Ok(())
        }
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ObjectIdError::Malformed);
        }
        match hex.len() {
            40 => {
                let mut bytes = [0; 20];
                fill(hex, &mut bytes)?;
                Ok(Self::Sha1(bytes))
            }
            64 => {
                let mut bytes = [0; 32];
                fill(hex, &mut bytes)?;
                Ok(Self::Sha256(bytes))
            }
            _ => Err(ObjectIdError::Malformed),
        }
    }

    fn bytes(&self) -> &[u8] {
        match self {
            Self::Sha1(bytes) => bytes,
            Self::Sha256(bytes) => bytes,
        }
    }
}

/// Lower-case hexadecimal, as Git prints it.
impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.bytes() {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// The longest module name accepted, in bytes.
pub const MAX_MODULE_NAME_LEN: usize = 1024;

/// A module of the project model: a Gradle project path such as `:payments`. Before the project
/// model exists (M3) a worktree is one module, [`ModuleName::worktree`] (plan §2.1).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleName(Box<str>);

/// Why a module name is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ModuleNameError {
    /// Empty, longer than [`MAX_MODULE_NAME_LEN`], or holding a control character.
    #[error("module name is empty, too long or holds a control character")]
    Invalid,
}

impl ModuleName {
    /// The module named `name`.
    pub fn new(name: &str) -> Result<Self, ModuleNameError> {
        if name.is_empty() || name.len() > MAX_MODULE_NAME_LEN || name.chars().any(char::is_control)
        {
            return Err(ModuleNameError::Invalid);
        }
        Ok(Self(name.into()))
    }

    /// The whole worktree's source set as one module, `:` (Gradle's root project path).
    #[must_use]
    pub fn worktree() -> Self {
        Self(":".into())
    }

    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModuleName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What the index holds of one module at one generation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleIndex {
    module: ModuleName,
    generation: Generation,
    files: u32,
    indexed: u32,
    stale: u32,
    missing_generated_roots: u32,
}

/// Why a module's index state is refused: counts that cannot all be true.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IndexStateError {
    /// More files indexed than the module has, or more stale than indexed.
    #[error("module index counts are inconsistent")]
    InconsistentCounts,
    /// One module listed twice.
    #[error("a module is listed twice")]
    DuplicateModule,
    /// `files == 0` through [`ModuleIndex::new`]: an empty module is [`ModuleIndex::empty`].
    #[error("a module with no files is built with ModuleIndex::empty")]
    NoFiles,
}

impl ModuleIndex {
    /// `module` at `generation`: `files` source files, of which `indexed` have a summary in the
    /// generation and `stale` of those are known to have changed on disk since (a freshness stat
    /// or watcher saw it, plan §4.3); and `missing_generated_roots` generated-source roots the
    /// project model names but the disk does not hold.
    ///
    /// `files` counts a finished listing and is at least 1: a module not yet listed has no
    /// `ModuleIndex` (it reads "not indexed"), and a module listed and found empty is
    /// [`ModuleIndex::empty`], so "not listed yet" can never pass for "empty". Pinned by:
    /// `envelope::tests::should_refuse_index_state_when_it_cannot_be_true`.
    pub fn new(
        module: ModuleName,
        generation: Generation,
        files: u32,
        indexed: u32,
        stale: u32,
        missing_generated_roots: u32,
    ) -> Result<Self, IndexStateError> {
        if files == 0 {
            return Err(IndexStateError::NoFiles);
        }
        if indexed > files || stale > indexed {
            return Err(IndexStateError::InconsistentCounts);
        }
        Ok(Self {
            module,
            generation,
            files,
            indexed,
            stale,
            missing_generated_roots,
        })
    }

    /// `module` at `generation`, listed and found to hold no source file; it is complete unless
    /// `missing_generated_roots` is not 0 (its only sources may be generated). Pinned by:
    /// `envelope::tests::should_be_complete_only_when_every_visible_module_is_indexed_and_fresh`.
    #[must_use]
    pub const fn empty(
        module: ModuleName,
        generation: Generation,
        missing_generated_roots: u32,
    ) -> Self {
        Self {
            module,
            generation,
            files: 0,
            indexed: 0,
            stale: 0,
            missing_generated_roots,
        }
    }

    /// The module.
    #[must_use]
    pub const fn module(&self) -> &ModuleName {
        &self.module
    }

    /// The generation these counts are for.
    #[must_use]
    pub const fn generation(&self) -> Generation {
        self.generation
    }

    /// Source files in the module.
    #[must_use]
    pub const fn files(&self) -> u32 {
        self.files
    }

    /// Files with a summary in the generation.
    #[must_use]
    pub const fn indexed(&self) -> u32 {
        self.indexed
    }

    /// Indexed files known to have changed on disk since.
    #[must_use]
    pub const fn stale(&self) -> u32 {
        self.stale
    }

    /// Generated-source roots named but missing.
    #[must_use]
    pub const fn missing_generated_roots(&self) -> u32 {
        self.missing_generated_roots
    }
}

/// The index state of a view at the generation a request pinned.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IndexState {
    pinned: Generation,
    /// Sorted by module, no duplicates.
    modules: Vec<ModuleIndex>,
}

impl IndexState {
    /// The state at `pinned`; a module listed twice is refused.
    pub fn new(pinned: Generation, mut modules: Vec<ModuleIndex>) -> Result<Self, IndexStateError> {
        modules.sort_unstable_by(|a, b| a.module.cmp(&b.module));
        if modules.windows(2).any(|pair| match pair {
            [a, b] => a.module == b.module,
            _ => false,
        }) {
            return Err(IndexStateError::DuplicateModule);
        }
        Ok(Self { pinned, modules })
    }

    /// The generation the request pinned.
    #[must_use]
    pub const fn pinned(&self) -> Generation {
        self.pinned
    }

    fn get(&self, module: &ModuleName) -> Option<&ModuleIndex> {
        self.modules
            .binary_search_by(|m| m.module.cmp(module))
            .ok()
            .and_then(|at| self.modules.get(at))
    }
}

/// Why an answer is not complete: one entry of `incomplete_because`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IncompleteReason {
    /// No module is known to see the target, so nothing proves the answer whole.
    NoVisibleModules,
    /// A visible module has no index at all.
    ModuleNotIndexed {
        /// The module.
        module: ModuleName,
    },
    /// A visible module is only partly indexed.
    ModulePartlyIndexed {
        /// The module.
        module: ModuleName,
        /// Files with a summary.
        indexed: u32,
        /// Files in the module.
        files: u32,
    },
    /// A visible module's index state is not the pinned generation's.
    ModuleAtOtherGeneration {
        /// The module.
        module: ModuleName,
        /// The generation its state is for.
        generation: Generation,
    },
    /// Files of a visible module changed on disk after their summaries.
    ModuleStale {
        /// The module.
        module: ModuleName,
        /// How many files.
        stale: u32,
    },
    /// A visible module's generated-source roots are missing (annotation processors not run).
    GeneratedSourcesMissing {
        /// The module.
        module: ModuleName,
        /// How many roots.
        roots: u32,
    },
    /// The completeness was computed at another generation than the answer's view pinned
    /// (plan §4.5: no answer mixes two generations).
    GenerationMismatch {
        /// The generation completeness was computed at.
        computed: Generation,
        /// The generation of the answer's view.
        view: Generation,
    },
    /// Changes were seen (a stat or a watcher) that no published generation holds yet.
    PendingChanges {
        /// How many.
        events: u32,
    },
    /// The index is still loading after start (plan §2.7).
    IndexLoading,
    /// The internal deadline passed; the answer holds what was found by then (plan §2.7).
    Deadline,
}

/// The plan's wording: `module :payments not indexed`, `deadline`.
impl fmt::Display for IncompleteReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoVisibleModules => f.write_str("no module known to see the target"),
            Self::ModuleNotIndexed { module } => write!(f, "module {module} not indexed"),
            Self::ModulePartlyIndexed {
                module,
                indexed,
                files,
            } => write!(
                f,
                "module {module} partly indexed ({indexed} of {files} files)"
            ),
            Self::ModuleAtOtherGeneration { module, generation } => write!(
                f,
                "module {module} indexed at generation {}, not the pinned one",
                generation.0
            ),
            Self::ModuleStale { module, stale } => {
                write!(f, "module {module} stale ({stale} files changed on disk)")
            }
            Self::GeneratedSourcesMissing { module, roots } => {
                write!(f, "generated sources missing in {module} ({roots} roots)")
            }
            Self::GenerationMismatch { computed, view } => write!(
                f,
                "completeness computed at generation {}, answer at generation {}",
                computed.0, view.0
            ),
            Self::PendingChanges { events } => {
                write!(f, "{events} changes seen and not yet indexed")
            }
            Self::IndexLoading => f.write_str("index loading"),
            Self::Deadline => f.write_str("deadline"),
        }
    }
}

/// Whether `exact ∪ possible` is proved to hold every reference, and if not, why.
///
/// Pinned by: `envelope::tests::should_be_complete_only_when_every_visible_module_is_indexed_and_fresh`
/// and `envelope::tests::should_give_each_missing_condition_its_reason_when_completeness_is_checked`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Completeness {
    /// The generation [`completeness`] checked; `None` for a value built from a reason alone,
    /// which is never complete.
    generation: Option<Generation>,
    /// Empty when complete; sorted and deduplicated otherwise.
    reasons: Vec<IncompleteReason>,
}

impl Completeness {
    /// Incomplete because of `reason`.
    #[must_use]
    pub fn because(reason: IncompleteReason) -> Self {
        Self {
            generation: None,
            reasons: alloc::vec![reason],
        }
    }

    /// The generation the check ran at, when it came from [`completeness`].
    #[must_use]
    pub const fn generation(&self) -> Option<Generation> {
        self.generation
    }

    /// `self`, also incomplete because of `reason`: never more complete than `self`.
    #[must_use]
    pub fn and(mut self, reason: IncompleteReason) -> Self {
        if let Err(at) = self.reasons.binary_search(&reason) {
            self.reasons.insert(at, reason);
        }
        self
    }

    /// `self` with the deadline passed (plan §2.7).
    #[must_use]
    pub fn deadline(self) -> Self {
        self.and(IncompleteReason::Deadline)
    }

    /// Whether every reference is in `exact ∪ possible`.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.reasons.is_empty()
    }

    /// `incomplete_because`: empty exactly when complete.
    #[must_use]
    pub fn reasons(&self) -> &[IncompleteReason] {
        &self.reasons
    }
}

/// Whether an answer about a target seen by the `visible` modules is complete in `index`: only
/// when there is at least one visible module and every one of them is in `index` at the pinned
/// generation, wholly indexed, with no stale file and no missing generated-source root. Every
/// condition that fails gives its own reason, ordered by module.
///
/// `visible` is the set of modules that can see the target's module (the target's own included;
/// before M3, `[ModuleName::worktree()]`); its order and repeats do not matter.
#[must_use]
pub fn completeness(visible: &[ModuleName], index: &IndexState) -> Completeness {
    let mut visible: Vec<&ModuleName> = visible.iter().collect();
    visible.sort_unstable();
    visible.dedup();
    if visible.is_empty() {
        return Completeness {
            generation: Some(index.pinned),
            reasons: alloc::vec![IncompleteReason::NoVisibleModules],
        };
    }
    let mut reasons = Vec::new();
    for module in visible {
        let Some(state) = index.get(module) else {
            reasons.push(IncompleteReason::ModuleNotIndexed {
                module: module.clone(),
            });
            continue;
        };
        if state.generation != index.pinned {
            reasons.push(IncompleteReason::ModuleAtOtherGeneration {
                module: module.clone(),
                generation: state.generation,
            });
        }
        if state.indexed == 0 && state.files > 0 {
            reasons.push(IncompleteReason::ModuleNotIndexed {
                module: module.clone(),
            });
        } else if state.indexed < state.files {
            reasons.push(IncompleteReason::ModulePartlyIndexed {
                module: module.clone(),
                indexed: state.indexed,
                files: state.files,
            });
        }
        if state.stale > 0 {
            reasons.push(IncompleteReason::ModuleStale {
                module: module.clone(),
                stale: state.stale,
            });
        }
        if state.missing_generated_roots > 0 {
            reasons.push(IncompleteReason::GeneratedSourcesMissing {
                module: module.clone(),
                roots: state.missing_generated_roots,
            });
        }
    }
    reasons.sort_unstable();
    reasons.dedup();
    Completeness {
        generation: Some(index.pinned),
        reasons,
    }
}

/// A place in a file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Location {
    /// The file.
    pub uri: FileUri,
    /// The range, in the envelope's encoding.
    pub range: Range,
}

/// Why a `possible` hit could not be proved (plan §2.1, §2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Reason {
    /// The receiver's type is unknown (`var` in a lambda, a chain, a raw type).
    ReceiverTypeUnknown,
    /// The owner type is proved but not the overload: every overload is listed (bounded
    /// ambiguity).
    OverloadNotProved,
    /// Matched by simple name and arity only.
    NameAndArity,
    /// A method reference, matched by name whatever the arity.
    MethodReference,
    /// An import that does not resolve.
    ImportUnresolved,
    /// A member that generated code (an annotation processor, Lombok) may declare.
    GeneratedMember,
    /// No classpath is known for the file.
    NoClasspath,
    /// The file has a syntax error around the site.
    SyntaxError,
}

impl Reason {
    /// The reason in words.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::ReceiverTypeUnknown => "receiver type unknown",
            Self::OverloadNotProved => "overload not proved",
            Self::NameAndArity => "matched by name and arity",
            Self::MethodReference => "method reference, matched by name",
            Self::ImportUnresolved => "import unresolved",
            Self::GeneratedMember => "member may be generated",
            Self::NoClasspath => "no classpath known",
            Self::SyntaxError => "syntax error nearby",
        }
    }
}

/// The longest [`Why`] detail kept, in bytes; a longer one is cut at a character boundary.
pub const MAX_WHY_DETAIL_LEN: usize = 256;

/// A [`Reason`] and an optional detail: `receiver type unknown (var in lambda)`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Why {
    reason: Reason,
    detail: Option<Box<str>>,
}

impl Why {
    /// The bare reason.
    #[must_use]
    pub const fn new(reason: Reason) -> Self {
        Self {
            reason,
            detail: None,
        }
    }

    /// The reason with a detail, cut to [`MAX_WHY_DETAIL_LEN`] bytes.
    #[must_use]
    pub fn with_detail(reason: Reason, detail: &str) -> Self {
        let mut end = detail.len().min(MAX_WHY_DETAIL_LEN);
        while !detail.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        Self {
            reason,
            detail: Some(detail.get(..end).unwrap_or("").into()),
        }
    }

    /// The reason.
    #[must_use]
    pub const fn reason(&self) -> Reason {
        self.reason
    }

    /// The detail.
    #[must_use]
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }
}

impl fmt::Display for Why {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.reason.text())?;
        if let Some(detail) = &self.detail {
            write!(f, " ({detail})")?;
        }
        Ok(())
    }
}

/// A `possible` hit.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PossibleHit {
    /// Where.
    pub location: Location,
    /// Why it could not be proved.
    pub why: Why,
}

/// How well the query's target is known (plan §2.2's hover status line).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetStatus {
    /// Bound with evidence.
    Exact,
    /// The owner type is proved, not the overload: `ambiguous (3 overloads)`.
    Ambiguous {
        /// How many overloads.
        overloads: u32,
    },
}

/// The symbol the query resolved to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Target {
    /// The symbol.
    pub symbol: SymbolId,
    /// How well it is known.
    pub status: TargetStatus,
}

/// The view an answer was computed in (plan §2.1, §4.5).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct View {
    /// The worktree root.
    pub worktree: FileUri,
    /// Its `HEAD` commit; `None` on an unborn branch.
    pub head: Option<ObjectId>,
    /// The generation the request pinned.
    pub generation: Generation,
}

/// Milliseconds since the Unix epoch, read by the caller (the domain has no clock).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnixMillis(pub u64);

/// How current the view was when the answer was computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Freshness {
    /// When the pinned generation's last reconcile finished; `None` before the first.
    pub synced_at: Option<UnixMillis>,
    /// Changes seen (stat, watcher) and not yet in a published generation.
    pub pending_events: u32,
}

/// Everything an [`Envelope`] is built from.
#[derive(Debug, Clone)]
pub struct EnvelopeParts {
    /// The view and pinned generation.
    pub view: View,
    /// The target, if resolved; `None` when unresolved (`possible` then carries the candidates).
    pub target: Option<Target>,
    /// Hits bound to the target with evidence, in any order.
    pub exact: Vec<Location>,
    /// The other candidates, in any order.
    pub possible: Vec<PossibleHit>,
    /// From [`completeness`], possibly made less complete.
    pub completeness: Completeness,
    /// How current the view was.
    pub freshness: Freshness,
    /// The encoding of every range.
    pub position_encoding: PositionEncoding,
}

/// One navigation answer (plan §2.1, ADR 0014). Its JSON form is `jfl-protocol`'s (IDX-03).
///
/// Pinned by: `envelope::tests::should_order_exact_before_possible_by_uri_and_range_when_an_envelope_is_built` and
/// `tests/properties.rs::should_build_the_same_envelope_when_inputs_are_shuffled`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Envelope {
    view: View,
    target: Option<Target>,
    exact: Vec<Location>,
    possible: Vec<PossibleHit>,
    completeness: Completeness,
    freshness: Freshness,
    position_encoding: PositionEncoding,
}

impl Envelope {
    /// The envelope of `parts`: `exact` and `possible` sorted by URI and range and
    /// deduplicated, a location in `exact` dropped from `possible`, and of two `possible` hits at
    /// one location the one whose [`Why`] sorts first kept.
    ///
    /// The completeness is bound to the answer: when it was computed at another generation than
    /// `view.generation`, the answer is incomplete with [`IncompleteReason::GenerationMismatch`],
    /// and while `freshness.pending_events` is not 0 with [`IncompleteReason::PendingChanges`].
    /// Pinned by: `envelope::tests::should_demote_completeness_when_it_does_not_match_the_answer`.
    #[must_use]
    pub fn new(parts: EnvelopeParts) -> Self {
        let EnvelopeParts {
            view,
            target,
            mut exact,
            mut possible,
            mut completeness,
            freshness,
            position_encoding,
        } = parts;
        if let Some(computed) = completeness.generation
            && computed != view.generation
        {
            completeness = completeness.and(IncompleteReason::GenerationMismatch {
                computed,
                view: view.generation,
            });
        }
        if freshness.pending_events > 0 {
            completeness = completeness.and(IncompleteReason::PendingChanges {
                events: freshness.pending_events,
            });
        }
        exact.sort_unstable();
        exact.dedup();
        possible.sort_unstable();
        possible.dedup_by(|later, earlier| later.location == earlier.location);
        possible.retain(|hit| exact.binary_search(&hit.location).is_err());
        Self {
            view,
            target,
            exact,
            possible,
            completeness,
            freshness,
            position_encoding,
        }
    }

    /// The view and pinned generation.
    #[must_use]
    pub const fn view(&self) -> &View {
        &self.view
    }

    /// The target, if resolved.
    #[must_use]
    pub const fn target(&self) -> Option<&Target> {
        self.target.as_ref()
    }

    /// Hits bound with evidence, by URI and range.
    #[must_use]
    pub fn exact(&self) -> &[Location] {
        &self.exact
    }

    /// The other candidates, by URI and range, none of them in `exact`.
    #[must_use]
    pub fn possible(&self) -> &[PossibleHit] {
        &self.possible
    }

    /// `complete`.
    #[must_use]
    pub fn complete(&self) -> bool {
        self.completeness.is_complete()
    }

    /// `incomplete_because`: non-empty exactly when `complete` is false.
    #[must_use]
    pub fn incomplete_because(&self) -> &[IncompleteReason] {
        self.completeness.reasons()
    }

    /// How current the view was.
    #[must_use]
    pub const fn freshness(&self) -> Freshness {
        self.freshness
    }

    /// The encoding of every range.
    #[must_use]
    pub const fn position_encoding(&self) -> PositionEncoding {
        self.position_encoding
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Position;
    use alloc::string::ToString;
    use alloc::vec;

    fn module(name: &str) -> ModuleName {
        ModuleName::new(name).unwrap()
    }

    fn state(
        name: &str,
        generation: u64,
        files: u32,
        indexed: u32,
        stale: u32,
        missing: u32,
    ) -> ModuleIndex {
        ModuleIndex::new(
            module(name),
            Generation(generation),
            files,
            indexed,
            stale,
            missing,
        )
        .unwrap()
    }

    fn location(path: &str, line: u32, column: u32) -> Location {
        let at = Position::new(line, column);
        Location {
            uri: FileUri::from_path(path).unwrap(),
            range: Range::new(at, at).unwrap(),
        }
    }

    fn envelope(exact: Vec<Location>, possible: Vec<PossibleHit>) -> Envelope {
        Envelope::new(EnvelopeParts {
            view: View {
                worktree: FileUri::from_path("/repo").unwrap(),
                head: None,
                generation: Generation(1),
            },
            target: None,
            exact,
            possible,
            completeness: Completeness::because(IncompleteReason::IndexLoading),
            freshness: Freshness {
                synced_at: None,
                pending_events: 0,
            },
            position_encoding: PositionEncoding::Utf16,
        })
    }

    fn answer(completeness: Completeness, generation: u64, pending_events: u32) -> Envelope {
        Envelope::new(EnvelopeParts {
            view: View {
                worktree: FileUri::from_path("/repo").unwrap(),
                head: None,
                generation: Generation(generation),
            },
            target: None,
            exact: vec![],
            possible: vec![],
            completeness,
            freshness: Freshness {
                synced_at: None,
                pending_events,
            },
            position_encoding: PositionEncoding::Utf16,
        })
    }

    #[test]
    fn should_demote_completeness_when_it_does_not_match_the_answer() {
        // given
        let index = IndexState::new(Generation(7), vec![state(":", 7, 2, 2, 0, 0)]).unwrap();
        let complete = completeness(&[ModuleName::worktree()], &index);
        // when
        let matching = answer(complete.clone(), 7, 0);
        let newer_view = answer(complete.clone(), 8, 0);
        let pending = answer(complete.clone(), 7, 3);
        // then
        assert_eq!(complete.generation(), Some(Generation(7)));
        assert!(matching.complete());
        assert!(!newer_view.complete());
        assert_eq!(
            newer_view.incomplete_because(),
            [IncompleteReason::GenerationMismatch {
                computed: Generation(7),
                view: Generation(8)
            }]
        );
        assert!(!pending.complete());
        assert_eq!(
            pending.incomplete_because(),
            [IncompleteReason::PendingChanges { events: 3 }]
        );
    }

    #[test]
    fn should_stay_incomplete_when_an_empty_module_misses_generated_roots() {
        // given
        let index = IndexState::new(
            Generation(7),
            vec![ModuleIndex::empty(module(":gen-only"), Generation(7), 2)],
        )
        .unwrap();
        // when
        let found = completeness(&[module(":gen-only")], &index);
        // then
        assert_eq!(
            found.reasons(),
            [IncompleteReason::GeneratedSourcesMissing {
                module: module(":gen-only"),
                roots: 2
            }]
        );
    }

    #[test]
    fn should_be_complete_only_when_every_visible_module_is_indexed_and_fresh() {
        // given
        let index = IndexState::new(
            Generation(7),
            vec![
                state(":api", 7, 10, 10, 0, 0),
                state(":core", 7, 3, 3, 0, 0),
                ModuleIndex::empty(module(":empty"), Generation(7), 0),
            ],
        )
        .unwrap();
        // when
        let all = completeness(
            &[
                module(":core"),
                module(":api"),
                module(":empty"),
                module(":api"),
            ],
            &index,
        );
        let one = completeness(&[module(":core")], &index);
        let worktree = completeness(
            &[ModuleName::worktree()],
            &IndexState::new(Generation(1), vec![state(":", 1, 5, 5, 0, 0)]).unwrap(),
        );
        // then
        assert!(all.is_complete() && all.reasons().is_empty());
        assert!(one.is_complete());
        assert!(worktree.is_complete());
    }

    #[test]
    fn should_give_each_missing_condition_its_reason_when_completeness_is_checked() {
        // given
        let index = IndexState::new(
            Generation(7),
            vec![
                state(":fresh", 7, 4, 4, 0, 0),
                state(":none", 7, 4, 0, 0, 0),
                state(":part", 7, 4, 3, 0, 0),
                state(":old", 6, 4, 4, 0, 0),
                state(":stale", 7, 4, 4, 2, 0),
                state(":gen", 7, 4, 4, 0, 1),
                state(":worst", 5, 4, 2, 1, 3),
            ],
        )
        .unwrap();
        let cases: Vec<(Vec<ModuleName>, Vec<IncompleteReason>)> = vec![
            (vec![], vec![IncompleteReason::NoVisibleModules]),
            (
                vec![module(":fresh"), module(":absent")],
                vec![IncompleteReason::ModuleNotIndexed {
                    module: module(":absent"),
                }],
            ),
            (
                vec![module(":none")],
                vec![IncompleteReason::ModuleNotIndexed {
                    module: module(":none"),
                }],
            ),
            (
                vec![module(":part")],
                vec![IncompleteReason::ModulePartlyIndexed {
                    module: module(":part"),
                    indexed: 3,
                    files: 4,
                }],
            ),
            (
                vec![module(":old")],
                vec![IncompleteReason::ModuleAtOtherGeneration {
                    module: module(":old"),
                    generation: Generation(6),
                }],
            ),
            (
                vec![module(":stale")],
                vec![IncompleteReason::ModuleStale {
                    module: module(":stale"),
                    stale: 2,
                }],
            ),
            (
                vec![module(":gen")],
                vec![IncompleteReason::GeneratedSourcesMissing {
                    module: module(":gen"),
                    roots: 1,
                }],
            ),
            (
                vec![module(":worst")],
                vec![
                    IncompleteReason::ModulePartlyIndexed {
                        module: module(":worst"),
                        indexed: 2,
                        files: 4,
                    },
                    IncompleteReason::ModuleAtOtherGeneration {
                        module: module(":worst"),
                        generation: Generation(5),
                    },
                    IncompleteReason::ModuleStale {
                        module: module(":worst"),
                        stale: 1,
                    },
                    IncompleteReason::GeneratedSourcesMissing {
                        module: module(":worst"),
                        roots: 3,
                    },
                ],
            ),
        ];
        // when
        let results: Vec<_> = cases
            .iter()
            .map(|(visible, _)| completeness(visible, &index))
            .collect();
        // then
        for ((visible, expected), result) in cases.iter().zip(&results) {
            assert!(!result.is_complete(), "{visible:?}");
            let mut expected = expected.clone();
            expected.sort();
            assert_eq!(result.reasons(), expected.as_slice(), "{visible:?}");
        }
    }

    #[test]
    fn should_never_become_more_complete_when_a_reason_is_added() {
        // given
        let index = IndexState::new(Generation(1), vec![state(":", 1, 1, 1, 0, 0)]).unwrap();
        let complete = completeness(&[ModuleName::worktree()], &index);
        // when
        let late = complete.clone().deadline();
        let twice = late.clone().deadline().and(IncompleteReason::IndexLoading);
        // then
        assert!(complete.is_complete());
        assert_eq!(late.reasons(), [IncompleteReason::Deadline]);
        assert_eq!(
            twice.reasons(),
            [IncompleteReason::IndexLoading, IncompleteReason::Deadline]
        );
        assert_eq!(IncompleteReason::Deadline.to_string(), "deadline");
        assert_eq!(
            IncompleteReason::ModuleNotIndexed {
                module: module(":payments")
            }
            .to_string(),
            "module :payments not indexed"
        );
        assert_eq!(
            IncompleteReason::GeneratedSourcesMissing {
                module: module(":api"),
                roots: 1
            }
            .to_string(),
            "generated sources missing in :api (1 roots)"
        );
    }

    #[test]
    fn should_refuse_index_state_when_it_cannot_be_true() {
        // given
        // when
        let results = [
            ModuleIndex::new(module(":a"), Generation(1), 1, 2, 0, 0).map(|_| ()),
            ModuleIndex::new(module(":a"), Generation(1), 2, 1, 2, 0).map(|_| ()),
            ModuleIndex::new(module(":a"), Generation(1), 0, 0, 0, 0).map(|_| ()),
            IndexState::new(
                Generation(1),
                vec![state(":a", 1, 1, 1, 0, 0), state(":a", 1, 2, 2, 0, 0)],
            )
            .map(|_| ()),
        ];
        // then
        assert_eq!(
            results,
            [
                Err(IndexStateError::InconsistentCounts),
                Err(IndexStateError::InconsistentCounts),
                Err(IndexStateError::NoFiles),
                Err(IndexStateError::DuplicateModule),
            ]
        );
        assert_eq!(ModuleName::new(""), Err(ModuleNameError::Invalid));
        assert_eq!(ModuleName::new(":a\nb"), Err(ModuleNameError::Invalid));
    }

    #[test]
    fn should_order_exact_before_possible_by_uri_and_range_when_an_envelope_is_built() {
        // given
        let why = |reason| Why::new(reason);
        let exact = vec![
            location("/r/B.java", 3, 1),
            location("/r/A.java", 9, 0),
            location("/r/B.java", 3, 1),
        ];
        let possible = vec![
            PossibleHit {
                location: location("/r/B.java", 1, 0),
                why: why(Reason::NameAndArity),
            },
            PossibleHit {
                location: location("/r/A.java", 9, 0),
                why: why(Reason::NameAndArity),
            },
            PossibleHit {
                location: location("/r/A.java", 2, 4),
                why: why(Reason::MethodReference),
            },
            PossibleHit {
                location: location("/r/A.java", 2, 4),
                why: why(Reason::ReceiverTypeUnknown),
            },
        ];
        // when
        let envelope = envelope(exact, possible);
        // then
        assert_eq!(
            envelope.exact(),
            [location("/r/A.java", 9, 0), location("/r/B.java", 3, 1)]
        );
        assert_eq!(
            envelope.possible(),
            [
                PossibleHit {
                    location: location("/r/A.java", 2, 4),
                    why: why(Reason::ReceiverTypeUnknown)
                },
                PossibleHit {
                    location: location("/r/B.java", 1, 0),
                    why: why(Reason::NameAndArity)
                },
            ]
        );
        assert!(!envelope.complete());
        assert_eq!(
            envelope.incomplete_because(),
            [IncompleteReason::IndexLoading]
        );
    }

    #[test]
    fn should_print_why_with_its_detail_when_one_is_given() {
        // given
        let long = "é".repeat(MAX_WHY_DETAIL_LEN);
        // when
        let why = Why::with_detail(Reason::ReceiverTypeUnknown, "var in lambda");
        let cut = Why::with_detail(Reason::NameAndArity, &long);
        // then
        assert_eq!(why.to_string(), "receiver type unknown (var in lambda)");
        assert_eq!(cut.detail().map(str::len), Some(MAX_WHY_DETAIL_LEN));
    }

    #[test]
    fn should_parse_an_object_id_when_it_is_40_or_64_hex_digits() {
        // given
        let sha1 = "3f2c".repeat(10);
        let sha256 = "ab".repeat(32);
        // when
        let parsed = [ObjectId::parse(&sha1), ObjectId::parse(&sha256)];
        // then
        assert_eq!(parsed[0].unwrap().to_string(), sha1);
        assert_eq!(parsed[1].unwrap().to_string(), sha256);
        assert_eq!(ObjectId::parse("3f2c"), Err(ObjectIdError::Malformed));
        assert_eq!(
            ObjectId::parse(&"g".repeat(40)),
            Err(ObjectIdError::Malformed)
        );
        assert_eq!(
            ObjectId::parse(&"+1".repeat(20)),
            Err(ObjectIdError::Malformed)
        );
    }
}
