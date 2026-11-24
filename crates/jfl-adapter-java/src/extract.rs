//! One file to its [`Indexed`]: parse, apply our queries' table in one walk, then three passes over
//! what it captured (declarations, reference sites, diagnostics). The extraction rules are the crate
//! documentation's; this module is their implementation.

use core::ops::ControlFlow;
use std::borrow::Cow;

use jfl_application::syntax_indexer::{
    GrammarId, Indexed, MAX_DIAGNOSTICS, SyntaxDiagnostic, SyntaxIndexError, SyntaxProblem,
    source_text, work_budget,
};
use jfl_domain::{
    Arity, ByteOffset, ByteRange, DeclId, DeclKind, DeclarationInput, Modifiers, RefShape,
    SummaryBuilder, SummaryError, erase_parameter_type_with, parameter_type_base,
};
use tree_sitter::{Node, ParseOptions, ParseState, Parser, Tree, TreeCursor};

use crate::grammar::Grammar;
use crate::source::{Translated, clamp_crlf, java_name};

/// The work one file may cost, parse and extraction together (plan §11): `work_budget(len)`
/// progress callbacks of 100 parser operations each, as units of one operation, and every step
/// of the extraction (a node walked, a comment or header byte scanned, a site or a local looked
/// at) charged to the same count. A file that spends it is refused as
/// [`SyntaxIndexError::TooMuchWork`]. The extraction is linear in the file: pinned by
/// `extract::tests`, which count these steps (never time) on inputs built to be quadratic.
pub(crate) struct Budget {
    used: core::cell::Cell<usize>,
    limit: usize,
}

/// Units per tree-sitter progress callback (it calls one per 100 parser operations).
const UNITS_PER_CALLBACK: usize = 100;

impl Budget {
    pub(crate) fn new(len: usize) -> Self {
        Self {
            used: core::cell::Cell::new(0),
            limit: work_budget(len).saturating_mul(UNITS_PER_CALLBACK),
        }
    }

    /// The units spent so far.
    pub(crate) fn used(&self) -> usize {
        self.used.get()
    }

    pub(crate) fn charge(&self, units: usize) {
        self.used.set(self.used.get().saturating_add(units));
    }

    /// Charges a scan of `node`'s children: what every child or field lookup of a node costs.
    fn scan(&self, node: Node<'_>) {
        self.charge(
            usize::try_from(node.child_count())
                .unwrap_or(usize::MAX)
                .saturating_add(1),
        );
    }

    pub(crate) fn check(&self) -> Result<(), SyntaxIndexError> {
        if self.used.get() > self.limit {
            Err(SyntaxIndexError::TooMuchWork)
        } else {
            Ok(())
        }
    }
}

/// A parser, reused for every file a thread indexes, with the grammar it was last given.
pub(crate) struct Worker {
    parser: Parser,
    grammar: GrammarId,
}

impl Worker {
    pub(crate) fn new() -> Self {
        Self {
            parser: Parser::new(),
            grammar: GrammarId {
                name: "",
                version: "",
            },
        }
    }
}

/// How a scope of the locals query bounds a local's binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScopeKind {
    /// A block, a callable, a lambda, a loop: locals declared in it bind uses in it.
    Local,
    /// A named type's body: a field there may shadow an outer local.
    Type,
    /// An anonymous class's body: a barrier, and its members have no symbol identity.
    Anonymous,
}

#[derive(Debug, Clone, Copy)]
struct Scope {
    start: usize,
    end: usize,
    kind: ScopeKind,
}

#[derive(Debug, Clone, Copy)]
struct LocalDef {
    start: usize,
    end: usize,
    binds: bool,
}

impl LocalDef {
    fn new(name: Node<'_>, binds: bool) -> Self {
        Self {
            start: name.start_byte(),
            end: name.end_byte(),
            binds,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct DeclCapture<'t> {
    kind: DeclKind,
    node: Node<'t>,
    name: Node<'t>,
    declarator: Option<Node<'t>>,
}

#[derive(Debug, Clone)]
struct RefCandidate {
    start: usize,
    end: usize,
    rank: u8,
    shape: RefShape,
    arity: Arity,
    name: Option<String>,
    /// A place no local can be bound at, whatever locals are in scope: an annotation element's
    /// key, a bare `case` label.
    never_local: bool,
}

/// What the walk captured, in document order.
#[derive(Default)]
struct Captured<'t> {
    decls: Vec<DeclCapture<'t>>,
    package: Option<Node<'t>>,
    imports: Vec<Node<'t>>,
    comments: Vec<(usize, usize)>,
    refs: Vec<RefCandidate>,
    skips: Vec<usize>,
    templates: Vec<(usize, usize)>,
    scopes: Vec<Scope>,
    locals: Vec<LocalDef>,
}

/// What indexing one file spent of its work budget, in units of one parser operation or one
/// extraction step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Work {
    /// Units the extraction (everything after the parse) spent.
    pub extraction: usize,
    /// Units spent in all.
    pub used: usize,
    /// The file's budget.
    pub limit: usize,
}

/// Indexes `bytes` with `grammar` on `worker`, and says what of its budget that spent.
pub(crate) fn index_counted(
    grammar: &Grammar,
    worker: &mut Worker,
    bytes: &[u8],
) -> (Result<Indexed, SyntaxIndexError>, Work) {
    let text = match source_text(bytes) {
        Ok(text) => text,
        Err(e) => return (Err(e), Work::default()),
    };
    let budget = Budget::new(text.len());
    let mut parsed = 0;
    let result = index_within(grammar, worker, text, &budget, &mut parsed);
    let used = budget.used.get();
    let work = Work {
        extraction: used.saturating_sub(parsed),
        used,
        limit: budget.limit,
    };
    (result, work)
}

fn index_within(
    grammar: &Grammar,
    worker: &mut Worker,
    text: &str,
    budget: &Budget,
    parsed: &mut usize,
) -> Result<Indexed, SyntaxIndexError> {
    let translated = Translated::new(text);
    let src = translated.text.as_bytes();
    budget.charge(text.len());
    let tree = parse(grammar, worker, src, budget)?;
    *parsed = budget.used.get();
    let root = tree.root_node();
    let mut captured = run_query(grammar, root, src, budget)?;
    sort_scopes(&mut captured.scopes);
    captured
        .decls
        .sort_by_key(|d| (d.node.start_byte(), d.name.start_byte()));
    let mut comments = core::mem::take(&mut captured.comments);
    comments.sort_unstable();
    let file = File {
        grammar,
        original: text.as_bytes(),
        translated: &translated,
        budget,
        comments: &comments,
    };
    let mut builder = SummaryBuilder::new();
    file.package_and_imports(&captured, &comments, &mut builder)?;
    file.declarations(&captured, &comments, &mut builder)?;
    let refs = core::mem::take(&mut captured.refs);
    file.references(refs, &captured, &mut builder)?;
    let (diagnostics, diagnostics_dropped) = file.diagnostics(root, &captured.templates)?;
    budget.check()?;
    Ok(Indexed {
        summary: builder.finish(),
        diagnostics,
        diagnostics_dropped,
    })
}

pub(crate) fn parse(
    grammar: &Grammar,
    worker: &mut Worker,
    src: &[u8],
    budget: &Budget,
) -> Result<Tree, SyntaxIndexError> {
    let parser = &mut worker.parser;
    if worker.grammar != grammar.id {
        parser
            .set_language(&grammar.language)
            .map_err(|_| SyntaxIndexError::Interrupted)?;
        worker.grammar = grammar.id;
    }
    let mut progress = |_: &ParseState| {
        budget.charge(UNITS_PER_CALLBACK);
        if budget.check().is_err() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let options = ParseOptions::new().progress_callback(&mut progress);
    let tree = parser.parse_with_options(
        &mut |at: usize, _| src.get(at..).unwrap_or(&[]),
        None,
        Some(options),
    );
    // A cancelled parse leaves the parser mid-way: the next file must start afresh.
    tree.ok_or_else(|| {
        parser.reset();
        SyntaxIndexError::TooMuchWork
    })
}

/// Applies the queries' table (kind to captures, `grammar::capture_table`) to every node in one
/// pre-order cursor walk: the same captures tree-sitter's query engine would give single-node
/// patterns, at the cost of the walk alone.
fn run_query<'t>(
    grammar: &Grammar,
    root: Node<'t>,
    src: &[u8],
    budget: &Budget,
) -> Result<Captured<'t>, SyntaxIndexError> {
    let mut out = Captured::default();
    let mut walker = root.walk();
    let mut cursor = root.walk();
    // The open class, enum and record declarations, innermost last: `this(...)` and `super(...)`
    // are named by the innermost (a `parent()` walk would cost a scan of siblings per level).
    let mut owners: Vec<Node<'t>> = Vec::new();
    loop {
        let node = cursor.node();
        budget.charge(1);
        while owners
            .last()
            .is_some_and(|o| o.end_byte() <= node.start_byte())
        {
            owners.pop();
        }
        if node.is_named() {
            if is_constructor_owner(grammar, node) {
                owners.push(node);
            }
            if let Some(captures) = grammar.by_kind.get(usize::from(node.kind_id())) {
                for &capture in captures {
                    let site = Site {
                        node,
                        owner: owners.last().copied(),
                    };
                    budget.scan(node);
                    capture_node(grammar, capture, site, src, &mut walker, &mut out, budget);
                }
            }
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                budget.charge(out.refs.len().saturating_add(out.decls.len()));
                budget.check()?;
                return Ok(out);
            }
        }
        budget.check()?;
    }
}

