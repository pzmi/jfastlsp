; jfastlsp's locals query for upstream tree-sitter-java, the switch-back grammar (ADR 0022, IDX-07);
; the same patterns as orchard/locals.scm today.
;
; Derived from nvim-treesitter's queries/java/locals.scm (Apache-2.0, Copyright the
; nvim-treesitter authors; https://github.com/nvim-treesitter/nvim-treesitter, master as fetched
; 2026-11-18; the licence is ../NOTICE's). Modified: reduced to what the syntax index needs, which
; is to drop reference sites bound to a local variable or a parameter (plan §5.5) and never one
; whose binding is unclear:
; - parameters are captured only where they are a method's, a constructor's, a lambda's or a
;   catch clause's, never a record header's (record components are fields), and every pattern is
;   one node, its names read through the start-up table in Rust (src/extract.rs);
; - try-with-resources variables and switch blocks were added;
; - a type body is a barrier (`@local.scope.type`; an anonymous class's body is marked as one in
;   Rust): a field declared or inherited there may shadow an outer local, so no use inside it binds
;   to a local outside it;
; - pattern variables are captured only to keep their names out of the reference sites, and never
;   bind a use: their scope follows flow (`if (!(o instanceof S s)) return; s.f();`), which a
;   nesting of nodes cannot express;
; - if, while and do bodies, the declarations' own names, the imports and `@local.reference` were
;   dropped (the tags query captures the reference sites).

; SCOPES. A callable's parameters are read from its `parameters` field (in Rust, through the
; table), so a record header's are never taken for a callable's.
(block) @local.scope
(constructor_body) @local.scope
(switch_block) @local.scope
(catch_clause) @local.scope
(for_statement) @local.scope
(enhanced_for_statement) @local.scope
(try_with_resources_statement) @local.scope
(method_declaration) @local.scope.callable
(constructor_declaration) @local.scope.callable
(lambda_expression) @local.scope.callable

; BARRIERS: type bodies. An anonymous class's body (an `object_creation_expression`'s, an enum
; constant's) also holds members with no symbol identity; Rust marks those from the expression and
; the constant.
(class_body) @local.scope.type
(interface_body) @local.scope.type
(enum_body) @local.scope.type
(annotation_type_body) @local.scope.type

; DEFINITIONS: each declarator's name, the loop variable's, the resource's, the caught
; exception's.
(local_variable_declaration) @local.definition.var
(enhanced_for_statement) @local.definition.var
(resource) @local.definition.var
(catch_formal_parameter) @local.definition.parameter

; Pattern variables: names only, never bound.
(instanceof_expression) @local.definition.pattern
(type_pattern) @local.definition.pattern
(record_pattern_component) @local.definition.pattern
