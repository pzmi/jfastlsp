//! The syntax tree around one reference site (IDX-10's syntax-tree resolver): what
//! `SyntaxIndexer::site_syntax` gives the queries, read from tree-sitter-java-orchard's parse of
//! the request's file at query time.
//!
//! **Every node kind of the grammar is classified once** ([`KINDS`]): a node between the site and
//! the compilation unit is plain code the lookup passes through, a callable, a field, a type
//! declaration, a part of a type's body, the unit, or denied. A kind the table does not list, a
//! listed kind in a position the walk does not know (a class body under `new` or an enum
//! constant, a type declared in a block, a member's initializer, a block directly in a class
//! body), or a syntax error anywhere in the file, makes the site [`Around::Denied`] or gives no
//! facts at all. `scopes::tests` fails when the grammar's named kinds and the table differ, so a
//! grammar update cannot add a scope the resolver silently walks through.
//!
//! **What the site is** ([`SiteForm`]) comes from its parent and the field it fills there, from
//! two explicit lists ([`NAME_PLACES`], [`TYPE_PLACES`]) and the call, `this.` and annotation
//! shapes; anything else is [`SiteForm::Other`].
//!
//! **Shadowing** ([`SiteSyntax::shadowed`]): whether the site's name is declared as a variable,
//! parameter, pattern, lambda parameter, local type or type parameter anywhere in a callable
//! around it, or as a type parameter of a type around it. Anywhere, not only before the site: a
//! pattern variable's flow scope is not followed, so any binder of the name counts. Names are
//! compared as Java names (identifier-ignorable characters dropped).
//!
//! A file with Unicode escapes gives no facts (the offsets of the tree and of the file would
//! differ); nor does one whose parse spends the indexer's work budget.
//!
//! Pinned by: `scopes::tests` (the table against the grammar, each form and frame) and
//! `tests/definitions/main.rs` (every site of the fixture projects against javac's bindings).

use jfl_application::syntax_indexer::{Around, SiteForm, SiteSyntax};
use jfl_domain::{ByteOffset, ByteRange};
use tree_sitter::Node;

use crate::extract::{Budget, Worker, parse};
use crate::grammar::Grammar;
use crate::source::{Translated, java_name};

/// What a node kind is to the walk from a site to the compilation unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    /// An expression, statement, block or piece of type syntax: it declares no member and opens
    /// no class body, so a lookup passes through it unchanged (its binders count as shadowing).
    Code,
    /// A method or constructor: its `body` field is the callable's body, the rest its header.
    Callable,
    /// A field or interface constant: its type and modifiers are the header; an initializer is
    /// denied (not handled yet).
    Field,
    /// A class, interface, enum or record declaration: its `body` field is the type's body, the
    /// rest its header (a record's components are denied).
    Type,
    /// A type body's own node (`class_body`, `interface_body`, `enum_body`,
    /// `enum_body_declarations`), checked against its parent and its child.
    Body,
    /// The compilation unit.
    Unit,
    /// Not handled: the site is never proved.
    Deny,
}