/// A captured node and the innermost class, enum or record declaration around it.
#[derive(Clone, Copy)]
struct Site<'t> {
    node: Node<'t>,
    owner: Option<Node<'t>>,
}

/// The children of `node` in field `field` (a field or local declaration's declarators).
fn declarators<'t>(node: Node<'t>, field: u16, walker: &mut TreeCursor<'t>) -> Vec<Node<'t>> {
    core::num::NonZero::new(field)
        .map(|field| node.children_by_field_id(field, walker).collect())
        .unwrap_or_default()
}

/// Records what capture `at` of `node` means: the queries' patterns are single nodes, and the
/// names and fields they stand for are read here through the start-up table.
fn capture_node<'t>(
    grammar: &Grammar,
    at: u32,
    site: Site<'t>,
    src: &[u8],
    walker: &mut TreeCursor<'t>,
    out: &mut Captured<'t>,
    budget: &Budget,
) {
    let node = site.node;
    let c = &grammar.captures;
    let f = &grammar.fields;
    let k = &grammar.kinds;
    let span = |n: Node<'_>| (n.start_byte(), n.end_byte());
    if let Some(kind) = decl_kind(c, at) {
        declarations_of(grammar, kind, node, walker, out, budget);
    } else if at == c.package {
        out.package = node
            .named_children(walker)
            .find(|n| n.kind_id() == k.identifier || n.kind_id() == k.scoped_identifier);
    } else if at == c.import {
        out.imports.push(node);
    } else if at == c.comment {
        out.comments.push(span(node));
    } else if at == c.skip_label {
        if let Some(label) = node
            .named_children(walker)
            .find(|n| n.kind_id() == k.identifier)
        {
            out.skips.push(label.start_byte());
        }
    } else if at == c.skip_type_parameter {
        if let Some(name) = node
            .named_children(walker)
            .find(|n| n.kind_id() == k.type_identifier)
        {
            out.skips.push(name.start_byte());
        }
    } else if at == c.template {
        out.templates.push(span(node));
    } else if at == c.scope || at == c.scope_callable || at == c.scope_type {
        let kind = if at == c.scope_type {
            ScopeKind::Type
        } else {
            ScopeKind::Local
        };
        out.scopes.push(Scope {
            start: node.start_byte(),
            end: node.end_byte(),
            kind,
        });
        if at == c.scope_callable
            && let Some(params) = node.child_by_field_id(f.parameters)
        {
            parameter_names(grammar, params, walker, budget, |_, name| {
                out.locals.push(LocalDef::new(name, true));
            });
        }
    } else if at == c.def_var {
        if node.kind_id() == k.identifier {
            return;
        }
        let declarators = declarators(node, f.declarator, walker);
        if declarators.is_empty()
            && let Some(name) = node.child_by_field_id(f.name)
        {
            out.locals.push(LocalDef::new(name, true));
        }
        for declarator in declarators {
            budget.scan(declarator);
            if let Some(name) = declarator.child_by_field_id(f.name) {
                out.locals.push(LocalDef::new(name, true));
            }
        }
    } else if at == c.def_parameter {
        if let Some(name) = node.child_by_field_id(f.name) {
            out.locals.push(LocalDef::new(name, true));
        }
    } else if at == c.def_pattern {
        let name = node.child_by_field_id(f.name).or_else(|| {
            let count = u32::try_from(node.named_child_count()).ok()?;
            node.named_child(count.checked_sub(1)?)
        });
        if let Some(name) = name.filter(|n| n.kind_id() == k.identifier) {
            out.locals.push(LocalDef::new(name, false));
        }
    } else {
        if at == c.ref_new
            && let Some(body) = node
                .named_children(walker)
                .find(|n| n.kind_id() == k.class_body)
        {
            out.scopes.push(Scope {
                start: body.start_byte(),
                end: body.end_byte(),
                kind: ScopeKind::Anonymous,
            });
        }
        if at == c.ref_case_label {
            let labels: Vec<Node<'t>> = node
                .named_children(walker)
                .filter(|n| n.kind_id() == k.identifier)
                .collect();
            for label in labels {
                let mut site = candidate(label, RANK_FIELD, RefShape::Name, Arity(0));
                site.never_local = true;
                out.refs.push(site);
            }
        } else if let Some(candidate) = reference(grammar, at, site, src, walker, budget) {
            out.refs.push(candidate);
        }
    }
}

/// The names a parameter list declares: a method's or a lambda's `formal_parameters`, a lambda's
/// `inferred_parameters` or its lone identifier. A receiver parameter declares none.
fn parameter_names<'t>(
    grammar: &Grammar,
    params: Node<'t>,
    walker: &mut TreeCursor<'t>,
    budget: &Budget,
    mut found: impl FnMut(Node<'t>, Node<'t>),
) {
    budget.scan(params);
    let k = &grammar.kinds;
    let kind = params.kind_id();
    if kind == k.identifier {
        found(params, params);
        return;
    }
    let children: Vec<Node<'t>> = params.named_children(walker).collect();
    for child in children {
        budget.scan(child);
        let kind = child.kind_id();
        if kind == k.identifier {
            found(child, child);
        } else if kind == k.formal_parameter {
            if let Some(name) = child.child_by_field_id(grammar.fields.name) {
                found(child, name);
            }
        } else if kind == k.spread_parameter {
            let declarator = child
                .named_children(walker)
                .find(|n| n.kind_id() == k.variable_declarator);
            if let Some(name) = declarator.and_then(|d| d.child_by_field_id(grammar.fields.name)) {
                found(child, name);
            }
        }
    }
}

/// The declarations one declaration capture stands for: one per field declarator, and a record
/// with its components.
fn declarations_of<'t>(
    grammar: &Grammar,
    kind: DeclKind,
    node: Node<'t>,
    walker: &mut TreeCursor<'t>,
    out: &mut Captured<'t>,
    budget: &Budget,
) {
    let f = &grammar.fields;
    if kind == DeclKind::Field {
        let declarators = declarators(node, f.declarator, walker);
        for declarator in declarators {
            budget.scan(declarator);
            if let Some(name) = declarator.child_by_field_id(f.name) {
                out.decls.push(DeclCapture {
                    kind,
                    node,
                    name,
                    declarator: Some(declarator),
                });
            }
        }
        return;
    }
    let Some(name) = node.child_by_field_id(f.name) else {
        return;
    };
    out.decls.push(DeclCapture {
        kind,
        node,
        name,
        declarator: None,
    });
    if kind == DeclKind::EnumConstant
        && let Some(body) = node.child_by_field_id(f.body)
    {
        out.scopes.push(Scope {
            start: body.start_byte(),
            end: body.end_byte(),
            kind: ScopeKind::Anonymous,
        });
    }
    if kind == DeclKind::Record
        && let Some(params) = node.child_by_field_id(f.parameters)
    {
        let mut components = Vec::new();
        parameter_names(grammar, params, walker, budget, |component, name| {
            components.push((component, name));
        });
        for (component, name) in components {
            out.decls.push(DeclCapture {
                kind: DeclKind::RecordComponent,
                node: component,
                name,
                declarator: None,
            });
        }
    }
}

fn decl_kind(c: &crate::grammar::Captures, at: u32) -> Option<DeclKind> {
    [
        (c.class, DeclKind::Class),
        (c.interface, DeclKind::Interface),
        (c.enum_, DeclKind::Enum),
        (c.record, DeclKind::Record),
        (c.annotation_type, DeclKind::Annotation),
        (c.method, DeclKind::Method),
        (c.constructor, DeclKind::Constructor),
        (c.field, DeclKind::Field),
        (c.enum_constant, DeclKind::EnumConstant),
        (c.annotation_element, DeclKind::AnnotationElement),
    ]
    .into_iter()
    .find_map(|(index, kind)| (index == at).then_some(kind))
}

/// The ranks of shapes captured on one node: the most specific wins.
const RANK_NAME: u8 = 0;
const RANK_TYPE: u8 = 1;
const RANK_FIELD: u8 = 2;
const RANK_SPECIFIC: u8 = 3;

