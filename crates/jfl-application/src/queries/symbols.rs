//! Document and workspace symbols (plan §2.2: declarations from the syntax index; candidates are
//! fine because this is search).
//!
//! **Matching** ([`Matcher`]): a name matches a query when it equals it, starts with it (with its
//! case, then ignoring ASCII case), or when the query's humps match the name's humps in order. A
//! query's humps are split before each upper-case letter and at `_` (`NPE`, `NuPoEx`,
//! `getName`); a name's humps start at its first character, at an upper-case letter after a
//! lower-case one or a digit, at the last upper-case letter of a run followed by a lower-case
//! one (`HTTPServer`: `HTTP`, `Server`), and after `_` or `$`. Each query hump, in order, matches
//! the next name hump that starts with it, ignoring ASCII case; humps of the name may be skipped,
//! and the first query hump may match any hump. Matches rank exact, then prefix, then
//! case-insensitive prefix, then humps (an earlier first hump first), then by name, file and
//! position. Pinned by `symbols::tests`.
//!
//! **Completeness** is the pin's, plus `ResultsCapped` when more matched than are listed,
//! `Deadline` when the scan was cut, and, for workspace symbols, `OtherLanguageSources` when the
//! view holds files of other JVM languages, which may declare a symbol the index does not hold
//! (IDX-16; pinned by
//! `tests/queries.rs::should_answer_workspace_symbols_incomplete_when_a_kotlin_file_may_declare_one`). A workspace scan matches each distinct declaration name of
//! the pinned generation once (the generation's name table, built once per generation and kept
//! in the `QueryCache`); a symbol's location needs its file's text (`Sources`), read only for the
//! symbols listed.

use core::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap};

use jfl_domain::{
    Completeness, DeclId, DeclKind, IncompleteReason, Location, ModuleName, PositionEncoding,
    Range, RelPath, SymbolId, SyntaxKey, View,
};

use super::{Deadline, Pinned, QueryCache, QueryConfig, QueryError, Sources, convert, read_texts};
use crate::view_service::Summary;

/// The longest query matched, in bytes; a longer one matches nothing, and its answer says it is
/// incomplete (`TargetUnresolved`).
pub const MAX_QUERY_BYTES: usize = 256;

/// The `--kind` filter of `jfl sym`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolFilter {
    /// Classes, interfaces, enums, records and annotation types.
    Type,
    /// Methods and annotation elements.
    Method,
    /// Fields, enum constants and record components.
    Field,
}

impl SymbolFilter {
    fn accepts(self, kind: DeclKind) -> bool {
        match self {
            Self::Type => kind.is_type(),
            Self::Method => matches!(kind, DeclKind::Method | DeclKind::AnnotationElement),
            Self::Field => matches!(
                kind,
                DeclKind::Field | DeclKind::EnumConstant | DeclKind::RecordComponent
            ),
        }
    }
}

/// A workspace symbol query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SymbolQuery<'q> {
    /// What to match (module documentation); empty matches every symbol.
    pub text: &'q str,
    /// Only these kinds.
    pub filter: Option<SymbolFilter>,
    /// At most this many; `None` for [`QueryConfig::symbols_limit`].
    pub limit: Option<usize>,
}

/// One declaration found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSymbol {
    /// Its simple name.
    pub name: String,
    /// Its kind.
    pub kind: DeclKind,
    /// Its symbol, when it has one.
    pub symbol: Option<SymbolId>,
    /// Where its name is.
    pub location: Location,
    /// The enclosing type's name, or the package of a top-level type.
    pub container: Option<String>,
}

/// `jfl sym`'s answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbols {
    /// The view and pinned generation.
    pub view: View,
    /// The symbols, best match first.
    pub symbols: Vec<WorkspaceSymbol>,
    /// Whether every declaration matching the query is listed.
    pub completeness: Completeness,
    /// The encoding of every range.
    pub position_encoding: PositionEncoding,
}

/// One declaration of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSymbol {
    /// Its simple name.
    pub name: String,
    /// Its kind.
    pub kind: DeclKind,
    /// Its symbol, when it has one.
    pub symbol: Option<SymbolId>,
    /// Its signature.
    pub detail: String,
    /// The whole declaration.
    pub range: Range,
    /// Its name.
    pub selection: Range,
    /// The enclosing declaration's index in the list.
    pub parent: Option<usize>,
}