/// Every named, visible node kind of tree-sitter-java-orchard 0.5.25 and its [`Role`], sorted by
/// name. Leaves (identifiers, literals, keywords) are never between a site and the unit; they are
/// denied so that the table is total.
pub(crate) const KINDS: [(&str, Role); 146] = [
    ("annotated_type", Role::Deny),
    ("annotation", Role::Deny),
    ("annotation_argument_list", Role::Deny),
    ("annotation_type_body", Role::Deny),
    ("annotation_type_declaration", Role::Deny),
    ("annotation_type_element_declaration", Role::Deny),
    ("argument_list", Role::Code),
    ("array_access", Role::Code),
    ("array_creation_expression", Role::Code),
    ("array_initializer", Role::Code),
    ("array_type", Role::Code),
    ("assert_statement", Role::Code),
    ("assignment_expression", Role::Code),
    ("asterisk", Role::Deny),
    ("binary_expression", Role::Code),
    ("binary_integer_literal", Role::Deny),
    ("block", Role::Code),
    ("block_comment", Role::Deny),
    ("boolean_type", Role::Deny),
    ("break_statement", Role::Code),
    ("cast_expression", Role::Code),
    ("catch_clause", Role::Code),
    ("catch_formal_parameter", Role::Code),
    ("catch_type", Role::Code),
    ("character_literal", Role::Deny),
    ("class_body", Role::Body),
    ("class_declaration", Role::Type),
    ("class_literal", Role::Code),
    ("compact_constructor_declaration", Role::Deny),
    ("constant_declaration", Role::Field),
    ("constructor_body", Role::Code),
    ("constructor_declaration", Role::Callable),
    ("continue_statement", Role::Code),
    ("decimal_floating_point_literal", Role::Deny),
    ("decimal_integer_literal", Role::Deny),
    ("dimensions", Role::Code),
    ("dimensions_expr", Role::Code),
    ("do_statement", Role::Code),
    ("element_value_array_initializer", Role::Deny),
    ("element_value_pair", Role::Deny),
    ("enhanced_for_statement", Role::Code),
    ("enum_body", Role::Body),
    ("enum_body_declarations", Role::Body),
    ("enum_constant", Role::Deny),
    ("enum_declaration", Role::Type),
    ("explicit_constructor_invocation", Role::Deny),
    ("exports_module_directive", Role::Deny),
    ("expression_statement", Role::Code),
    ("extends_interfaces", Role::Code),
    ("false", Role::Deny),
    ("field_access", Role::Code),
    ("field_declaration", Role::Field),
    ("finally_clause", Role::Code),
    ("floating_point_type", Role::Deny),
    ("for_statement", Role::Code),
    ("formal_parameter", Role::Code),
    ("formal_parameters", Role::Code),
    ("generic_type", Role::Code),
    ("guard", Role::Deny),
    ("hex_floating_point_literal", Role::Deny),
    ("hex_integer_literal", Role::Deny),
    ("identifier", Role::Deny),
    ("if_statement", Role::Code),
    ("import_declaration", Role::Deny),
    ("inferred_parameters", Role::Deny),
    ("instanceof_expression", Role::Code),
    ("integral_type", Role::Deny),
    ("interface_body", Role::Body),
    ("interface_declaration", Role::Type),
    ("labeled_statement", Role::Code),
    ("lambda_expression", Role::Deny),
    ("line_comment", Role::Deny),
    ("local_variable_declaration", Role::Code),
    ("marker_annotation", Role::Deny),
    ("method_declaration", Role::Callable),
    ("method_invocation", Role::Code),
    ("method_reference", Role::Deny),
    ("modifier", Role::Deny),
    ("modifiers", Role::Code),
    ("module_body", Role::Deny),
    ("module_declaration", Role::Deny),
    ("null_literal", Role::Deny),
    ("object_creation_expression", Role::Code),
    ("octal_integer_literal", Role::Deny),
    ("opens_module_directive", Role::Deny),
    ("package_declaration", Role::Deny),
    ("parenthesized_expression", Role::Code),
    ("pattern", Role::Code),
    ("permits", Role::Code),
    ("program", Role::Unit),
    ("provides_module_directive", Role::Deny),
    ("receiver_parameter", Role::Deny),
    ("record_declaration", Role::Type),
    ("record_pattern", Role::Code),
    ("record_pattern_body", Role::Code),
    ("record_pattern_component", Role::Code),
    ("requires_modifier", Role::Deny),
    ("requires_module_directive", Role::Deny),
    ("resource", Role::Code),
    ("resource_specification", Role::Code),
    ("return_statement", Role::Code),
    ("scoped_identifier", Role::Deny),
    ("scoped_type_identifier", Role::Code),
    ("spread_parameter", Role::Code),
    ("static_initializer", Role::Deny),
    ("string_literal", Role::Deny),
    ("string_template_begin", Role::Deny),
    ("string_template_end", Role::Deny),
    ("string_template_mid", Role::Deny),
    ("super", Role::Deny),
    ("super_interfaces", Role::Code),
    ("superclass", Role::Code),
    ("switch_block", Role::Deny),
    ("switch_block_statement_group", Role::Deny),
    ("switch_expression", Role::Deny),
    ("switch_label", Role::Deny),
    ("switch_rule", Role::Deny),
    ("synchronized_statement", Role::Code),
    ("template_expression", Role::Deny),
    ("ternary_expression", Role::Code),
    ("text_block_template_begin", Role::Deny),
    ("text_block_template_end", Role::Deny),
    ("text_block_template_mid", Role::Deny),
    ("this", Role::Deny),
    ("throw_statement", Role::Code),
    ("throws", Role::Code),
    ("true", Role::Deny),
    ("try_statement", Role::Code),
    ("try_with_resources_statement", Role::Code),
    ("type_arguments", Role::Code),
    ("type_bound", Role::Code),
    ("type_identifier", Role::Deny),
    ("type_list", Role::Code),
    ("type_parameter", Role::Code),
    ("type_parameters", Role::Code),
    ("type_pattern", Role::Code),
    ("unary_expression", Role::Code),
    ("underscore_pattern", Role::Deny),
    ("update_expression", Role::Code),
    ("uses_module_directive", Role::Deny),
    ("variable_declarator", Role::Code),
    ("visibility", Role::Deny),
    ("void_type", Role::Deny),
    ("while_statement", Role::Code),
    ("wildcard", Role::Code),
    ("yield_statement", Role::Deny),
];