fn candidate(node: Node<'_>, rank: u8, shape: RefShape, arity: Arity) -> RefCandidate {
    RefCandidate {
        start: node.start_byte(),
        end: node.end_byte(),
        rank,
        shape,
        arity,
        name: None,
        never_local: false,
    }
}

/// The reference site a reference capture makes, if any.
fn reference<'t>(
    grammar: &Grammar,
    at: u32,
    site: Site<'t>,
    src: &[u8],
    walker: &mut TreeCursor<'t>,
    budget: &Budget,
) -> Option<RefCandidate> {
    let node = site.node;
    let c = &grammar.captures;
    let f = &grammar.fields;
    let k = &grammar.kinds;
    if at == c.ref_call {
        let name = node.child_by_field_id(f.name)?;
        let shape = if node.child_by_field_id(f.object).is_some() {
            RefShape::QualifiedCall
        } else {
            RefShape::Call
        };
        let arity = arity(grammar, node.child_by_field_id(f.arguments), walker, budget);
        Some(candidate(name, RANK_SPECIFIC, shape, arity))
    } else if at == c.ref_new {
        let name = rightmost_type_name(grammar, node.child_by_field_id(f.type_)?)?;
        let arity = arity(grammar, node.child_by_field_id(f.arguments), walker, budget);
        Some(candidate(name, RANK_SPECIFIC, RefShape::New, arity))
    } else if at == c.ref_constructor {
        budget.scan(site.owner?);
        constructor_call(grammar, node, site.owner?, src, walker, budget)
    } else if at == c.ref_method_ref {
        let last = node.child(node.child_count().checked_sub(1)?)?;
        let name = if last.kind_id() == k.identifier {
            last
        } else if last.kind_id() == k.new_kw {
            rightmost_type_name(grammar, node.named_child(0)?)?
        } else {
            return None;
        };
        Some(candidate(
            name,
            RANK_SPECIFIC,
            RefShape::MethodRef,
            Arity(0),
        ))
    } else if at == c.ref_field {
        let field = node
            .child_by_field_id(f.field)
            .filter(|n| n.kind_id() == k.identifier)?;
        Some(candidate(
            field,
            RANK_FIELD,
            RefShape::QualifiedName,
            Arity(0),
        ))
    } else if at == c.ref_annotation {
        let name = last_identifier(grammar, node.child_by_field_id(f.name)?)?;
        Some(candidate(
            name,
            RANK_SPECIFIC,
            RefShape::Annotation,
            Arity(0),
        ))
    } else if at == c.ref_type {
        // `var` is not a type since Java 10: it names no declaration.
        (src.get(node.byte_range()) != Some(&b"var"[..]))
            .then(|| candidate(node, RANK_TYPE, RefShape::TypeUse, Arity(0)))
    } else if at == c.ref_name {
        Some(candidate(node, RANK_NAME, RefShape::Name, Arity(0)))
    } else if at == c.ref_element_key {
        let key = node
            .child_by_field_id(f.key)
            .filter(|n| n.kind_id() == k.identifier)?;
        let mut site = candidate(key, RANK_FIELD, RefShape::Name, Arity(0));
        site.never_local = true;
        Some(site)
    } else {
        None
    }
}

/// `this(...)` and `super(...)` name a constructor of the enclosing type or of its superclass:
/// a reference site by that type's name, so a constructor's references include them.
fn constructor_call<'t>(
    grammar: &Grammar,
    node: Node<'t>,
    owner: Node<'t>,
    src: &[u8],
    walker: &mut TreeCursor<'t>,
    budget: &Budget,
) -> Option<RefCandidate> {
    let f = &grammar.fields;
    let keyword = node.child_by_field_id(f.constructor)?;
    let is_super = keyword.kind_id() == grammar.kinds.super_;
    let named = if is_super {
        let superclass = owner.child_by_field_id(f.superclass)?;
        let ty = first_named(grammar, superclass, walker)?;
        rightmost_type_name(grammar, ty)?
    } else {
        owner.child_by_field_id(f.name)?
    };
    let name = std::str::from_utf8(src.get(named.byte_range())?).ok()?;
    let arity = arity(grammar, node.child_by_field_id(f.arguments), walker, budget);
    let mut site = candidate(keyword, RANK_SPECIFIC, RefShape::New, arity);
    site.name = Some(java_name(name).into_owned());
    Some(site)
}

/// A type declaration that may declare a constructor: a class, an enum or a record.
fn is_constructor_owner(grammar: &Grammar, node: Node<'_>) -> bool {
    let k = &grammar.kinds;
    let kind = node.kind_id();
    kind == k.class_declaration || kind == k.enum_declaration || kind == k.record_declaration
}

/// The simple name of a type as written: `C` of `a.b.C<T>` and of `@A C` (an annotated type,
/// as in `extends @A C`: a `super(…)` there names `C`, IDX-11's review).
///
/// Pinned by: `extract::tests::should_name_the_superclass_when_super_is_called_on_an_annotated_type`.
fn rightmost_type_name<'t>(grammar: &Grammar, mut node: Node<'t>) -> Option<Node<'t>> {
    let k = &grammar.kinds;
    // Each step goes one level down; a type's nesting is bounded by the file's.
    for _ in 0..1024 {
        let kind = node.kind_id();
        // Each step reads one node's children; the 1024 steps are a fixed bound.
        if kind == k.type_identifier || kind == k.identifier {
            return Some(node);
        }
        node = if kind == k.generic_type {
            node.named_child(0)?
        } else if kind == k.annotated_type {
            // The annotations come first; the type is the last child that is not a comment.
            (0..node.named_child_count())
                .rev()
                .filter_map(|i| node.named_child(u32::try_from(i).ok()?))
                .find(|n| !is_comment(grammar, *n))?
        } else if kind == k.scoped_type_identifier || kind == k.scoped_identifier {
            node.named_child(u32::try_from(node.named_child_count().checked_sub(1)?).ok()?)?
        } else {
            return None;
        };
    }
    None
}

/// The last identifier of a name: `C` of `a.b.C`.
fn last_identifier<'t>(grammar: &Grammar, node: Node<'t>) -> Option<Node<'t>> {
    if node.kind_id() == grammar.kinds.scoped_identifier {
        node.child_by_field_id(grammar.fields.name)
    } else if node.kind_id() == grammar.kinds.identifier {
        Some(node)
    } else {
        None
    }
}

fn is_comment(grammar: &Grammar, node: Node<'_>) -> bool {
    let kind = node.kind_id();
    kind == grammar.kinds.block_comment || kind == grammar.kinds.line_comment
}

fn first_named<'t>(
    grammar: &Grammar,
    node: Node<'t>,
    walker: &mut TreeCursor<'t>,
) -> Option<Node<'t>> {
    node.named_children(walker)
        .find(|n| !is_comment(grammar, *n))
}

/// The number of arguments of an argument list, comments left out.
fn arity<'t>(
    grammar: &Grammar,
    list: Option<Node<'t>>,
    walker: &mut TreeCursor<'t>,
    budget: &Budget,
) -> Arity {
    list.map_or(Arity(0), |list| {
        budget.scan(list);
        Arity::of(
            list.named_children(walker)
                .filter(|n| !is_comment(grammar, *n))
                .count(),
        )
    })
}

/// One file's texts, for the passes.
struct File<'a> {
    grammar: &'a Grammar,
    /// The file as written (after a byte-order mark): the text the ranges are of.
    original: &'a [u8],
    translated: &'a Translated<'a>,
    budget: &'a Budget,
    /// The comments, sorted.
    comments: &'a [(usize, usize)],
}

/// What every declarator of one declaration shares, computed once per declaration node: a field
/// with many declarators must not re-read its annotations, Javadoc and type per declarator.
struct Shared {
    node: usize,
    modifiers: Modifiers,
    annotations: Vec<String>,
    javadoc: Option<ByteRange>,
    /// A field's head (`private final int`), for each declarator's signature.
    head: String,
}