/// `textDocument/documentSymbol`'s answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSymbols {
    /// The declarations, in source order, each after its parent.
    pub symbols: Vec<DocumentSymbol>,
    /// Whether the document's declarations are all listed (the pin's completeness).
    pub completeness: Completeness,
    /// The encoding of every range.
    pub position_encoding: PositionEncoding,
}

/// How well a name matches; smaller is better.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Rank {
    /// 0 equal, 1 prefix, 2 prefix ignoring case, 3 humps.
    tier: u8,
    /// The name hump the first query hump matched (0 below tier 3).
    start: u16,
}

fn hump_starts(name: &[u8]) -> impl Iterator<Item = usize> + '_ {
    (0..name.len()).filter(move |&i| {
        let Some(&c) = name.get(i) else {
            return false;
        };
        let Some(prev_at) = i.checked_sub(1) else {
            return true;
        };
        let prev = name.get(prev_at).copied().unwrap_or(b'_');
        let next = name.get(i.saturating_add(1)).copied();
        (prev == b'_' || prev == b'$') && c != b'_' && c != b'$'
            || c.is_ascii_uppercase()
                && (prev.is_ascii_lowercase()
                    || prev.is_ascii_digit()
                    || (prev.is_ascii_uppercase() && next.is_some_and(|n| n.is_ascii_lowercase())))
    })
}

fn query_humps(query: &[u8]) -> Vec<&[u8]> {
    let mut humps = Vec::new();
    let mut start = 0;
    for (i, &c) in query.iter().enumerate() {
        if (c.is_ascii_uppercase() || c == b'_') && i > start {
            humps.push(query.get(start..i).unwrap_or(&[]));
            start = i;
        }
        if c == b'_' {
            start = i.saturating_add(1);
        }
    }
    if start < query.len() {
        humps.push(query.get(start..).unwrap_or(&[]));
    }
    humps
}

fn starts_ignoring_case(text: &[u8], prefix: &[u8]) -> bool {
    text.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

/// A query, prepared once for every name it is matched with.
#[derive(Debug, Clone)]
pub(crate) struct Matcher<'q> {
    query: &'q str,
    humps: Vec<&'q [u8]>,
}

impl<'q> Matcher<'q> {
    /// `query` prepared; `None` for one over [`MAX_QUERY_BYTES`], which matches nothing.
    pub(crate) fn new(query: &'q str) -> Option<Self> {
        (query.len() <= MAX_QUERY_BYTES).then(|| Self {
            query,
            humps: query_humps(query.as_bytes()),
        })
    }

    /// How `name` matches (module documentation), if it does. Allocates nothing.
    ///
    /// Pinned by: `symbols::tests::should_rank_exact_prefix_and_humps_when_names_are_matched`.
    pub(crate) fn rank(&self, name: &str) -> Option<Rank> {
        let (n, q) = (name.as_bytes(), self.query.as_bytes());
        if name == self.query {
            return Some(Rank { tier: 0, start: 0 });
        }
        if name.starts_with(self.query) {
            return Some(Rank { tier: 1, start: 0 });
        }
        if starts_ignoring_case(n, q) {
            return Some(Rank { tier: 2, start: 0 });
        }
        // Every query byte, in order, ignoring case: a cheap refusal of most names.
        let mut rest = n.iter();
        if !q
            .iter()
            .filter(|&&b| b != b'_')
            .all(|b| rest.any(|c| c.eq_ignore_ascii_case(b)))
        {
            return None;
        }
        let mut starts = hump_starts(n).enumerate();
        let mut first = None;
        for hump in &self.humps {
            let (index, _) = starts.find(|&(_, at)| {
                n.get(at..)
                    .is_some_and(|rest| starts_ignoring_case(rest, hump))
            })?;
            first.get_or_insert(index);
        }
        let start = u16::try_from(first?).unwrap_or(u16::MAX);
        Some(Rank { tier: 3, start })
    }
}

/// How `name` matches `query`, if it does: [`Matcher::rank`] for one name.
#[cfg(test)]
pub(crate) fn rank(name: &str, query: &str) -> Option<Rank> {
    Matcher::new(query)?.rank(name)
}

/// A match kept for the answer, ordered best first.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Kept<'g> {
    rank: Rank,
    name: &'g str,
    file: u32,
    decl: DeclId,
}

impl Ord for Kept<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.rank, self.name, self.file, self.decl)
            .cmp(&(other.rank, other.name, other.file, other.decl))
    }
}

