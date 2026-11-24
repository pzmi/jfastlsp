//! The grammar boundary of ADR 0022 (amended 2026-11-17): the one place a grammar is chosen, and
//! the table every node kind, field and capture name the adapter uses is resolved into when the
//! indexer starts.
//!
//! A [`GrammarSpec`] is a tree-sitter language plus its own `tags.scm` and `locals.scm`
//! (`queries/<grammar>/`); [`Grammar::new`] compiles the queries and resolves every name of
//! [`KIND_NAMES`], [`FIELD_NAMES`] and [`CAPTURE_NAMES`] with `Language::id_for_node_kind`,
//! `Language::field_id_for_name` and `Query::capture_index_for_name`. A name the grammar or the
//! queries lack fails start-up with [`StartupError`] naming it, before any file is parsed, never
//! a silent empty match later. Switching back to upstream `tree-sitter-java` is a new
//! [`GrammarSpec`] with [`UPSTREAM_TAGS`] and [`UPSTREAM_LOCALS`]: the extraction reads only the
//! table, so nothing else changes (`tests/goldens.rs` runs both).
//!
//! Pinned by: `grammar::tests::should_fail_start_up_naming_a_kind_when_the_grammar_lacks_it`,
//! `grammar::tests::should_fail_start_up_naming_a_field_when_the_grammar_lacks_it`,
//! `grammar::tests::should_fail_start_up_naming_the_node_when_a_query_names_an_unknown_one` and
//! `tests/goldens.rs`.

use std::sync::Arc;

use jfl_application::GrammarId;
use tree_sitter::{Language, Query};

/// Our tags query for tree-sitter-java-orchard.
pub const ORCHARD_TAGS: &str = include_str!("../queries/orchard/tags.scm");
/// Our locals query for tree-sitter-java-orchard (nvim-treesitter's, modified; `queries/NOTICE`).
pub const ORCHARD_LOCALS: &str = include_str!("../queries/orchard/locals.scm");
/// Our tags query for upstream tree-sitter-java, the switch-back grammar.
pub const UPSTREAM_TAGS: &str = include_str!("../queries/upstream/tags.scm");
/// Our locals query for upstream tree-sitter-java (nvim-treesitter's, modified; `queries/NOTICE`).
pub const UPSTREAM_LOCALS: &str = include_str!("../queries/upstream/locals.scm");

/// The grammar crate's exact version, as `Cargo.toml` pins it (`=`): an input of the
/// `SyntaxKey`. Pinned by `tests/goldens.rs::should_name_the_locked_version_when_the_grammar_is_identified`.
pub const ORCHARD_VERSION: &str = "0.5.25";

/// A grammar and our queries for it.
#[derive(Clone)]
pub struct GrammarSpec {
    /// The crate's name and exact version.
    pub id: GrammarId,
    /// The language.
    pub language: Language,
    /// Our `tags.scm` for it.
    pub tags: &'static str,
    /// Our `locals.scm` for it.
    pub locals: &'static str,
}

impl core::fmt::Debug for GrammarSpec {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GrammarSpec").field("id", &self.id).finish()
    }
}

impl GrammarSpec {
    /// tree-sitter-java-orchard, the grammar of ADR 0022.
    #[must_use]
    pub fn orchard() -> Self {
        Self {
            id: GrammarId {
                name: "tree-sitter-java-orchard",
                version: ORCHARD_VERSION,
            },
            language: tree_sitter_java_orchard::LANGUAGE.into(),
            tags: ORCHARD_TAGS,
            locals: ORCHARD_LOCALS,
        }
    }
}

/// Why an indexer could not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupError {
    /// A node kind the adapter reads is not in the grammar.
    MissingKind(&'static str),
    /// A field the adapter reads is not in the grammar.
    MissingField(&'static str),
    /// A capture the adapter reads is not in the queries.
    MissingCapture(&'static str),
    /// A query does not compile against the grammar; the message names the node, field or
    /// capture and where.
    Query(String),
    /// tree-sitter cannot load the grammar (an ABI it does not speak).
    Language(String),
}

impl core::fmt::Display for StartupError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MissingKind(name) => write!(f, "the grammar has no node kind `{name}`"),
            Self::MissingField(name) => write!(f, "the grammar has no field `{name}`"),
            Self::MissingCapture(name) => write!(f, "the queries have no capture `@{name}`"),
            Self::Query(message) => write!(f, "a query does not compile: {message}"),
            Self::Language(message) => write!(f, "the grammar cannot be loaded: {message}"),
        }
    }
}