impl File<'_> {
    fn text(&self, start: usize, end: usize) -> &str {
        self.translated.text.get(start..end).unwrap_or("")
    }

    fn node_text(&self, node: Node<'_>) -> &str {
        self.text(node.start_byte(), node.end_byte())
    }

    /// A range of the parsed text as a range of the file as written.
    fn range(&self, start: usize, end: usize) -> Result<ByteRange, SyntaxIndexError> {
        let (start, end) = if self.translated.map.is_identity() {
            (start, end)
        } else {
            (
                self.translated.map.start(start),
                self.translated.map.end(end),
            )
        };
        let (start, end) = clamp_crlf(self.original, start, end);
        let offset = |at: usize| {
            ByteOffset::try_from_usize(at)
                .map_err(|_| SyntaxIndexError::Summary(SummaryError::TooLarge))
        };
        ByteRange::new(offset(start)?, offset(end)?)
            .map_err(|_| SyntaxIndexError::Summary(SummaryError::TooLarge))
    }

    fn node_range(&self, node: Node<'_>) -> Result<ByteRange, SyntaxIndexError> {
        self.range(node.start_byte(), node.end_byte())
    }

    /// `start..end` as written, with the comments of `comments` left out, white space collapsed
    /// and identifier-ignorable characters dropped.
    fn clean(&self, start: usize, end: usize, comments: &[(usize, usize)]) -> String {
        self.budget.charge(end.saturating_sub(start));
        let mut out = String::new();
        let mut at = start;
        let first = comments.partition_point(|&(_, e)| e <= start);
        let pieces = comments
            .get(first..)
            .unwrap_or(&[])
            .iter()
            .take_while(|&&(s, _)| s < end)
            .map(|&(s, e)| {
                self.budget.charge(1);
                (s.max(start), e.min(end))
            })
            .chain(core::iter::once((end, end)));
        for (comment_start, comment_end) in pieces {
            for word in self.text(at, comment_start.max(at)).split_whitespace() {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(word);
            }
            at = comment_end.max(at);
        }
        java_name(&out).into_owned()
    }

    fn package_and_imports(
        &self,
        captured: &Captured<'_>,
        comments: &[(usize, usize)],
        builder: &mut SummaryBuilder,
    ) -> Result<(), SyntaxIndexError> {
        let k = &self.grammar.kinds;
        if let Some(package) = captured.package {
            let name = self.dotted(package, comments);
            builder.package(&name)?;
        }
        let mut walker = match captured.imports.first() {
            Some(first) => first.walk(),
            None => return Ok(()),
        };
        for &import in &captured.imports {
            self.budget.scan(import);
            let mut is_static = false;
            let mut module = false;
            let mut on_demand = false;
            let mut name = None;
            for child in import.children(&mut walker) {
                let kind = child.kind_id();
                if kind == k.static_kw {
                    is_static = true;
                } else if kind == k.module_kw {
                    module = true;
                } else if kind == k.asterisk {
                    on_demand = true;
                } else if kind == k.identifier || kind == k.scoped_identifier {
                    name = Some(child);
                }
            }
            let Some(name) = name else { continue };
            let dotted = self.dotted(name, comments);
            let range = self.node_range(import)?;
            if module {
                builder.module_import(&dotted, range)?;
            } else {
                builder.import(&dotted, is_static, on_demand, range)?;
            }
        }
        Ok(())
    }

    /// A dotted name as written, without white space or comments.
    fn dotted(&self, node: Node<'_>, comments: &[(usize, usize)]) -> String {
        self.clean(node.start_byte(), node.end_byte(), comments)
            .split_whitespace()
            .collect()
    }

    fn javadoc(&self, start: usize, comments: &[(usize, usize)]) -> Option<(usize, usize)> {
        let mut cursor = start;
        let mut index = comments.partition_point(|&(_, e)| e <= start);
        while let Some(previous) = index.checked_sub(1) {
            let &(s, e) = comments.get(previous)?;
            // From the declaration backwards, so a long gap stops at its first non-blank.
            let mut scanned = 0_usize;
            let blank = self.text(e, cursor).chars().rev().all(|c| {
                scanned = scanned.saturating_add(1);
                c.is_whitespace()
            });
            self.budget.charge(scanned.saturating_add(1));
            if !blank {
                return None;
            }
            let body = self.text(s, e);
            if body.starts_with("/**") && body != "/**/" {
                return Some((s, e));
            }
            cursor = s;
            index = previous;
        }
        None
    }

    fn declarations(
        &self,
        captured: &Captured<'_>,
        comments: &[(usize, usize)],
        builder: &mut SummaryBuilder,
    ) -> Result<(), SyntaxIndexError> {
        let decls = &captured.decls;
        let scopes = &captured.scopes;
        let mut scope_stack: Vec<Scope> = Vec::new();
        let mut next_scope = 0_usize;
        // Enclosing type declarations: (end, kept id, node, type parameters).
        let mut types: Vec<OpenType<'_>> = Vec::new();
        let Some(first) = decls.first() else {
            return Ok(());
        };
        let mut walker = first.node.walk();
        let mut shared: Option<Shared> = None;
        let mut scope = TypeScope::default();
        for decl in decls {
            self.budget.charge(1);
            self.budget.check()?;
            let (start, end) = (decl.node.start_byte(), decl.node.end_byte());
            while let Some(scope) = scopes.get(next_scope).copied() {
                if scope.start < start || (scope.start == start && scope.end > end) {
                    pop_ended(&mut scope_stack, scope.start);
                    scope_stack.push(scope);
                    next_scope = next_scope.saturating_add(1);
                } else {
                    break;
                }
            }
            pop_ended(&mut scope_stack, start);
            while let Some(open) = types.pop_if(|t| t.end <= start) {
                scope.pop(open.pushed);
            }
            // A declaration in a block, a lambda or an anonymous body is local: it has no
            // symbol identity, and neither has anything inside it.
            let local = scope_stack
                .last()
                .is_some_and(|s| s.kind != ScopeKind::Type);
            let parent = match types.last() {
                Some(open) => open.id,
                None => None,
            };
            let orphan = types.last().is_some_and(|open| open.id.is_none());
            let skipped = local || orphan;
            if skipped {
                if decl.kind.is_type() {
                    types.push(OpenType {
                        end,
                        id: None,
                        node: decl.node,
                        pushed: 0,
                    });
                }
                continue;
            }
            if shared.as_ref().is_none_or(|s| s.node != decl.node.id()) {
                shared = Some(self.shared(decl, comments, &mut walker)?);
            }
            let Some(common) = shared.as_ref() else {
                continue;
            };
            // Each declarator stores its own copy of the shared head and annotation names, so a
            // field with many of both is charged for every copy, and cut off when that is too much.
            self.budget
                .charge(common.head.len().saturating_add(common.annotations.len()));
            self.budget.check()?;
            let entry = self.declaration(decl, common, parent, &types, &mut scope, &mut walker)?;
            self.budget.charge(
                entry
                    .params
                    .len()
                    .saturating_add(entry.supertypes.len())
                    .saturating_add(entry.permits.len())
                    .saturating_add(entry.annotations.len()),
            );
            self.budget.check()?;
            let owned = entry.input();
            let id = builder.declaration(&owned.input())?;
            if decl.kind.is_type() {
                // A type's own parameters stay in scope for its members until it closes.
                let pushed = scope.push(&entry.type_params, self.budget);
                types.push(OpenType {
                    end,
                    id: Some(id),
                    node: decl.node,
                    pushed,
                });
            }
        }
        Ok(())
    }

    /// The parts of `decl` its declaration node's other declarators share.
    fn shared<'t>(
        &self,
        decl: &DeclCapture<'t>,
        comments: &[(usize, usize)],
        walker: &mut TreeCursor<'t>,
    ) -> Result<Shared, SyntaxIndexError> {
        let node = decl.node;
        let (modifiers, annotations) = self.modifiers(node, walker);
        let javadoc = self
            .javadoc(node.start_byte(), comments)
            .map(|(s, e)| self.range(s, e))
            .transpose()?;
        let head = if decl.kind == DeclKind::Field {
            let head_end = node
                .child_by_field_id(self.grammar.fields.type_)
                .map_or(node.start_byte(), |t| t.end_byte());
            self.clean(node.start_byte(), head_end, comments)
        } else {
            String::new()
        };
        Ok(Shared {
            node: node.id(),
            modifiers,
            annotations,
            javadoc,
            head,
        })
    }

    fn declaration<'t>(
        &self,
        decl: &DeclCapture<'t>,
        shared: &Shared,
        parent: Option<DeclId>,
        types: &[OpenType<'t>],
        scope: &mut TypeScope,
        walker: &mut TreeCursor<'t>,
    ) -> Result<Entry, SyntaxIndexError> {
        let comments = self.comments;
        let f = &self.grammar.fields;
        let k = &self.grammar.kinds;
        let node = decl.node;
        let modifiers = shared.modifiers;
        let callable = matches!(decl.kind, DeclKind::Method | DeclKind::Constructor);
        let own_params = if decl.kind.is_type() || callable {
            self.type_params(node.child_by_field_id(f.type_parameters), comments, walker)
        } else {
            Vec::new()
        };
        let signature = self.signature(decl, &shared.head, comments)?;
        let (params, varargs) = match decl.kind {
            DeclKind::Method | DeclKind::Constructor => {
                let list = node.child_by_field_id(f.parameters).or_else(|| {
                    // A compact constructor's parameters are its record's components.
                    types
                        .last()
                        .and_then(|open| open.node.child_by_field_id(f.parameters))
                });
                // The callable's own type parameters, innermost, for its parameters only.
                let pushed = scope.push(&own_params, self.budget);
                let erased = self.parameters(list, scope, comments, walker);
                scope.pop(pushed);
                erased
            }
            _ => (Vec::new(), false),
        };
        let modifiers = if varargs {
            modifiers.with(Modifiers::VARARGS)
        } else {
            modifiers
        };
        let erased = |list: Vec<Node<'_>>| -> Vec<String> {
            list.into_iter()
                .map(|n| {
                    self.erase(
                        n.start_byte(),
                        n.end_byte(),
                        &TypeScope::default(),
                        comments,
                    )
                })
                .collect()
        };
        let mut supertypes = Vec::new();
        let mut permits = Vec::new();
        if decl.kind.is_type() {
            if let Some(superclass) = node.child_by_field_id(f.superclass)
                && let Some(ty) = first_named(self.grammar, superclass, walker)
            {
                supertypes.push(ty);
            }
            if let Some(list) = node.child_by_field_id(f.interfaces) {
                supertypes.extend(self.type_list(list, walker));
            }
            self.budget.scan(node);
            let extends = node
                .named_children(walker)
                .find(|n| n.kind_id() == k.extends_interfaces);
            if let Some(list) = extends {
                supertypes.extend(self.type_list(list, walker));
            }
            if let Some(list) = node.child_by_field_id(f.permits) {
                permits.extend(self.type_list(list, walker));
            }
        }
        Ok(Entry {
            kind: decl.kind,
            modifiers,
            parent,
            name: java_name(self.node_text(decl.name)).into_owned(),
            name_range: self.node_range(decl.name)?,
            range: self.node_range(node)?,
            signature,
            params,
            supertypes: erased(supertypes),
            permits: erased(permits),
            annotations: shared.annotations.clone(),
            javadoc: shared.javadoc,
            type_params: own_params,
        })
    }

    fn type_list<'t>(&self, holder: Node<'t>, walker: &mut TreeCursor<'t>) -> Vec<Node<'t>> {
        let k = &self.grammar.kinds;
        self.budget.scan(holder);
        let list = holder
            .named_children(walker)
            .find(|n| n.kind_id() == k.type_list);
        list.map(|list| {
            self.budget.scan(list);
            list.named_children(walker)
                .filter(|n| !is_comment(self.grammar, *n))
                .collect()
        })
        .unwrap_or_default()
    }

    /// The modifiers as written and the annotations' names. Orchard wraps the keywords in
    /// `visibility` and `modifier` nodes and upstream does not, so a keyword is read by its text.
    fn modifiers<'t>(
        &self,
        node: Node<'t>,
        walker: &mut TreeCursor<'t>,
    ) -> (Modifiers, Vec<String>) {
        let k = &self.grammar.kinds;
        let mut modifiers = Modifiers::empty();
        let mut annotations = Vec::new();
        self.budget.scan(node);
        let Some(list) = node.children(walker).find(|n| n.kind_id() == k.modifiers) else {
            return (modifiers, annotations);
        };
        self.budget.scan(list);
        for child in list.children(walker) {
            let kind = child.kind_id();
            if kind == k.marker_annotation || kind == k.annotation {
                if let Some(name) = child.child_by_field_id(self.grammar.fields.name) {
                    let text: String = self.node_text(name).split_whitespace().collect();
                    annotations.push(java_name(&text).into_owned());
                }
                continue;
            }
            let bit = match self.node_text(child).trim() {
                "public" => Modifiers::PUBLIC,
                "protected" => Modifiers::PROTECTED,
                "private" => Modifiers::PRIVATE,
                "static" => Modifiers::STATIC,
                "final" => Modifiers::FINAL,
                "abstract" => Modifiers::ABSTRACT,
                "default" => Modifiers::DEFAULT,
                "sealed" => Modifiers::SEALED,
                "non-sealed" => Modifiers::NON_SEALED,
                "synchronized" => Modifiers::SYNCHRONIZED,
                "native" => Modifiers::NATIVE,
                "transient" => Modifiers::TRANSIENT,
                "volatile" => Modifiers::VOLATILE,
                "strictfp" => Modifiers::STRICTFP,
                _ => Modifiers::empty(),
            };
            modifiers = modifiers.with(bit);
        }
        (modifiers, annotations)
    }

    /// The type parameters a declaration introduces: each name and its first bound as written.
    fn type_params<'t>(
        &self,
        list: Option<Node<'t>>,
        comments: &[(usize, usize)],
        walker: &mut TreeCursor<'t>,
    ) -> Vec<(String, Option<String>)> {
        let k = &self.grammar.kinds;
        let Some(list) = list else {
            return Vec::new();
        };
        self.budget.scan(list);
        let params: Vec<Node<'t>> = list
            .named_children(walker)
            .filter(|n| n.kind_id() == k.type_parameter)
            .collect();
        params
            .into_iter()
            .filter_map(|param| {
                self.budget.scan(param);
                let children: Vec<Node<'t>> = param.named_children(walker).collect();
                let name = children.iter().find(|n| n.kind_id() == k.type_identifier)?;
                let bound = children
                    .iter()
                    .find(|n| n.kind_id() == k.type_bound)
                    .and_then(|b| first_named(self.grammar, *b, walker))
                    .map(|b| self.clean(b.start_byte(), b.end_byte(), comments));
                Some((java_name(self.node_text(*name)).into_owned(), bound))
            })
            .collect()
    }

    /// The parameter types, erased with `scope` (innermost first), and whether the last is
    /// variable-arity. A receiver parameter (`Foo this`) is not a parameter.
    fn parameters<'t>(
        &self,
        list: Option<Node<'t>>,
        scope: &TypeScope,
        comments: &[(usize, usize)],
        walker: &mut TreeCursor<'t>,
    ) -> (Vec<String>, bool) {
        let k = &self.grammar.kinds;
        let f = &self.grammar.fields;
        let Some(list) = list else {
            return (Vec::new(), false);
        };
        let mut out = Vec::new();
        let mut varargs = false;
        self.budget.scan(list);
        let params: Vec<Node<'t>> = list.named_children(walker).collect();
        for param in params {
            self.budget.scan(param);
            let kind = param.kind_id();
            if kind == k.formal_parameter {
                let Some(ty) = param.child_by_field_id(f.type_) else {
                    continue;
                };
                let mut text = self.clean(ty.start_byte(), ty.end_byte(), comments);
                if let Some(dims) = param.child_by_field_id(f.dimensions) {
                    text.push_str(&self.clean(dims.start_byte(), dims.end_byte(), comments));
                }
                out.push(self.erase_text(&text, scope));
                varargs = false;
            } else if kind == k.spread_parameter {
                let children: Vec<Node<'t>> = param.children(walker).collect();
                let ty = children.iter().find(|n| {
                    n.is_named() && n.kind_id() != k.modifiers && !is_comment(self.grammar, **n)
                });
                let ellipsis = children.iter().find(|n| n.kind_id() == k.ellipsis);
                if let (Some(ty), Some(ellipsis)) = (ty, ellipsis) {
                    let text = self.clean(ty.start_byte(), ellipsis.end_byte(), comments);
                    out.push(self.erase_text(&text, scope));
                    varargs = true;
                }
            }
        }
        (out, varargs)
    }

    fn erase(
        &self,
        start: usize,
        end: usize,
        scope: &TypeScope,
        comments: &[(usize, usize)],
    ) -> String {
        let text = self.clean(start, end, comments);
        self.erase_text(&text, scope)
    }

    /// `text` erased as a `SymbolId` writes it; as written (white space dropped) when it is not a
    /// type `erase_parameter_type` reads, so the declaration keeps its other parts and only its
    /// symbol identity is refused later.
    fn erase_text(&self, text: &str, scope: &TypeScope) -> String {
        self.budget.charge(text.len().saturating_add(1));
        erase_parameter_type_with(text, |name| scope.erasure(name))
            .unwrap_or_else(|_| text.split_whitespace().collect())
    }

    /// The header as written: from the declaration's start to its body, initializer or
    /// arguments, comments left out and white space collapsed, without a trailing `;`.
    fn signature(
        &self,
        decl: &DeclCapture<'_>,
        head: &str,
        comments: &[(usize, usize)],
    ) -> Result<String, SyntaxIndexError> {
        let f = &self.grammar.fields;
        let node = decl.node;
        let start = node.start_byte();
        let field = |n: u16| node.child_by_field_id(n).map(|c| c.start_byte());
        let text = match decl.kind {
            DeclKind::Field => {
                let declarator = decl.declarator.unwrap_or(decl.name);
                let tail_end = declarator
                    .child_by_field_id(f.dimensions)
                    .map_or(decl.name.end_byte(), |d| d.end_byte());
                let tail = self.clean(declarator.start_byte(), tail_end, comments);
                format!("{head} {tail}")
            }
            DeclKind::EnumConstant => {
                let end = field(f.arguments)
                    .or_else(|| field(f.body))
                    .unwrap_or(node.end_byte());
                self.clean(start, end, comments)
            }
            DeclKind::RecordComponent | DeclKind::AnnotationElement => {
                self.clean(start, node.end_byte(), comments)
            }
            _ => {
                let end = field(f.body).unwrap_or(node.end_byte());
                self.clean(start, end, comments)
            }
        };
        Ok(text.trim_end_matches(';').trim_end().to_owned())
    }

    fn references(
        &self,
        mut refs: Vec<RefCandidate>,
        captured: &Captured<'_>,
        builder: &mut SummaryBuilder,
    ) -> Result<(), SyntaxIndexError> {
        // Most specific shape first at each place, then one per place.
        refs.sort_by(|a, b| a.start.cmp(&b.start).then(b.rank.cmp(&a.rank)));
        refs.dedup_by_key(|r| r.start);
        let mut excluded: Vec<usize> = captured
            .decls
            .iter()
            .map(|d| d.name.start_byte())
            .chain(captured.locals.iter().map(|l| l.start))
            .chain(captured.skips.iter().copied())
            .collect();
        excluded.sort_unstable();
        let mut spans: Vec<(usize, usize)> = captured
            .imports
            .iter()
            .chain(captured.package.iter())
            .map(|n| (n.start_byte(), n.end_byte()))
            .collect();
        spans.sort_unstable();
        refs.retain(|r| excluded.binary_search(&r.start).is_err() && !in_spans(&spans, r.start));
        let refs = self.drop_locals(refs, captured);
        self.budget.check()?;
        let mut sites = refs;
        sites.extend(self.import_sites(captured));
        sites.sort_by_key(|r| r.start);
        for site in sites {
            self.budget.charge(1);
            let name: Cow<'_, str> = match &site.name {
                Some(name) => Cow::Borrowed(name.as_str()),
                None => java_name(self.text(site.start, site.end)),
            };
            if name.is_empty() {
                continue;
            }
            let arity = if site.shape.has_arity() {
                site.arity
            } else {
                Arity(0)
            };
            builder.reference(&name, self.range(site.start, site.end)?, site.shape, arity)?;
        }
        Ok(())
    }

    /// An import names its last segment: a use of that type or member.
    fn import_sites(&self, captured: &Captured<'_>) -> Vec<RefCandidate> {
        let k = &self.grammar.kinds;
        let mut out = Vec::new();
        for &import in &captured.imports {
            self.budget.scan(import);
            let mut walker = import.walk();
            let mut module = false;
            let mut last = None;
            for child in import.children(&mut walker) {
                if child.kind_id() == k.module_kw {
                    module = true;
                } else if let Some(id) = last_identifier(self.grammar, child) {
                    last = Some(id);
                }
            }
            if let (false, Some(id)) = (module, last) {
                out.push(candidate(
                    id,
                    RANK_SPECIFIC,
                    RefShape::QualifiedName,
                    Arity(0),
                ));
            }
        }
        out
    }

    /// Drops the bare names bound to a local variable or a parameter (plan §5.5): a use binds to
    /// the nearest enclosing scope that declares its name before it, unless a type body lies
    /// between them (a field there may shadow the local), and a use whose binding is unclear
    /// stays. A site no local can be bound at (`never_local`) always stays.
    ///
    /// Each name maps to the stack depths that declare it, so a use is one lookup, whatever the
    /// number of locals (a scan of every scope's names was quadratic on a hostile file).
    fn drop_locals(&self, refs: Vec<RefCandidate>, captured: &Captured<'_>) -> Vec<RefCandidate> {
        let scopes = &captured.scopes;
        let mut defs: Vec<LocalDef> = captured
            .locals
            .iter()
            .copied()
            .filter(|d| d.binds)
            .collect();
        defs.sort_by_key(|d| d.start);
        let mut locals = Locals::default();
        let mut next_scope = 0_usize;
        let mut next_def = 0_usize;
        let mut kept = Vec::with_capacity(refs.len());
        for site in refs {
            let at = site.start;
            self.budget.charge(1);
            loop {
                let scope = scopes.get(next_scope).copied().filter(|s| s.start <= at);
                let def = defs.get(next_def).copied().filter(|d| d.start < at);
                match (scope, def) {
                    (Some(scope), Some(def)) if scope.start <= def.start => {
                        locals.enter(scope);
                        next_scope = next_scope.saturating_add(1);
                    }
                    (_, Some(def)) => {
                        locals.leave_ended(def.start);
                        locals.declare(self.text(def.start, def.end));
                        next_def = next_def.saturating_add(1);
                    }
                    (Some(scope), None) => {
                        locals.enter(scope);
                        next_scope = next_scope.saturating_add(1);
                    }
                    (None, None) => break,
                }
                self.budget.charge(1);
            }
            locals.leave_ended(at);
            if site.shape == RefShape::Name
                && !site.never_local
                && locals.binds(self.text(site.start, site.end))
            {
                continue;
            }
            kept.push(site);
        }
        kept
    }

    fn diagnostics(
        &self,
        root: Node<'_>,
        templates: &[(usize, usize)],
    ) -> Result<(Vec<SyntaxDiagnostic>, u32), SyntaxIndexError> {
        let mut found: Vec<(usize, usize, SyntaxProblem)> = templates
            .iter()
            .map(|&(s, e)| (s, e, SyntaxProblem::StringTemplate))
            .collect();
        if root.has_error() {
            let mut cursor = root.walk();
            'walk: loop {
                let node = cursor.node();
                self.budget.charge(1);
                let descend = if node.is_error() {
                    found.push((
                        node.start_byte(),
                        node.end_byte(),
                        SyntaxProblem::Unexpected,
                    ));
                    false
                } else if node.is_missing() {
                    let what = if node.is_named() {
                        node.kind().replace('_', " ")
                    } else {
                        node.kind().to_owned()
                    };
                    found.push((
                        node.start_byte(),
                        node.start_byte(),
                        SyntaxProblem::Missing(what.into_boxed_str()),
                    ));
                    false
                } else {
                    node.has_error()
                };
                if descend && cursor.goto_first_child() {
                    continue;
                }
                while !cursor.goto_next_sibling() {
                    if !cursor.goto_parent() {
                        break 'walk;
                    }
                }
            }
        }
        found.sort_by_key(|&(s, e, _)| (s, e));
        let dropped =
            u32::try_from(found.len().saturating_sub(MAX_DIAGNOSTICS)).unwrap_or(u32::MAX);
        let diagnostics = found
            .into_iter()
            .take(MAX_DIAGNOSTICS)
            .map(|(s, e, problem)| {
                Ok(SyntaxDiagnostic {
                    range: self.range(s, e)?,
                    problem,
                })
            })
            .collect::<Result<Vec<_>, SyntaxIndexError>>()?;
        Ok((diagnostics, dropped))
    }
}