impl PartialOrd for Kept<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The container a workspace symbol shows: its enclosing declaration's name, or its package.
fn container(summary: &jfl_domain::FileSummary, parent: Option<DeclId>) -> Option<String> {
    match parent.and_then(|p| summary.declaration(p)) {
        Some(d) => Some(summary.str(d.name).to_owned()),
        None => summary.package().map(str::to_owned),
    }
}

/// `jfl sym` / `workspace/symbol` (plan §2.2, §2.7): the declarations of the pinned generation
/// whose names match `query`, best first, at most its limit.
///
/// Pinned by `tests/queries.rs` and `symbols::tests`.
pub async fn workspace_symbols<S: Sources, D: Deadline>(
    pinned: &Pinned<'_>,
    sources: &S,
    query: SymbolQuery<'_>,
    encoding: PositionEncoding,
    config: &QueryConfig,
    deadline: &D,
    cache: &QueryCache,
) -> Symbols {
    let answer = pinned.answer;
    let limit = query.limit.unwrap_or(config.symbols_limit).max(1);
    let mut completeness = answer.completeness.clone();
    // Any file of another JVM language may declare a symbol the search does not list (IDX-16).
    if answer.other_sources.total() > 0 {
        completeness = completeness.and(IncompleteReason::OtherLanguageSources {
            files: answer.other_sources.total(),
        });
    }
    let empty = || Symbols {
        view: pinned.view(),
        symbols: Vec::new(),
        completeness: answer.completeness.clone().deadline(),
        position_encoding: encoding,
    };
    let Some(snapshot) = cache.snapshot(answer, deadline) else {
        return empty();
    };
    let matcher = Matcher::new(query.text);
    if matcher.is_none() {
        // A query over the limit matches nothing here, yet a declaration may have that name.
        completeness = completeness.and(IncompleteReason::TargetUnresolved);
    }
    let mut kept: BinaryHeap<Kept<'_>> = BinaryHeap::with_capacity(limit.saturating_add(1));
    let mut matched = 0_usize;
    let mut capped = false;
    for (i, (name, hits)) in snapshot.names().enumerate() {
        if i % 4096 == 4095 && deadline.passed() {
            completeness = completeness.deadline();
            break;
        }
        let Some(rank) = matcher.as_ref().and_then(|m| m.rank(name)) else {
            continue;
        };
        if kept.len() == limit
            && kept
                .peek()
                .is_some_and(|worst| (rank, name) > (worst.rank, worst.name))
        {
            // Every declaration of this name ranks below all kept: none is listed, and it is a
            // match beyond the limit. A name with declarations always has one a query with no
            // filter lists (a constructor shares its name with its type); under a filter this
            // may call a full answer capped, never the reverse.
            capped = true;
            continue;
        }
        for &hit in hits {
            let Some(d) = snapshot.decl(hit) else {
                continue;
            };
            if d.kind == DeclKind::Constructor || query.filter.is_some_and(|f| !f.accepts(d.kind)) {
                continue;
            }
            matched = matched.saturating_add(1);
            let candidate = Kept {
                rank,
                name,
                file: hit.file,
                decl: hit.decl,
            };
            if kept.len() < limit {
                kept.push(candidate);
            } else if kept.peek().is_some_and(|worst| candidate < *worst) {
                kept.pop();
                kept.push(candidate);
            }
        }
    }
    if capped || matched > limit {
        completeness = completeness.and(IncompleteReason::ResultsCapped {
            limit: u32::try_from(limit).unwrap_or(u32::MAX),
        });
    }
    let kept = kept.into_sorted_vec();
    let mut wanted: Vec<(usize, &RelPath, &SyntaxKey)> = kept
        .iter()
        .filter_map(|k| {
            let f = snapshot.file(k.file)?;
            Some((usize::try_from(k.file).ok()?, &f.path, &f.key))
        })
        .collect();
    wanted.sort_unstable_by_key(|w| w.0);
    wanted.dedup_by_key(|w| w.0);
    let (texts, stale, cut) = read_texts(&answer.view, wanted, sources, deadline).await;
    if cut {
        completeness = completeness.deadline();
    }
    let mut symbols = Vec::with_capacity(kept.len());
    for k in kept {
        let Some(f) = snapshot.file(k.file) else {
            continue;
        };
        let Some(text) = usize::try_from(k.file).ok().and_then(|at| texts.get(&at)) else {
            continue;
        };
        let Some(d) = f.summary.declaration(k.decl) else {
            continue;
        };
        let (Some(range), Ok(uri)) = (
            convert(text, d.name_range, encoding),
            answer.view.join(&f.path),
        ) else {
            continue;
        };
        symbols.push(WorkspaceSymbol {
            name: k.name.to_owned(),
            kind: d.kind,
            symbol: f.summary.symbol_id(k.decl).ok(),
            location: Location { uri, range },
            container: container(&f.summary, d.parent),
        });
    }
    if stale > 0 {
        completeness = completeness.and(IncompleteReason::ModuleStale {
            module: ModuleName::worktree(),
            stale,
        });
    }
    Symbols {
        view: pinned.view(),
        symbols,
        completeness,
        position_encoding: encoding,
    }
}