/// The role of `kind` and the table's spelling of it; a kind the table does not list is denied.
pub(crate) fn role(kind: &str) -> (Role, &'static str) {
    KINDS
        .binary_search_by(|(k, _)| (*k).cmp(kind))
        .ok()
        .and_then(|at| KINDS.get(at))
        .map_or(
            (Role::Deny, "a kind the table does not list"),
            |&(k, role)| (role, k),
        )
}

/// Where a bare identifier is a name in an expression ([`SiteForm::Name`]): its parent's kind and
/// the field it fills there (`""`: no field).
pub(crate) const NAME_PLACES: [(&str, &str); 26] = [
    ("argument_list", ""),
    ("array_access", "array"),
    ("array_access", "index"),
    ("array_initializer", ""),
    ("assert_statement", ""),
    ("assignment_expression", "left"),
    ("assignment_expression", "right"),
    ("binary_expression", "left"),
    ("binary_expression", "right"),
    ("cast_expression", "value"),
    ("dimensions_expr", ""),
    ("enhanced_for_statement", "value"),
    ("field_access", "object"),
    ("for_statement", "condition"),
    ("for_statement", "update"),
    ("instanceof_expression", "left"),
    ("method_invocation", "object"),
    ("parenthesized_expression", ""),
    ("return_statement", ""),
    ("ternary_expression", "alternative"),
    ("ternary_expression", "condition"),
    ("ternary_expression", "consequence"),
    ("throw_statement", ""),
    ("unary_expression", "operand"),
    ("update_expression", ""),
    ("variable_declarator", "value"),
];

/// Where a simple type name is a type ([`SiteForm::Type`]): its parent's kind and the field it
/// fills there (`""`: no field). A `generic_type` stands for its base name.
pub(crate) const TYPE_PLACES: [(&str, &str); 21] = [
    ("array_creation_expression", "type"),
    ("array_type", "element"),
    ("cast_expression", "type"),
    ("catch_type", ""),
    ("class_literal", ""),
    ("constant_declaration", "type"),
    ("enhanced_for_statement", "type"),
    ("field_declaration", "type"),
    ("formal_parameter", "type"),
    ("instanceof_expression", "right"),
    ("local_variable_declaration", "type"),
    ("method_declaration", "type"),
    ("resource", "type"),
    ("spread_parameter", "type"),
    ("superclass", ""),
    ("throws", ""),
    ("type_arguments", ""),
    ("type_bound", ""),
    ("type_list", ""),
    ("type_pattern", ""),
    ("wildcard", ""),
];

/// The member declarations a type body may hold between it and a site.
const MEMBERS: [&str; 9] = [
    "class_declaration",
    "constant_declaration",
    "constructor_declaration",
    "enum_body_declarations",
    "enum_declaration",
    "field_declaration",
    "interface_declaration",
    "method_declaration",
    "record_declaration",
];

/// The type bodies a member may be declared in.
const MEMBER_HOLDERS: [&str; 3] = ["class_body", "enum_body_declarations", "interface_body"];

/// The deepest site the walk reads: a site further from the compilation unit gives no facts.
/// Real code stays far below it; generated left-deep concatenations go past it (the slice-2
/// review's H3), and so are left unproved rather than walked.
pub(crate) const MAX_SITE_DEPTH: usize = 512;

/// One node on the path from the compilation unit to a site, and the field it fills in the
/// node before it (`""`: none).
#[derive(Debug, Clone, Copy)]
struct Step<'t> {
    node: Node<'t>,
    field: &'t str,
}

/// The byte range of `node`'s `name` field.
fn name_range(node: Node<'_>) -> Option<ByteRange> {
    let name = node.child_by_field_name("name")?;
    let start = u32::try_from(name.start_byte()).ok()?;
    let end = u32::try_from(name.end_byte()).ok()?;
    ByteRange::new(ByteOffset::new(start), ByteOffset::new(end)).ok()
}

/// The largest file whose facts a query reads: its parse must fit in definition's 300 ms
/// deadline (plan §2.7), which no deadline can interrupt. The largest medium file with sites
/// (316 KB) parses in 50 ms (`just bench-queries`); a larger file proves nothing.
pub(crate) const MAX_FACTS_BYTES: usize = 1024 * 1024;