fn in_spans(spans: &[(usize, usize)], at: usize) -> bool {
    let index = spans.partition_point(|&(s, _)| s <= at);
    index
        .checked_sub(1)
        .and_then(|i| spans.get(i))
        .is_some_and(|&(_, e)| at < e)
}

/// The locals in scope while the reference sites are walked in order.
#[derive(Default)]
struct Locals<'n> {
    /// The open scopes, outermost first, each with the names it declared.
    stack: Vec<(Scope, Vec<&'n str>)>,
    /// For each name, the stack depths that declare it, innermost last.
    depths: std::collections::HashMap<&'n str, Vec<usize>>,
    /// The stack depths of the open type bodies, innermost last.
    barriers: Vec<usize>,
}

impl<'n> Locals<'n> {
    fn enter(&mut self, scope: Scope) {
        self.leave_ended(scope.start);
        if scope.kind != ScopeKind::Local {
            self.barriers.push(self.stack.len());
        }
        self.stack.push((scope, Vec::new()));
    }

    fn leave_ended(&mut self, at: usize) {
        while self.stack.last().is_some_and(|(s, _)| s.end <= at) {
            let Some((scope, names)) = self.stack.pop() else {
                break;
            };
            for name in names {
                if let Some(depths) = self.depths.get_mut(name) {
                    depths.pop();
                }
            }
            if scope.kind != ScopeKind::Local {
                self.barriers.pop();
            }
        }
    }