/// `textDocument/documentSymbol`: every declaration of the request's file, in source order.
///
/// Pinned by `tests/queries.rs::should_list_a_document_s_declarations_when_asked_for_its_symbols`.
pub async fn document_symbols<S: Sources>(
    pinned: &Pinned<'_>,
    sources: &S,
    encoding: PositionEncoding,
) -> Result<DocumentSymbols, QueryError> {
    let answer = pinned.answer;
    let completeness = answer.completeness.clone();
    let entry = answer
        .generation
        .index
        .get(&answer.path)
        .ok_or(QueryError::NotInGeneration)?;
    let (key, summary) = match &entry.summary {
        Some(Summary::Indexed { key, summary }) => (key, summary),
        Some(Summary::Absent) => return Err(QueryError::NotInGeneration),
        Some(Summary::Gap | Summary::Pending) | None => {
            return Ok(DocumentSymbols {
                symbols: Vec::new(),
                completeness: completeness.and(IncompleteReason::TargetUnresolved),
                position_encoding: encoding,
            });
        }
    };
    let text = sources
        .text(&answer.view, &answer.path, key)
        .await
        .ok_or(QueryError::Changed)?;
    let mut index_of: BTreeMap<DeclId, usize> = BTreeMap::new();
    let mut symbols = Vec::new();
    for (id, d) in summary.declarations() {
        let (Some(range), Some(selection)) = (
            convert(&text, d.range, encoding),
            convert(&text, d.name_range, encoding),
        ) else {
            continue;
        };
        index_of.insert(id, symbols.len());
        symbols.push(DocumentSymbol {
            name: summary.str(d.name).to_owned(),
            kind: d.kind,
            symbol: summary.symbol_id(id).ok(),
            detail: summary.str(d.signature).to_owned(),
            range,
            selection,
            parent: d.parent.and_then(|p| index_of.get(&p).copied()),
        });
    }
    Ok(DocumentSymbols {
        symbols,
        completeness,
        position_encoding: encoding,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_rank_exact_prefix_and_humps_when_names_are_matched() {
        // given
        let cases = [
            ("Billing", "Billing", Some((0, 0))),
            ("BillingService", "Billing", Some((1, 0))),
            ("billingService", "Billing", Some((2, 0))),
            ("NullPointerException", "NPE", Some((3, 0))),
            ("NullPointerException", "NuPoEx", Some((3, 0))),
            ("PremiumBilling", "Billing", Some((3, 1))),
            ("HTTPServer", "Server", Some((3, 1))),
            ("HTTPServer", "HS", Some((3, 0))),
            ("getName", "gN", Some((3, 0))),
            ("MAX_VALUE", "MV", Some((3, 0))),
            ("MAX_VALUE", "max_va", Some((2, 0))),
            ("Billing", "Bx", None),
            ("Cart", "Billing", None),
            ("PointerNull", "NP", None),
        ];
        for (name, query, want) in cases {
            // when
            let got = rank(name, query).map(|r| (r.tier, r.start));
            // then
            assert_eq!(got, want, "{name} for {query}");
        }
    }

    #[test]
    fn should_match_every_name_when_the_query_is_empty() {
        // given
        let names = ["Billing", "a", "MAX_VALUE"];
        // when
        let ranks: Vec<Option<Rank>> = names.iter().map(|n| rank(n, "")).collect();
        // then
        assert!(ranks.iter().all(Option::is_some));
        assert_eq!(rank("Billing", &"B".repeat(MAX_QUERY_BYTES + 1)), None);
    }
}
