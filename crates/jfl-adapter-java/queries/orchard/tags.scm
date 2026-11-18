; jfastlsp's tags query for tree-sitter-java-orchard (ADR 0022, IDX-07). Our own: it is not the
; grammar's queries/tags.scm. Every capture name is resolved when the indexer starts
; (src/grammar.rs), and the Rust side reads the captured nodes' fields through the start-up table;
; a pattern here that names a node the grammar lacks fails start-up, naming it.
;

; Every pattern is one node: a nested pattern keeps a match state open across the whole node it
; starts at (a method's parameters pattern, across its body), and those states were a third of the
; indexing time (IDX-07's profile). The Rust side reads names and fields through the table.
;
; Declarations, `@definition.<kind>` (a field or constant declaration is one per declarator; a
; record's components come from its `parameters`).
(class_declaration) @definition.class
(interface_declaration) @definition.interface
(enum_declaration) @definition.enum
(record_declaration) @definition.record
(annotation_type_declaration) @definition.annotation
(method_declaration) @definition.method
(constructor_declaration) @definition.constructor
(compact_constructor_declaration) @definition.constructor
(field_declaration) @definition.field
(constant_declaration) @definition.field
(enum_constant) @definition.enum_constant
(annotation_type_element_declaration) @definition.annotation_element

; The package and each import: never reference sites themselves (an import adds the imported
; simple name as one, in Rust).
(package_declaration) @package
(import_declaration) @import

; Comments: never reference sites; a `/**` comment before a declaration is its Javadoc, and a
; comment inside a header is left out of its signature.
(block_comment) @comment
(line_comment) @comment

; Reference sites. A place captured several times keeps the most specific shape (Rust ranks them);
; the catch-alls last keep every other identifier, so the `possible` superset loses no use.
(method_invocation) @reference.call
(object_creation_expression) @reference.new
(explicit_constructor_invocation) @reference.constructor
(method_reference) @reference.method_ref
(field_access) @reference.field
(marker_annotation) @reference.annotation
(annotation) @reference.annotation
; Names that are never a local's, whatever a local in scope is called: an annotation element's key
; (`@A(value = 2)`, a use of `A.value()`) and a bare `case` label (`case RED:`, an enum constant).
(element_value_pair) @reference.element_key
(switch_label) @reference.case_label
(type_identifier) @reference.type
(identifier) @reference.name

; Never reference sites: labels, and a type parameter's own name.
(labeled_statement) @skip.label
(break_statement) @skip.label
(continue_statement) @skip.label
(type_parameter) @skip.type_parameter

; Not Java: a string template parses, but was withdrawn in Java 23 (ADR 0022).
(template_expression) @template