    /// Declares `name` in the innermost scope, when that is a local one.
    fn declare(&mut self, name: &'n str) {
        let depth = self.stack.len().saturating_sub(1);
        if let Some((scope, names)) = self.stack.last_mut()
            && scope.kind == ScopeKind::Local
        {
            names.push(name);
            self.depths.entry(name).or_default().push(depth);
        }
    }

    /// Whether `name` is bound to a local with no type body between it and here.
    fn binds(&self, name: &str) -> bool {
        let Some(&depth) = self.depths.get(name).and_then(|d| d.last()) else {
            return false;
        };
        self.barriers.last().is_none_or(|&barrier| barrier < depth)
    }
}

/// Outer before inner: by start, then the longer first. One node captured as two kinds of scope
/// (a class body that is also anonymous) is kept once, as its most restrictive kind.
fn sort_scopes(scopes: &mut Vec<Scope>) {
    scopes.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then(b.end.cmp(&a.end))
            .then(rank(b.kind).cmp(&rank(a.kind)))
    });
    scopes.dedup_by_key(|s| (s.start, s.end));
}

const fn rank(kind: ScopeKind) -> u8 {
    match kind {
        ScopeKind::Local => 0,
        ScopeKind::Type => 1,
        ScopeKind::Anonymous => 2,
    }
}