/// The facts about the site whose name is at `name` of `text`, parsed with `grammar` on
/// `worker`; `None` for a file the walk cannot read faithfully (Unicode escapes, a syntax error,
/// a spent work budget, another grammar), a site deeper than [`MAX_SITE_DEPTH`], or a range that
/// is not one identifier.
pub(crate) fn site_syntax(
    grammar: &Grammar,
    worker: &mut Worker,
    text: &str,
    name: ByteRange,
) -> Option<SiteSyntax> {
    if text.len() > MAX_FACTS_BYTES {
        return None;
    }
    sites_syntax(grammar, worker, text, &[name]).pop().flatten()
}

/// [`site_syntax`] for each of `names`, over one parse: what facts kept in the summary at index
/// time would cost (the benchmark's measure of that design).
pub(crate) fn sites_syntax(
    grammar: &Grammar,
    worker: &mut Worker,
    text: &str,
    names: &[ByteRange],
) -> Vec<Option<SiteSyntax>> {
    facts(grammar, worker, text, names).0
}

/// [`sites_syntax`], and the units of work the walks spent after the parse: every step down the
/// path, out along it, and through a binder scan costs one, charged to the parse's budget.
pub(crate) fn facts(
    grammar: &Grammar,
    worker: &mut Worker,
    text: &str,
    names: &[ByteRange],
) -> (Vec<Option<SiteSyntax>>, usize) {
    let none = || (vec![None; names.len()], 0);
    if grammar.id.name != "tree-sitter-java-orchard" {
        return none();
    }
    let translated = Translated::new(text);
    if !translated.map.is_identity() {
        return none();
    }
    let src = translated.text.as_bytes();
    let budget = Budget::new(text.len());
    let Ok(tree) = parse(grammar, worker, src, &budget) else {
        return none();
    };
    let root = tree.root_node();
    if root.has_error() {
        return none();
    }
    let parsed = budget.used();
    let found = names
        .iter()
        .map(|&name| {
            let facts = at(root, src, name, &budget);
            // A walk that spent the file's budget proves nothing.
            facts.filter(|_| budget.check().is_ok())
        })
        .collect();
    (found, budget.used().saturating_sub(parsed))
}

/// The path from `root` to the identifier at `start..end`, descending once (each level a
/// binary search among the children, O(depth) in all); `None` past [`MAX_SITE_DEPTH`] or when no
/// identifier has exactly that range.
fn path<'t>(root: Node<'t>, start: usize, end: usize, budget: &Budget) -> Option<Vec<Step<'t>>> {
    let mut cursor = root.walk();
    let mut out = vec![Step {
        node: root,
        field: "",
    }];
    loop {
        let node = cursor.node();
        if node.start_byte() == start
            && node.end_byte() == end
            && matches!(node.kind(), "identifier" | "type_identifier")
        {
            return Some(out);
        }
        if out.len() > MAX_SITE_DEPTH {
            return None;
        }
        budget.charge(1);
        cursor.goto_first_child_for_byte(start)?;
        let child = cursor.node();
        if child.start_byte() > start || child.end_byte() < end {
            return None;
        }
        out.push(Step {
            node: child,
            field: cursor.field_name().unwrap_or(""),
        });
    }
}

/// The facts about the identifier at `name` under `root`.
fn at<'t>(root: Node<'t>, src: &[u8], name: ByteRange, budget: &Budget) -> Option<SiteSyntax> {
    let start = usize::try_from(name.start().get()).ok()?;
    let end = usize::try_from(name.end().get()).ok()?;
    let path = path(root, start, end, budget)?;
    let site = path.len().checked_sub(1)?;
    let spelled = java_name(core::str::from_utf8(src.get(start..end)?).ok()?).into_owned();
    let (form, from) = form(&path, site);
    let around = around(&path, from, budget);
    // Only a walk that reached the unit can be proved; the binder scan is spent only then, and
    // such a walk passes at most one callable.
    let shadowed = around.last() == Some(&Around::Unit) && shadowed(&path, &spelled, src, budget);
    Some(SiteSyntax {
        form,
        around,
        shadowed,
    })
}

/// Whether `node` has a child of one of `kinds` (one pass over its children).
fn has_child(node: Node<'_>, kinds: &[&str]) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|c| kinds.contains(&c.kind()))
}