impl std::error::Error for StartupError {}

macro_rules! names {
    ($(#[$doc:meta])* $table:ident, $const:ident, $ty:ty, { $($field:ident = $name:expr, $named:expr;)* }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy)]
        pub(crate) struct $table {
            $(pub(crate) $field: $ty,)*
        }

        /// Every name of the table, with whether it is a named node (kinds only).
        #[cfg(test)]
        pub(crate) const $const: &[(&str, bool)] = &[$(($name, $named),)*];

        impl $table {
            /// Each name looked up by `lookup`, which fails naming the first that is missing.
            pub(crate) fn resolve(
                mut lookup: impl FnMut(&'static str, bool) -> Result<$ty, StartupError>,
            ) -> Result<Self, StartupError> {
                Ok(Self { $($field: lookup($name, $named)?,)* })
            }
        }
    };
}

names!(
    /// The node kinds the extraction reads, by id.
    Kinds, KIND_NAMES, u16, {
        identifier = "identifier", true;
        type_identifier = "type_identifier", true;
        scoped_type_identifier = "scoped_type_identifier", true;
        generic_type = "generic_type", true;
        annotated_type = "annotated_type", true;
        scoped_identifier = "scoped_identifier", true;
        block_comment = "block_comment", true;
        line_comment = "line_comment", true;
        modifiers = "modifiers", true;
        marker_annotation = "marker_annotation", true;
        annotation = "annotation", true;
        formal_parameter = "formal_parameter", true;
        spread_parameter = "spread_parameter", true;
        type_parameter = "type_parameter", true;
        type_bound = "type_bound", true;
        type_list = "type_list", true;
        extends_interfaces = "extends_interfaces", true;
        asterisk = "asterisk", true;
        super_ = "super", true;
        static_kw = "static", false;
        module_kw = "module", false;
        new_kw = "new", false;
        ellipsis = "...", false;
        class_declaration = "class_declaration", true;
        enum_declaration = "enum_declaration", true;
        record_declaration = "record_declaration", true;
        variable_declarator = "variable_declarator", true;
        class_body = "class_body", true;
    }
);

names!(
    /// The fields the extraction reads, by id.
    Fields, FIELD_NAMES, u16, {
        name = "name", true;
        body = "body", true;
        type_ = "type", true;
        parameters = "parameters", true;
        type_parameters = "type_parameters", true;
        superclass = "superclass", true;
        interfaces = "interfaces", true;
        permits = "permits", true;
        object = "object", true;
        arguments = "arguments", true;
        dimensions = "dimensions", true;
        constructor = "constructor", true;
        declarator = "declarator", true;
        field = "field", true;
        key = "key", true;
    }
);

names!(
    /// The captures of our queries, by index (the locals query's are offset past the tags query's,
    /// since both run as one query).
    Captures, CAPTURE_NAMES, u32, {
        class = "definition.class", true;
        interface = "definition.interface", true;
        enum_ = "definition.enum", true;
        record = "definition.record", true;
        annotation_type = "definition.annotation", true;
        method = "definition.method", true;
        constructor = "definition.constructor", true;
        field = "definition.field", true;
        enum_constant = "definition.enum_constant", true;
        annotation_element = "definition.annotation_element", true;
        package = "package", true;
        import = "import", true;
        comment = "comment", true;
        ref_call = "reference.call", true;
        ref_new = "reference.new", true;
        ref_constructor = "reference.constructor", true;
        ref_method_ref = "reference.method_ref", true;
        ref_field = "reference.field", true;
        ref_annotation = "reference.annotation", true;
        ref_type = "reference.type", true;
        ref_name = "reference.name", true;
        ref_element_key = "reference.element_key", true;
        ref_case_label = "reference.case_label", true;
        skip_label = "skip.label", true;
        skip_type_parameter = "skip.type_parameter", true;
        template = "template", true;
        scope = "local.scope", true;
        scope_callable = "local.scope.callable", true;
        scope_type = "local.scope.type", true;
        def_var = "local.definition.var", true;
        def_parameter = "local.definition.parameter", true;
        def_pattern = "local.definition.pattern", true;
    }
);

/// A grammar ready to index with: the language, our two queries checked and read as one table, and
/// every name resolved.
#[derive(Debug)]
pub struct Grammar {
    pub(crate) id: GrammarId,
    pub(crate) language: Language,
    pub(crate) kinds: Kinds,
    pub(crate) fields: Fields,
    pub(crate) captures: Captures,
    /// The queries as a table: for each node kind id, the captures its patterns give it.
    pub(crate) by_kind: Vec<Vec<u32>>,
}

impl Grammar {
    /// Compiles `spec`'s queries and resolves every name the adapter uses.
    pub fn new(spec: &GrammarSpec) -> Result<Arc<Self>, StartupError> {
        let kinds = Kinds::resolve(|name, named| kind_id(&spec.language, name, named))?;
        let fields = Fields::resolve(|name, _| field_id(&spec.language, name))?;
        // Both queries as one: compiled to check every name against the grammar, then read as the
        // kind-to-captures table one walk applies.
        let source = format!("{}\n{}", spec.tags, spec.locals);
        let query = Query::new(&spec.language, &source).map_err(|e| {
            StartupError::Query(format!(
                "{:?} `{}` at row {}, column {}",
                e.kind, e.message, e.row, e.column
            ))
        })?;
        let captures = Captures::resolve(|name, _| capture_id(&query, name))?;
        let by_kind = capture_table(&spec.language, &query, &source)?;
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&spec.language)
            .map_err(|e| StartupError::Language(e.to_string()))?;
        Ok(Arc::new(Self {
            id: spec.id,
            language: spec.language.clone(),
            kinds,
            fields,
            captures,
            by_kind,
        }))
    }

    /// The grammar's crate name and version.
    #[must_use]
    pub const fn id(&self) -> GrammarId {
        self.id
    }
}

/// Our queries hold single-node patterns only, `(kind) @capture`: a nested pattern makes
/// tree-sitter's query engine keep match states open across whole subtrees, and running the engine
/// at all cost half a parse again on the corpus (IDX-07's profile, ADR 0022 as amended
/// 2026-11-18). So the queries are compiled (which checks every node name against the grammar)
/// and then read as a table, kind to captures, that one cursor walk applies (`extract.rs`). A
/// pattern of any other shape fails start-up, naming it.
///
/// Pinned by: `grammar::tests::should_fail_start_up_naming_a_pattern_when_it_is_not_a_single_node`.
pub(crate) fn capture_table(
    language: &Language,
    query: &Query,
    source: &str,
) -> Result<Vec<Vec<u32>>, StartupError> {
    let mut table = vec![Vec::new(); language.node_kind_count()];
    let code = source
        .lines()
        .map(|line| line.split_once(';').map_or(line, |(code, _)| code))
        .collect::<Vec<_>>()
        .join(" ");
    let mut words = code.split_whitespace();
    while let Some(pattern) = words.next() {
        let capture = words.next().unwrap_or("");
        let kind = pattern
            .strip_prefix('(')
            .and_then(|p| p.strip_suffix(')'))
            .filter(|k| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        let (Some(kind), Some(name)) = (kind, capture.strip_prefix('@')) else {
            return Err(StartupError::Query(format!(
                "`{pattern} {capture}` is not a single-node pattern `(kind) @capture`"
            )));
        };
        let id = language.id_for_node_kind(kind, true);
        let index = query
            .capture_index_for_name(name)
            .ok_or_else(|| StartupError::Query(format!("no capture `@{name}`")))?;
        let slot = table
            .get_mut(usize::from(id))
            .filter(|_| id != 0)
            .ok_or_else(|| StartupError::Query(format!("the grammar has no node `{kind}`")))?;
        slot.push(index);
    }
    Ok(table)
}

/// The id of node kind `name` in `language`.
pub(crate) fn kind_id(
    language: &Language,
    name: &'static str,
    named: bool,
) -> Result<u16, StartupError> {
    // 0 is the end-of-input symbol: tree-sitter's "no such kind".
    match language.id_for_node_kind(name, named) {
        0 => Err(StartupError::MissingKind(name)),
        id => Ok(id),
    }
}

/// The id of field `name` in `language`.
pub(crate) fn field_id(language: &Language, name: &'static str) -> Result<u16, StartupError> {
    language
        .field_id_for_name(name)
        .map(core::num::NonZero::get)
        .ok_or(StartupError::MissingField(name))
}

/// The index of capture `name` in `query`.
pub(crate) fn capture_id(query: &Query, name: &'static str) -> Result<u32, StartupError> {
    query
        .capture_index_for_name(name)
        .ok_or(StartupError::MissingCapture(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `lookup` as a grammar would answer it after renaming `from` to `to`.
    fn renamed<T>(
        from: &'static str,
        to: &'static str,
        lookup: impl Fn(&'static str) -> Result<T, StartupError>,
    ) -> impl FnMut(&'static str, bool) -> Result<T, StartupError> {
        move |name, _| lookup(if name == from { to } else { name })
    }

    #[test]
    fn should_fail_start_up_naming_a_kind_when_the_grammar_lacks_it() {
        // given: `type_identifier` renamed, as a grammar release might
        let language: Language = tree_sitter_java_orchard::LANGUAGE.into();
        let lookup = renamed("type_identifier", "type_name", |name| {
            let named =
                KIND_NAMES.iter().any(|&(n, named)| n == name && named) || name == "type_name";
            kind_id(&language, name, named)
        });
        // when
        let result = Kinds::resolve(lookup);
        // then
        assert_eq!(result.err(), Some(StartupError::MissingKind("type_name")));
    }

    #[test]
    fn should_fail_start_up_naming_a_field_when_the_grammar_lacks_it() {
        // given
        let language: Language = tree_sitter_java_orchard::LANGUAGE.into();
        let lookup = renamed("superclass", "super_class", |name| {
            field_id(&language, name)
        });
        // when
        let result = Fields::resolve(lookup);
        // then
        assert_eq!(
            result.err(),
            Some(StartupError::MissingField("super_class"))
        );
    }

    #[test]
    fn should_resolve_every_table_name_when_upstream_is_loaded() {
        // given: the switch-back grammar
        let language: Language = tree_sitter_java::LANGUAGE.into();
        // when
        let kinds = Kinds::resolve(|name, named| kind_id(&language, name, named));
        let fields = Fields::resolve(|name, _| field_id(&language, name));
        // then
        assert!(
            kinds.is_ok() && fields.is_ok(),
            "{:?} {:?}",
            kinds.err(),
            fields.err()
        );
        assert_eq!(KIND_NAMES.len(), 28);
        assert!(!FIELD_NAMES.is_empty() && !CAPTURE_NAMES.is_empty());
    }

    #[test]
    fn should_fail_start_up_naming_the_node_when_a_query_names_an_unknown_one() {
        // given: the tags query with one node renamed
        let spec = GrammarSpec {
            tags: "(class_declaration) @definition.class\n(record_decl) @x",
            ..GrammarSpec::orchard()
        };
        // when
        let result = Grammar::new(&spec);
        // then
        let message = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(message.contains("record_decl"), "{message}");
    }

    #[test]
    fn should_fail_start_up_naming_a_pattern_when_it_is_not_a_single_node() {
        // given: a nested pattern, which the engine would run and the table cannot
        let tags = format!(
            "{ORCHARD_TAGS}\n(class_declaration name: (identifier) @name) @definition.class"
        );
        let spec = GrammarSpec {
            tags: Box::leak(tags.into_boxed_str()),
            ..GrammarSpec::orchard()
        };
        // when
        let result = Grammar::new(&spec);
        // then
        let message = result.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(message.contains("(class_declaration name:"), "{message}");
    }

    #[test]
    fn should_fail_start_up_naming_a_capture_when_the_queries_lack_it() {
        // given: the tags query without its `@definition.enum_constant` capture
        let tags = ORCHARD_TAGS.replace("@definition.enum_constant", "@definition.constant");
        let spec = GrammarSpec {
            tags: Box::leak(tags.into_boxed_str()),
            ..GrammarSpec::orchard()
        };
        // when
        let result = Grammar::new(&spec);
        // then
        assert_eq!(
            result.err(),
            Some(StartupError::MissingCapture("definition.enum_constant"))
        );
    }

    #[test]
    fn should_start_when_every_name_resolves_in_orchard() {
        // given
        let spec = GrammarSpec::orchard();
        // when
        let grammar = Grammar::new(&spec);
        // then
        assert!(grammar.is_ok(), "{:?}", grammar.err());
    }
}