fn pop_ended(stack: &mut Vec<Scope>, at: usize) {
    while stack.last().is_some_and(|s| s.end <= at) {
        stack.pop();
    }
}

/// An enclosing type declaration while the declarations are walked.
struct OpenType<'t> {
    end: usize,
    /// `None` for a local or anonymous one, whose members are skipped too.
    id: Option<DeclId>,
    node: Node<'t>,
    /// How many type parameters it put in the [`TypeScope`].
    pushed: usize,
}

/// The type parameters in scope while the declarations are walked: a stack, each entry's erasure
/// resolved once when it is pushed (its first bound's, followed to the end, as javac erases it;
/// `Object` without a bound or on a cycle), and a map from each name to the stack entries that
/// declare it, innermost last. Erasing a parameter is one lookup, whatever the number of
/// parameters in scope or the length of a bound chain (copying the scope per declaration and
/// following chains per parameter was quadratic on a hostile file). Pinned by `extract::tests`.
#[derive(Default)]
struct TypeScope {
    /// `(name, erasure)`.
    entries: Vec<(String, String)>,
    by_name: std::collections::HashMap<String, Vec<usize>>,
}

impl TypeScope {
    /// The erasure of type variable `name`, if one is in scope.
    fn erasure(&self, name: &str) -> Option<String> {
        let at = *self.by_name.get(name)?.last()?;
        self.entries.get(at).map(|(_, erased)| erased.clone())
    }

    /// Pushes one declaration's type parameters (name and first bound as written) and returns how
    /// many it pushed. `<T, U extends T>` and `<T extends U, U>` both see the whole list: the
    /// bounds are resolved after every name is in, each chain once, iteratively.
    fn push(&mut self, params: &[(String, Option<String>)], budget: &Budget) -> usize {
        let first = self.entries.len();
        let mut bounds: Vec<Option<String>> = Vec::with_capacity(params.len());
        for (name, bound) in params {
            budget.charge(name.len().saturating_add(1));
            let index = self.entries.len();
            self.entries.push((name.clone(), String::new()));
            self.by_name.entry(name.clone()).or_default().push(index);
            bounds.push(bound.as_deref().and_then(|b| {
                budget.charge(b.len());
                parameter_type_base(b).ok().map(|(base, _)| base)
            }));
        }
        // Resolve each new entry by following its chain through the new entries only; an entry
        // already resolved ends a chain, so every entry is visited a bounded number of times.
        let mut done = vec![false; params.len()];
        let mut on_path = vec![false; params.len()];
        for start in 0..params.len() {
            let mut path = Vec::new();
            let mut at = start;
            let erased = loop {
                budget.charge(1);
                if done.get(at).copied().unwrap_or(true) {
                    break self
                        .entries
                        .get(first.saturating_add(at))
                        .map_or_else(|| String::from("Object"), |(_, e)| e.clone());
                }
                if on_path.get(at).copied().unwrap_or(true) {
                    break String::from("Object");
                }
                if let Some(flag) = on_path.get_mut(at) {
                    *flag = true;
                }
                path.push(at);
                let Some(base) = bounds.get(at).cloned().flatten() else {
                    break String::from("Object");
                };
                let Some(&next) = (!base.contains('.'))
                    .then(|| self.by_name.get(&base).and_then(|d| d.last()))
                    .flatten()
                else {
                    break base;
                };
                match next.checked_sub(first) {
                    // A parameter of this same list: follow it.
                    Some(local) => at = local,
                    // An enclosing one, already resolved.
                    None => {
                        break self
                            .entries
                            .get(next)
                            .map_or_else(|| String::from("Object"), |(_, e)| e.clone());
                    }
                }
            };
            for at in path {
                if let Some(entry) = self.entries.get_mut(first.saturating_add(at)) {
                    entry.1.clone_from(&erased);
                }
                if let Some(flag) = done.get_mut(at) {
                    *flag = true;
                }
                if let Some(flag) = on_path.get_mut(at) {
                    *flag = false;
                }
            }
        }
        params.len()
    }

    /// Pops the last `count` entries.
    fn pop(&mut self, count: usize) {
        for _ in 0..count {
            let Some((name, _)) = self.entries.pop() else {
                return;
            };
            if let Some(depths) = self.by_name.get_mut(&name) {
                depths.pop();
                if depths.is_empty() {
                    self.by_name.remove(&name);
                }
            }
        }
    }
}

/// A declaration's parts before the builder interns them.
struct Entry {
    kind: DeclKind,
    modifiers: Modifiers,
    parent: Option<DeclId>,
    name: String,
    name_range: ByteRange,
    range: ByteRange,
    signature: String,
    params: Vec<String>,
    supertypes: Vec<String>,
    permits: Vec<String>,
    annotations: Vec<String>,
    javadoc: Option<ByteRange>,
    type_params: Vec<(String, Option<String>)>,
}

impl Entry {
    fn input(&self) -> OwnedInput<'_> {
        OwnedInput {
            entry: self,
            params: self.params.iter().map(String::as_str).collect(),
            supertypes: self.supertypes.iter().map(String::as_str).collect(),
            permits: self.permits.iter().map(String::as_str).collect(),
            annotations: self.annotations.iter().map(String::as_str).collect(),
        }
    }
}

struct OwnedInput<'e> {
    entry: &'e Entry,
    params: Vec<&'e str>,
    supertypes: Vec<&'e str>,
    permits: Vec<&'e str>,
    annotations: Vec<&'e str>,
}