/// What the identifier at `path[site]` is, and the index of the node the walk outward starts
/// from (the site's own place: the identifier, its `generic_type`, or its annotation).
fn form(path: &[Step<'_>], site: usize) -> (SiteForm, usize) {
    let (Some(at), Some(up)) = (
        path.get(site),
        site.checked_sub(1).and_then(|i| path.get(i)),
    ) else {
        return (SiteForm::Other, site);
    };
    let (node, parent, field) = (at.node, up.node, at.field);
    if node.kind() == "identifier" {
        let form = match (parent.kind(), field) {
            ("marker_annotation" | "annotation", "name") => {
                return (SiteForm::Annotation, site.saturating_sub(1));
            }
            ("method_invocation", "name") => match parent.child_by_field_name("object") {
                None if !has_child(parent, &["super"]) => SiteForm::Call,
                Some(object) if object.kind() == "this" => SiteForm::ThisCall,
                _ => SiteForm::Other,
            },
            ("field_access", "field") => match parent.child_by_field_name("object") {
                Some(object) if object.kind() == "this" => SiteForm::ThisName,
                _ => SiteForm::Other,
            },
            // `X.this` and `X.super`: the qualifier is a type name, never an ambiguous name
            // (JLS 15.8.4, 15.11.2, 15.12.1; the slice-2 review's H2).
            ("field_access" | "method_invocation", "object")
                if has_child(parent, &["this", "super"]) =>
            {
                SiteForm::Other
            }
            (kind, field) if NAME_PLACES.contains(&(kind, field)) => SiteForm::Name,
            _ => SiteForm::Other,
        };
        return (form, site);
    }
    // A type identifier: a generic type stands for its base name.
    let generic = parent.kind() == "generic_type" && parent.named_child(0) == Some(node);
    let used = if generic {
        site.saturating_sub(1)
    } else {
        site
    };
    let (Some(place), Some(holder)) = (
        path.get(used),
        used.checked_sub(1).and_then(|i| path.get(i)),
    ) else {
        return (SiteForm::Other, used);
    };
    let form = match (holder.node.kind(), place.field) {
        ("object_creation_expression", "type") => {
            if has_child(holder.node, &["class_body"]) {
                SiteForm::Other
            } else {
                SiteForm::New
            }
        }
        (kind, field) if TYPE_PLACES.contains(&(kind, field)) => SiteForm::Type,
        _ => SiteForm::Other,
    };
    (form, used)
}

/// The frames out from `path[from]` (exclusive) to the compilation unit, innermost first,
/// consecutive [`Around::Code`] as one, ending at the unit or the first [`Around::Denied`].
fn around(path: &[Step<'_>], from: usize, budget: &Budget) -> Vec<Around> {
    let mut out: Vec<Around> = Vec::new();
    let push = |out: &mut Vec<Around>, frame: Around| {
        if !(frame == Around::Code && out.last() == Some(&Around::Code)) {
            out.push(frame);
        }
    };
    let kind_at = |i: Option<usize>| i.and_then(|i| path.get(i)).map_or("", |s| s.node.kind());
    let mut at = from;
    while let Some(i) = at.checked_sub(1) {
        budget.charge(1);
        let (Some(step), Some(child)) = (path.get(i), path.get(at)) else {
            break;
        };
        let node = step.node;
        let field = child.field;
        let above = kind_at(i.checked_sub(1));
        let (role, kind) = role(node.kind());
        let frame = match role {
            Role::Code => Around::Code,
            Role::Callable => {
                let Some(name) = name_range(node) else {
                    return deny(out, kind);
                };
                if !MEMBER_HOLDERS.contains(&above) {
                    return deny(out, "callable outside a type body");
                }
                if field == "body" {
                    Around::CallableBody(name)
                } else {
                    Around::CallableHeader(name)
                }
            }
            Role::Field => {
                if field == "declarator" {
                    return deny(out, "field initializer");
                }
                Around::FieldHeader
            }
            Role::Type => {
                let Some(name) = name_range(node) else {
                    return deny(out, kind);
                };
                if above != "program" && !MEMBER_HOLDERS.contains(&above) {
                    return deny(out, "local type");
                }
                match field {
                    "body" => Around::TypeBody(name),
                    "parameters" => return deny(out, "record header"),
                    _ => Around::TypeHeader(name),
                }
            }
            Role::Body => {
                if !MEMBERS.contains(&child.node.kind()) {
                    return deny(out, "initializer or enum constant");
                }
                let placed = match kind {
                    "class_body" => matches!(above, "class_declaration" | "record_declaration"),
                    "interface_body" => above == "interface_declaration",
                    "enum_body" => above == "enum_declaration",
                    "enum_body_declarations" => above == "enum_body",
                    _ => false,
                };
                if !placed {
                    return deny(out, "anonymous or enum constant body");
                }
                at = i;
                continue;
            }
            Role::Unit => {
                push(&mut out, Around::Unit);
                return out;
            }
            Role::Deny => return deny(out, kind),
        };
        push(&mut out, frame);
        at = i;
    }
    // A tree's root is always the unit; a walk that ends elsewhere is not one the table knows.
    deny(out, "detached")
}

fn deny(mut out: Vec<Around>, kind: &'static str) -> Vec<Around> {
    out.push(Around::Denied(kind));
    out
}

/// Whether an identifier (or a type identifier, `type_identifier`) that fills `field` in a node
/// of kind `parent` declares a name there: a variable, parameter, pattern or lambda parameter, a
/// type, or a type parameter.
fn declares(parent: &str, field: &str, type_identifier: bool) -> bool {
    match parent {
        "variable_declarator"
        | "formal_parameter"
        | "catch_formal_parameter"
        | "enhanced_for_statement"
        | "resource"
        | "instanceof_expression"
        | "class_declaration"
        | "interface_declaration"
        | "enum_declaration"
        | "record_declaration"
        | "annotation_type_declaration" => field == "name",
        "lambda_expression" => field == "parameters",
        "inferred_parameters" | "type_pattern" | "record_pattern_component" => !type_identifier,
        "type_parameter" => type_identifier,
        _ => false,
    }
}

/// Whether `name` is declared by a binder in a callable on `path` (anywhere in it) or as a type
/// parameter of a type on it.
fn shadowed(path: &[Step<'_>], name: &str, src: &[u8], budget: &Budget) -> bool {
    let named = |node: Node<'_>| {
        src.get(node.byte_range())
            .and_then(|b| core::str::from_utf8(b).ok())
            .is_some_and(|text| java_name(text) == name)
    };
    path.iter().any(|step| match role(step.node.kind()).0 {
        Role::Callable => binds_within(step.node, &named, budget),
        Role::Type => step
            .node
            .child_by_field_name("type_parameters")
            .is_some_and(|params| binds_within(params, &named, budget)),
        _ => false,
    })
}

/// Whether a binder spelled as `named` accepts lies anywhere under `root`: one pre-order pass,
/// each node charged to `budget`, its parent's kind kept on a stack (no walk back up).
fn binds_within(root: Node<'_>, named: &dyn Fn(Node<'_>) -> bool, budget: &Budget) -> bool {
    let mut walk = root.walk();
    let mut kinds: Vec<&str> = Vec::new();
    'visit: loop {
        let node = walk.node();
        budget.charge(1);
        if budget.check().is_err() {
            // Spent: the caller's check refuses the facts.
            return true;
        }
        let kind = node.kind();
        let identifier = kind == "identifier";
        if (identifier || kind == "type_identifier")
            && declares(
                kinds.last().copied().unwrap_or(""),
                walk.field_name().unwrap_or(""),
                !identifier,
            )
            && named(node)
        {
            return true;
        }
        if walk.goto_first_child() {
            kinds.push(kind);
            continue;
        }
        loop {
            if walk.goto_next_sibling() {
                continue 'visit;
            }
            if !walk.goto_parent() {
                return false;
            }
            kinds.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::GrammarSpec;

    /// Every named, visible kind of the orchard grammar, sorted.
    fn grammar_kinds() -> Vec<String> {
        let language: tree_sitter::Language = tree_sitter_java_orchard::LANGUAGE.into();
        let mut kinds: Vec<String> = (0..language.node_kind_count())
            .filter_map(|id| u16::try_from(id).ok())
            .filter(|&id| language.node_kind_is_named(id) && language.node_kind_is_visible(id))
            .filter_map(|id| language.node_kind_for_id(id).map(str::to_owned))
            .collect();
        kinds.sort_unstable();
        kinds.dedup();
        kinds
    }

    #[test]
    fn should_classify_every_kind_once_when_the_grammar_is_read() {
        // given
        let grammar = grammar_kinds();
        // when
        let table: Vec<&str> = KINDS.iter().map(|(k, _)| *k).collect();
        // then: sorted, no duplicate, exactly the grammar's named kinds
        let mut sorted = table.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            table, sorted,
            "KINDS must be sorted and hold each kind once"
        );
        let missing: Vec<_> = grammar
            .iter()
            .filter(|k| !table.contains(&k.as_str()))
            .collect();
        let extra: Vec<_> = table
            .iter()
            .filter(|k| !grammar.iter().any(|g| g == *k))
            .collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "a grammar kind is neither allowed nor denied: missing {missing:?}, not in the grammar {extra:?}"
        );
    }

    #[test]
    fn should_name_only_grammar_kinds_when_places_are_listed() {
        // given
        let grammar = grammar_kinds();
        // when
        let places: Vec<&str> = NAME_PLACES
            .iter()
            .chain(TYPE_PLACES.iter())
            .map(|(k, _)| *k)
            .chain(MEMBERS)
            .chain(MEMBER_HOLDERS)
            .collect();
        // then
        let unknown: Vec<_> = places
            .iter()
            .filter(|k| !grammar.iter().any(|g| g == *k))
            .collect();
        assert!(unknown.is_empty(), "not kinds of the grammar: {unknown:?}");
    }

    /// The facts at the `nth` occurrence of `needle` in `text`.
    fn facts(text: &str, needle: &str, nth: usize) -> Option<SiteSyntax> {
        let grammar = Grammar::new(&GrammarSpec::orchard()).unwrap();
        let start = text.match_indices(needle).nth(nth).unwrap().0;
        let end = start.saturating_add(needle.len());
        let range = ByteRange::new(
            ByteOffset::new(u32::try_from(start).unwrap()),
            ByteOffset::new(u32::try_from(end).unwrap()),
        )
        .unwrap();
        site_syntax(&grammar, &mut Worker::new(), text, range)
    }

    fn range_of(text: &str, needle: &str, nth: usize) -> ByteRange {
        let start = text.match_indices(needle).nth(nth).unwrap().0;
        ByteRange::new(
            ByteOffset::new(u32::try_from(start).unwrap()),
            ByteOffset::new(u32::try_from(start.saturating_add(needle.len())).unwrap()),
        )
        .unwrap()
    }

    #[test]
    fn should_place_a_bare_name_in_a_method_body_when_its_class_is_top_level() {
        // given
        let text = "class A { int count; class B { int f() { return count + 1; } } }";
        // when
        let found = facts(text, "count", 1).unwrap();
        // then
        assert_eq!(found.form, SiteForm::Name);
        assert_eq!(
            found.around,
            vec![
                Around::Code,
                Around::CallableBody(range_of(text, "f", 0)),
                Around::TypeBody(range_of(text, "B", 0)),
                Around::TypeBody(range_of(text, "A", 0)),
                Around::Unit,
            ]
        );
        assert!(!found.shadowed);
    }

    #[test]
    fn should_tell_each_form_when_names_stand_in_their_places() {
        // given
        let text = "import a.Tag; class A extends Base { @Tag Money mon; void f(Money x) { \
                    this.mon = x; this.gee(); gee(); new Money(); new Money() {}; super.gee(); \
                    mon.hop(); java.util.List<Money> l; } }";
        // when
        let forms: Vec<SiteForm> = [
            ("Base", 0),
            ("Tag", 1),
            ("Money", 0),
            ("Money", 1),
            ("mon", 1),
            ("gee", 0),
            ("gee", 1),
            ("Money", 2),
            ("Money", 3),
            ("gee", 2),
            ("mon", 2),
            ("hop", 0),
            ("util", 0),
            ("Money", 4),
        ]
        .iter()
        .map(|&(needle, nth)| facts(text, needle, nth).map_or(SiteForm::Other, |f| f.form))
        .collect();
        // then
        assert_eq!(
            forms,
            [
                SiteForm::Type,
                SiteForm::Annotation,
                SiteForm::Type,
                SiteForm::Type,
                SiteForm::ThisName,
                SiteForm::ThisCall,
                SiteForm::Call,
                SiteForm::New,
                SiteForm::Other,
                SiteForm::Other,
                SiteForm::Name,
                SiteForm::Other,
                SiteForm::Other,
                SiteForm::Type,
            ]
        );
    }

    #[test]
    fn should_deny_the_site_when_a_construct_around_it_is_not_handled() {
        // given: an anonymous class, a lambda, a local class, an enum constant body, a switch,
        // an annotation's argument, a field initializer, an instance initializer, a static one
        let text = "class A { int x; enum E { K { int g() { return x; } } } \
                    @T(v = x) int y = x; { int z = x; } static { int w = x; } \
                    void f() { new Object() { int g() { return x; } }; \
                    Runnable r = () -> { int q = x; }; class L { int g() { return x; } } \
                    switch (x) { case 1 -> x++; } } }";
        // when
        let denied: Vec<Option<&'static str>> = (1..=9)
            .map(|nth| {
                facts(text, "x", nth).and_then(|f| match f.around.last() {
                    Some(Around::Denied(kind)) => Some(*kind),
                    _ => None,
                })
            })
            .collect();
        // then
        assert_eq!(
            denied,
            [
                Some("anonymous or enum constant body"),
                Some("element_value_pair"),
                Some("field initializer"),
                Some("initializer or enum constant"),
                Some("static_initializer"),
                Some("anonymous or enum constant body"),
                Some("lambda_expression"),
                Some("local type"),
                Some("switch_expression"),
            ]
        );
    }

    #[test]
    fn should_shadow_the_name_when_a_callable_or_type_declares_it_anywhere() {
        // given
        let text = "class A<Tee> { int alpha, beta, gamma, eps; Tee t; \
                    void f(int alpha) { beta++; alpha++; } void g() { gamma++; } \
                    void h() { eps++; } void i() { int eps = 0; } }";
        let pattern =
            "class A { int delta; void g() { delta++; if (o instanceof Integer delta) {} } }";
        // when
        let shadowed: Vec<bool> = [
            ("alpha", 2),
            ("beta", 1),
            ("gamma", 1),
            ("Tee", 1),
            ("eps", 1),
        ]
        .iter()
        .map(|&(needle, nth)| facts(text, needle, nth).unwrap().shadowed)
        .collect();
        let by_pattern = facts(pattern, "delta", 1).unwrap().shadowed;
        // then: `alpha` is a parameter, `Tee` the class's type parameter, `delta` a pattern
        // variable; `eps` is declared only in another method
        assert_eq!(shadowed, [true, false, false, true, false]);
        assert!(by_pattern);
    }

    #[test]
    fn should_read_a_type_qualifier_when_this_or_super_follows_it() {
        // given: a field named like the class (the slice-2 review's H2)
        let text = "class Qual { int Qual; int count; void m() {} class In extends Qual { \
                    int f() { return Qual.this.count; } void g() { Qual.this.m(); \
                    Qual.super.m(); } int h() { return Qual.count; } } }";
        // when
        let forms: Vec<SiteForm> = [("Qual", 3), ("Qual", 4), ("Qual", 5), ("Qual", 6)]
            .iter()
            .map(|&(needle, nth)| facts(text, needle, nth).map_or(SiteForm::Other, |f| f.form))
            .collect();
        // then: a type name before `.this` and `.super`; an ambiguous name before `.count`
        assert_eq!(
            forms,
            [
                SiteForm::Other,
                SiteForm::Other,
                SiteForm::Other,
                SiteForm::Name
            ]
        );
    }

    /// The units the walk from the `count` `skip` bytes into the first `needle` spends, and its
    /// facts.
    fn walked(text: &str, needle: &str, skip: u32) -> (Option<SiteSyntax>, usize) {
        let grammar = Grammar::new(&GrammarSpec::orchard()).unwrap();
        let start = range_of(text, needle, 0).start().get().saturating_add(skip);
        let name = ByteRange::new(
            ByteOffset::new(start),
            ByteOffset::new(start.saturating_add(5)),
        )
        .unwrap();
        let (mut found, units) = super::facts(&grammar, &mut Worker::new(), text, &[name]);
        (found.pop().flatten(), units)
    }

    #[test]
    fn should_walk_in_linear_steps_when_a_site_is_deep_or_its_method_is_long() {
        // given: the review's H3 inputs (60,000 parentheses, a 6,000-term left-deep
        // concatenation), and a method of 6,000 statements around a shallow site
        let parens = format!(
            "class A {{ int count; int f() {{ return {}count{}; }} }}",
            "(".repeat(60_000),
            ")".repeat(60_000)
        );
        let concat = format!(
            "class A {{ int count; String f() {{ return count{}; }} }}",
            " + \"ab\"".repeat(6_000)
        );
        let long = format!(
            "class A {{ int count; int f() {{ int t = count;{} return t; }} }}",
            " t = t + 1;".repeat(6_000)
        );
        // when
        let deep = walked(&parens, "(count)", 1);
        let wide = walked(&concat, "return count", 7);
        let flat = walked(&long, "= count;", 2);
        // then: past the depth cap no facts, at most one step a level; a long method is one
        // scan, about one step a node
        assert_eq!(deep.0, None);
        assert!(
            deep.1 >= MAX_SITE_DEPTH && deep.1 <= MAX_SITE_DEPTH + 1,
            "{} units",
            deep.1
        );
        assert_eq!(wide.0, None);
        assert!(
            wide.1 >= MAX_SITE_DEPTH && wide.1 <= MAX_SITE_DEPTH + 1,
            "{} units",
            wide.1
        );
        assert!(flat.0.is_some());
        assert!(
            flat.1 <= long.len(),
            "{} units for {} bytes",
            flat.1,
            long.len()
        );
    }

    #[test]
    fn should_give_no_facts_when_the_file_has_escapes_or_errors() {
        // given
        let escaped = "class A { int count; int f() { return count; } } // \\u0041";
        let broken = "class A { int count; int f() { return count } }";
        // when
        let found = [facts(escaped, "count", 1), facts(broken, "count", 1)];
        // then
        assert_eq!(found, [None, None]);
    }
}