impl OwnedInput<'_> {
    fn input(&self) -> DeclarationInput<'_> {
        DeclarationInput {
            kind: self.entry.kind,
            modifiers: self.entry.modifiers,
            parent: self.entry.parent,
            name: &self.entry.name,
            name_range: self.entry.name_range,
            range: self.entry.range,
            signature: &self.entry.signature,
            params: &self.params,
            supertypes: &self.supertypes,
            permits: &self.permits,
            annotations: &self.annotations,
            javadoc: self.entry.javadoc,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::GrammarSpec;

    /// Indexes `text` and returns the result and the extraction's steps per input byte.
    fn steps_per_byte(text: &str) -> (Result<Indexed, SyntaxIndexError>, usize) {
        let grammar = Grammar::new(&GrammarSpec::orchard()).unwrap();
        let (result, work) = index_counted(&grammar, &mut Worker::new(), text.as_bytes());
        let steps = work.extraction;
        (result, steps.checked_div(text.len()).unwrap_or(steps))
    }

    /// The most the extraction may spend per byte of a file it indexes: linear, with room.
    const LINEAR: usize = 64;

    #[test]
    fn should_name_the_superclass_when_super_is_called_on_an_annotated_type() {
        // given: `super(…)` in a class whose superclass carries a type-use annotation, and the
        // other places a type's simple name is read from (`new`, `::new`) annotated too
        let text = "class S extends @TA B { S() { super(1); Object o = new @TA B(2); \
                    java.util.function.IntFunction<B> f = @TA B::new; } }";
        // when
        let (result, _) = steps_per_byte(text);
        // then
        let indexed = result.unwrap();
        let summary = &indexed.summary;
        let creations: Vec<(&str, u32, u32)> = summary
            .references()
            .iter()
            .filter(|r| matches!(r.shape, RefShape::New | RefShape::MethodRef))
            .map(|r| (summary.str(r.name), r.range.start().get(), r.arity.0.into()))
            .collect();
        let at = |needle: &str| u32::try_from(text.find(needle).unwrap()).unwrap();
        assert!(
            creations.contains(&("B", at("super(1)"), 1)),
            "{creations:?}"
        );
        assert!(creations.contains(&("B", at("B(2)"), 1)), "{creations:?}");
    }

    #[test]
    fn should_stay_linear_when_a_comment_chain_precedes_a_field_with_many_declarators() {
        // given: the review's first shape, about 1 MB
        let comments = "/**/ ".repeat(100_000);
        let declarators: Vec<String> = (0..50_000).map(|i| format!("a{i}")).collect();
        let text = format!("class A {{ {comments} int {}; }}", declarators.join(", "));
        // when
        let (result, per_byte) = steps_per_byte(&text);
        // then
        let indexed = result.unwrap();
        assert_eq!(indexed.summary.declarations().count(), 50_001);
        assert!(per_byte <= LINEAR, "{per_byte} steps per byte");
    }

    #[test]
    fn should_cut_off_within_the_budget_when_a_long_annotation_heads_many_declarators() {
        // given: the review's second shape; every declarator's signature repeats the head, so
        // the summary itself would be quadratic: the file is refused within its budget
        let annotation = format!("@A({}1)", "1 + ".repeat(125_000));
        let declarators: Vec<String> = (0..50_000).map(|i| format!("a{i}")).collect();
        let text = format!("class A {{ {annotation} int {}; }}", declarators.join(", "));
        // when
        let (result, per_byte) = steps_per_byte(&text);
        // then
        assert_eq!(result.err(), Some(SyntaxIndexError::TooMuchWork));
        assert!(
            per_byte <= UNITS_PER_CALLBACK + 2,
            "{per_byte} steps per byte"
        );
    }

    #[test]
    fn should_stay_linear_when_a_method_has_many_locals_and_uses() {
        // given: the review's third shape, about 1 MB: 50k locals, then a use of each
        let locals: String = (0..50_000).map(|i| format!("int v{i} = 0; ")).collect();
        let uses: Vec<String> = (0..50_000).map(|i| format!("v{i}")).collect();
        let text = format!(
            "class A {{ int v1; void f() {{ {locals} int s = {}; }} }}",
            uses.join(" + ")
        );
        // when
        let (result, per_byte) = steps_per_byte(&text);
        // then
        let indexed = result.unwrap();
        let summary = &indexed.summary;
        assert!(
            summary
                .references()
                .iter()
                .all(|r| !summary.str(r.name).starts_with('v')),
            "a use bound to a local stayed"
        );
        assert!(per_byte <= LINEAR, "{per_byte} steps per byte");
    }

    #[test]
    fn should_stay_linear_when_a_type_has_many_parameters_and_methods() {
        // given: `class A<T0..T19999>` with one method per parameter (the round-2 review's shape)
        let params: Vec<String> = (0..20_000).map(|i| format!("T{i}")).collect();
        let methods: String = (0..20_000)
            .map(|i| format!("void m{i}(T{i} a) {{}} "))
            .collect();
        let text = format!("class A<{}> {{ {methods} }}", params.join(", "));
        // when
        let (result, per_byte) = steps_per_byte(&text);
        // then
        let indexed = result.unwrap();
        assert_eq!(indexed.summary.declarations().count(), 20_001);
        assert!(per_byte <= LINEAR, "{per_byte} steps per byte");
    }

    #[test]
    fn should_stay_linear_when_generic_classes_nest_deeply() {
        // given: 4,000 nested generic classes (the nesting bound), each with a method
        let depth = 4_000;
        let open: String = (0..depth)
            .map(|i| format!("class C{i}<X{i} extends Number> {{ void m(X{i} x) {{}} "))
            .collect();
        let text = format!("{open}{}", "}".repeat(depth));
        // when
        let (result, per_byte) = steps_per_byte(&text);
        // then
        let indexed = result.unwrap();
        let summary = &indexed.summary;
        assert!(
            summary
                .declarations()
                .all(|(_, d)| summary.list(d.params).all(|p| p == "Number"))
        );
        assert!(per_byte <= LINEAR, "{per_byte} steps per byte");
    }

    #[test]
    fn should_stay_linear_when_a_bound_chain_meets_many_parameters() {
        // given: `<T0, T1 extends T0, ..., T1999 extends T1998>` and 8,000 parameters of the last
        let params: Vec<String> = (0_usize..2_000)
            .map(|i| match i.checked_sub(1) {
                Some(p) => format!("T{i} extends T{p}"),
                None => String::from("T0 extends CharSequence"),
            })
            .collect();
        let args: Vec<String> = (0..8_000).map(|i| format!("T1999 a{i}")).collect();
        let text = format!(
            "class A<{}> {{ void m({}) {{}} }}",
            params.join(", "),
            args.join(", ")
        );
        // when
        let (result, per_byte) = steps_per_byte(&text);
        // then
        let indexed = result.unwrap();
        let summary = &indexed.summary;
        let (_, method) = summary.declarations().nth(1).unwrap();
        assert!(summary.list(method.params).all(|p| p == "CharSequence"));
        assert!(per_byte <= LINEAR, "{per_byte} steps per byte");
    }

    /// One piece of a generated file.
    #[derive(Debug, Clone)]
    enum Piece {
        Open {
            params: usize,
            chained: bool,
        },
        Close,
        Field {
            annotations: usize,
            declarators: usize,
            comments: usize,
        },
        Method {
            params: usize,
            args: usize,
            locals: usize,
            uses: usize,
        },
    }

    fn piece() -> impl proptest::strategy::Strategy<Value = Piece> {
        use proptest::prelude::*;
        prop_oneof![
            (0usize..12, any::<bool>())
                .prop_map(|(params, chained)| Piece::Open { params, chained }),
            Just(Piece::Close),
            (0usize..4, 1usize..8, 0usize..6).prop_map(|(annotations, declarators, comments)| {
                Piece::Field {
                    annotations,
                    declarators,
                    comments,
                }
            }),
            (0usize..6, 0usize..12, 0usize..12, 0usize..24).prop_map(
                |(params, args, locals, uses)| Piece::Method {
                    params,
                    args,
                    locals,
                    uses
                }
            ),
        ]
    }

    /// A Java file of `pieces`: generic classes, bounds chained or not, nesting, annotated fields
    /// with several declarators and comments, generic methods with locals and their uses.
    fn render(pieces: &[Piece]) -> String {
        let mut out = String::from("package p;\nclass Root<R extends Comparable<R>> {\n");
        let mut open = 1_usize;
        let mut n = 0_usize;
        for piece in pieces {
            n = n.saturating_add(1);
            match *piece {
                Piece::Open { params, chained } => {
                    let names: Vec<String> = (0..params)
                        .map(|i| match (chained, i.checked_sub(1)) {
                            (true, Some(p)) => format!("T{n}_{i} extends T{n}_{p}"),
                            _ => format!("T{n}_{i} extends Number"),
                        })
                        .collect();
                    let generic = if names.is_empty() {
                        String::new()
                    } else {
                        format!("<{}>", names.join(", "))
                    };
                    out.push_str(&format!(
                        "/** C{n} */ @Deprecated static class C{n}{generic} {{\n"
                    ));
                    open = open.saturating_add(1);
                }
                Piece::Close if open > 1 => {
                    out.push_str("}\n");
                    open = open.saturating_sub(1);
                }
                Piece::Close => {}
                Piece::Field {
                    annotations,
                    declarators,
                    comments,
                } => {
                    out.push_str(&"/* c */ ".repeat(comments));
                    out.push_str(&"@SuppressWarnings(\"x\") ".repeat(annotations));
                    let names: Vec<String> =
                        (0..declarators).map(|i| format!("f{n}_{i}")).collect();
                    out.push_str(&format!("int {};\n", names.join(", ")));
                }
                Piece::Method {
                    params,
                    args,
                    locals,
                    uses,
                } => {
                    let generic: Vec<String> =
                        (0..params).map(|i| format!("M{i} extends R")).collect();
                    let generic = if generic.is_empty() {
                        String::new()
                    } else {
                        format!("<{}> ", generic.join(", "))
                    };
                    let args: Vec<String> = (0..args).map(|i| format!("R a{i}")).collect();
                    out.push_str(&format!("{generic}void m{n}({}) {{ ", args.join(", ")));
                    for i in 0..locals {
                        out.push_str(&format!("int v{i} = {i}; "));
                    }
                    for i in 0..uses {
                        out.push_str(&format!(
                            "g(v{}, a{}); ",
                            i.checked_rem(locals).unwrap_or(0),
                            i.checked_rem(3).unwrap_or(0)
                        ));
                    }
                    out.push_str("}\n");
                }
            }
        }
        out.push_str(&"}\n".repeat(open));
        out
    }

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config {
            rng_seed: proptest::test_runner::RngSeed::Fixed(0x1DC0_0703),
            failure_persistence: None,
            ..proptest::test_runner::Config::default()
        })]

        #[test]
        fn should_spend_linear_steps_when_given_any_mix_of_declarations(
            pieces in proptest::collection::vec(piece(), 0..200),
        ) {
            // given
            let text = render(&pieces);
            // when
            let (result, per_byte) = steps_per_byte(&text);
            // then
            proptest::prop_assert!(result.is_ok(), "{:?}", result.err());
            proptest::prop_assert!(per_byte <= LINEAR, "{} steps per byte", per_byte);
        }
    }
}
