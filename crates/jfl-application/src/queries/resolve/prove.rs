//! The syntax-tree resolver (IDX-10's second slice): the bindings `super::super::admit` may
//! answer `exact`, proved from the syntax tree around the site ([`SiteSyntax`], the indexer's
//! parse of the request's file) and the summaries of the pinned generation.
//!
//! **The allowlist, in one place** ([`Construct`]). A site is proved only when its form and the
//! nodes around it match one construct below exactly, every type around it is a top-level or
//! member type the summary holds, and no local, parameter, pattern or type parameter of its name
//! is declared in the callables and types around it ([`SiteSyntax::shadowed`]). The indexer
//! classifies every node kind of its grammar (`jfl-adapter-java`'s `scopes.rs`); a kind it does
//! not place, or a node in a position it does not know, makes the site [`Around::Denied`].
//!
//! **The JLS rules implemented**, each conservative: anything the rule cannot see is "not
//! proved", never a guess.
//! - A bare name (JLS 6.5.6.1) and an unqualified call (15.12.1) in a method's or constructor's
//!   body: the innermost type around the site with a member of that name and kind is the owner
//!   (walked through its closed hierarchy, [`Resolver::members`]: private members, an
//!   interface's static methods and package-private members across packages not inherited);
//!   a type further in whose hierarchy leaves the index (or whose file uses Lombok) ends the
//!   proof. A field is the one the owner declares, or the one it inherits from a closed
//!   hierarchy; a method is the one overload applicable by arity in a closed hierarchy.
//!   Single-static and on-demand imports are not followed (not proved).
//! - `this.name` and `this.name(…)`: the same lookup in the innermost type alone (15.11.1,
//!   15.12.1).
//! - A simple type name or annotation name (6.5.5.1): in the body or a member's header, each
//!   type around the site from the innermost: its member types (declared ones hide inherited
//!   ones; an inherited one shadows the type's own name, 6.4.1; a hierarchy that leaves the index
//!   ends the proof), then the type's own name; then the file's own top-level types; then its
//!   single-type imports. In a type's header (its annotations, type parameters, `extends`,
//!   `implements`, `permits`) the same from the type around it, since a member's scope is the
//!   body (6.3, 8.1.6). The file's package and on-demand imports are never proved before the
//!   project model (PROJ).
//! - `new T(…)` without a class body: `T` proved as above, a class (not a record or enum) whose
//!   file does not use Lombok, then its one constructor applicable by arity (or the implicit one
//!   when it declares none).
//!
//! Pinned by: `jfl-adapter-java`'s `tests/definitions/main.rs` (each construct, and every site of the
//! fixture projects and the generated corpus against javac's recorded bindings) and
//! `prove::tests` (each frame shape refused).

use jfl_domain::{ByteRange, DeclId, DeclKind, RefShape, ReferenceSite};

use super::{Hit, Member, Members, Outcome, Resolver, field_outcome};
use crate::syntax_indexer::{Around, SiteForm, SiteSyntax};

/// A construct the resolver proves: the allowlist of `exact` beyond a declaration's own name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Construct {
    /// A bare name (a field) in a method's or constructor's body.
    NameInBody,
    /// An unqualified call in a method's or constructor's body.
    CallInBody,
    /// `this.name` in a method's or constructor's body.
    ThisNameInBody,
    /// `this.name(…)` in a method's or constructor's body.
    ThisCallInBody,
    /// A simple type or annotation name in a method's or constructor's body or header, or a
    /// field's header.
    TypeInMember,
    /// A simple type or annotation name in a type's header, resolved from the type around it.
    TypeInHeader,
    /// `new T(…)` without a class body, in a method's or constructor's body.
    NewInBody,
}

/// What a site's form and its innermost frame make of it, with the summary's shape it must have:
/// the allowlist. Anything not listed is not proved.
const ALLOWLIST: [(SiteForm, Frame, RefShape, Construct); 12] = [
    (
        SiteForm::Name,
        Frame::Body,
        RefShape::Name,
        Construct::NameInBody,
    ),
    (
        SiteForm::Call,
        Frame::Body,
        RefShape::Call,
        Construct::CallInBody,
    ),
    (
        SiteForm::ThisName,
        Frame::Body,
        RefShape::QualifiedName,
        Construct::ThisNameInBody,
    ),
    (
        SiteForm::ThisCall,
        Frame::Body,
        RefShape::QualifiedCall,
        Construct::ThisCallInBody,
    ),
    (
        SiteForm::Type,
        Frame::Body,
        RefShape::TypeUse,
        Construct::TypeInMember,
    ),
    (
        SiteForm::Type,
        Frame::CallableHeader,
        RefShape::TypeUse,
        Construct::TypeInMember,
    ),
    (
        SiteForm::Type,
        Frame::FieldHeader,
        RefShape::TypeUse,
        Construct::TypeInMember,
    ),
    (
        SiteForm::Annotation,
        Frame::CallableHeader,
        RefShape::Annotation,
        Construct::TypeInMember,
    ),
    (
        SiteForm::Annotation,
        Frame::FieldHeader,
        RefShape::Annotation,
        Construct::TypeInMember,
    ),
    (
        SiteForm::Type,
        Frame::TypeHeader,
        RefShape::TypeUse,
        Construct::TypeInHeader,
    ),
    (
        SiteForm::Annotation,
        Frame::TypeHeader,
        RefShape::Annotation,
        Construct::TypeInHeader,
    ),
    (
        SiteForm::New,
        Frame::Body,
        RefShape::New,
        Construct::NewInBody,
    ),
];

/// The innermost frame around a site that is not code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    /// A method's or constructor's body.
    Body,
    /// A method's or constructor's header.
    CallableHeader,
    /// A field's modifiers or type.
    FieldHeader,
    /// A type's header.
    TypeHeader,
}

/// A proved site's construct and the types whose scopes it is resolved in, innermost first.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Placed {
    construct: Construct,
    types: Vec<DeclId>,
}

/// The one declaration an outcome names, if it is exact.
fn exact(outcome: Outcome) -> Option<Hit> {
    match outcome {
        Outcome::Exact(hit) => Some(hit),
        _ => None,
    }
}

impl Resolver<'_> {
    /// The declaration the reference `site` of `file` binds to, when `syntax` places it on the
    /// allowlist and the JLS rules of the module documentation prove it; `None` otherwise.
    pub(crate) fn prove(
        &self,
        file: u32,
        site: &ReferenceSite,
        syntax: &SiteSyntax,
    ) -> Option<Hit> {
        if syntax.shadowed {
            return None;
        }
        let placed = self.place(file, site, syntax)?;
        let summary = &self.file(file)?.summary;
        let name = summary.str(site.name);
        let arity = site.shape.has_arity().then_some(site.arity.0);
        let innermost = || placed.types.first().map(|&decl| Hit { file, decl });
        let hit = match placed.construct {
            Construct::NameInBody => {
                self.proved_member(file, &placed.types, name, Member::Field, None)
            }
            Construct::CallInBody => {
                self.proved_member(file, &placed.types, name, Member::Method, arity)
            }
            Construct::ThisNameInBody => exact(field_outcome(self.members(
                innermost()?,
                name,
                Member::Field,
            ))),
            Construct::ThisCallInBody => {
                exact(self.overloads(self.members(innermost()?, name, Member::Method), arity))
            }
            Construct::TypeInMember | Construct::TypeInHeader => {
                self.proved_type(file, &placed.types, name)
            }
            Construct::NewInBody => {
                let owner = self.proved_type(file, &placed.types, name)?;
                if self.decl(owner)?.kind != DeclKind::Class {
                    return None;
                }
                exact(self.constructors(owner, arity))
            }
        };
        // A lookup cut by the deadline or the budget answered "outside" somewhere: no proof.
        hit.filter(|_| !self.cut())
    }

    /// The site's construct on the allowlist and the types around it, checked against the
    /// summary: each frame's declaration is there, and each encloses the one inside it.
    fn place(&self, file: u32, site: &ReferenceSite, syntax: &SiteSyntax) -> Option<Placed> {
        let mut frames = syntax.around.iter().copied().peekable();
        while frames.next_if_eq(&Around::Code).is_some() {}
        let (frame, member, header) = match frames.next()? {
            Around::CallableBody(name) => (Frame::Body, Some(name), None),
            Around::CallableHeader(name) => (Frame::CallableHeader, Some(name), None),
            Around::FieldHeader => (Frame::FieldHeader, None, None),
            Around::TypeHeader(name) => (Frame::TypeHeader, None, Some(name)),
            _ => return None,
        };
        let construct = ALLOWLIST
            .iter()
            .find(|(form, f, shape, _)| *form == syntax.form && *f == frame && *shape == site.shape)
            .map(|&(_, _, _, construct)| construct)?;
        let mut types = Vec::new();
        let mut unit = false;
        for around in frames {
            match around {
                Around::TypeBody(name) if !unit => types.push(self.declared_at(file, name, true)?),
                Around::Unit if !unit => unit = true,
                _ => return None,
            }
        }
        if !unit {
            return None;
        }
        let summary = &self.file(file)?.summary;
        let parent = |id: DeclId| summary.declaration(id).and_then(|d| d.parent);
        // Each type is a member of the next, the outermost a top-level type.
        for (i, &id) in types.iter().enumerate() {
            if parent(id) != types.get(i.saturating_add(1)).copied() {
                return None;
            }
        }
        if let Some(name) = member {
            let callable = self.declared_at(file, name, false)?;
            if parent(callable) != types.first().copied() {
                return None;
            }
        }
        if let Some(name) = header {
            let owner = self.declared_at(file, name, true)?;
            if parent(owner) != types.first().copied() {
                return None;
            }
        } else if types.is_empty() {
            return None;
        }
        Some(Placed { construct, types })
    }

    /// The declaration of `file` whose name is at `name`: a type when `is_type`, else a method
    /// or constructor.
    fn declared_at(&self, file: u32, name: ByteRange, is_type: bool) -> Option<DeclId> {
        let summary = &self.file(file)?.summary;
        summary
            .declarations()
            .find(|(_, d)| {
                d.name_range == name
                    && if is_type {
                        d.kind.is_type()
                    } else {
                        matches!(d.kind, DeclKind::Method | DeclKind::Constructor)
                    }
            })
            .map(|(id, _)| id)
    }

    /// A bare field name or unqualified call: the innermost of `types` with a member of `name`
    /// owns it; a type further in whose hierarchy may hold one outside the index ends the proof.
    fn proved_member(
        &self,
        file: u32,
        types: &[DeclId],
        name: &str,
        member: Member,
        arity: Option<u8>,
    ) -> Option<Hit> {
        for &decl in types {
            let members = self.members(Hit { file, decl }, name, member);
            if !members.hits.is_empty() {
                return exact(self.decide(members, member, arity));
            }
            if members.outside {
                return None;
            }
        }
        None
    }

    fn decide(&self, members: Members, member: Member, arity: Option<u8>) -> Outcome {
        match member {
            Member::Method => self.overloads(members, arity),
            _ => field_outcome(members),
        }
    }

    /// A simple type name seen from inside `types` (innermost first): each one's member types,
    /// then its own name; the file's top-level types; its single-type imports.
    fn proved_type(&self, file: u32, types: &[DeclId], name: &str) -> Option<Hit> {
        let summary = &self.file(file)?.summary;
        for &decl in types {
            let hit = Hit { file, decl };
            let members = self.members(hit, name, Member::Type);
            if members.own > 0 {
                // A declared member type hides every inherited one.
                return match members.hits.first() {
                    Some(&own) if members.own == 1 => Some(own),
                    _ => None,
                };
            }
            if members.outside {
                return None;
            }
            match members.hits.as_slice() {
                [] => {}
                [one] => return Some(*one),
                _ => return None,
            }
            if summary.declaration(decl).map(|d| summary.str(d.name)) == Some(name) {
                return Some(hit);
            }
        }
        let own: Vec<Hit> = self
            .types_named(name)
            .iter()
            .copied()
            .filter(|h| h.file == file && self.decl(*h).is_some_and(|d| d.parent.is_none()))
            .collect();
        if let [one] = own.as_slice() {
            return Some(*one);
        }
        if !own.is_empty() {
            return None;
        }
        let import = summary.imports().iter().find(|i| {
            !i.is_static
                && !i.on_demand
                && !i.module
                && summary.str(i.name).rsplit('.').next() == Some(name)
        })?;
        match self.types_qualified(summary.str(import.name)).as_slice() {
            [one] => Some(*one),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use jfl_domain::Language;
    use std::sync::Arc;

    use jfl_domain::{
        Arity, ByteOffset, DeclarationInput, Digest, Modifiers, RelPath, SummaryBuilder,
        SyntaxInputs, SyntaxKey,
    };

    use super::super::{FileEntry, Snapshot};
    use super::*;

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

    const A: (u32, u32) = (6, 7);
    const B: (u32, u32) = (27, 28);
    const F: (u32, u32) = (40, 41);

    /// `class A { int count; class Inner {} class B { class Inner {} void f() { count; Inner } } }`
    /// as a summary: `count` at 55, `Inner` at 62, both reference sites.
    fn snapshot() -> Snapshot {
        let mut b = SummaryBuilder::new(Language::Java);
        b.package("p").unwrap();
        let declare = |b: &mut SummaryBuilder,
                       kind: DeclKind,
                       parent: Option<DeclId>,
                       name: &str,
                       at: (u32, u32),
                       whole: (u32, u32)| {
            b.declaration(&DeclarationInput {
                kind,
                modifiers: Modifiers::empty(),
                parent,
                name,
                name_range: range(at.0, at.1),
                range: range(whole.0, whole.1),
                signature: name,
                params: &[],
                supertypes: &[],
                permits: &[],
                annotations: &[],
                javadoc: None,
            })
            .unwrap()
        };
        let a = declare(&mut b, DeclKind::Class, None, "A", A, (0, 100));
        declare(
            &mut b,
            DeclKind::Field,
            Some(a),
            "count",
            (14, 19),
            (10, 20),
        );
        declare(
            &mut b,
            DeclKind::Class,
            Some(a),
            "Inner",
            (22, 23),
            (20, 24),
        );
        let class_b = declare(&mut b, DeclKind::Class, Some(a), "B", B, (25, 90));
        declare(
            &mut b,
            DeclKind::Class,
            Some(class_b),
            "Inner",
            (30, 31),
            (29, 33),
        );
        declare(&mut b, DeclKind::Method, Some(class_b), "f", F, (35, 80));
        b.reference("count", range(55, 60), RefShape::Name, Arity(0))
            .unwrap();
        b.reference("Inner", range(62, 67), RefShape::TypeUse, Arity(0))
            .unwrap();
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
            summary: Arc::new(b.finish()),
        }])
    }

    fn site(snapshot: &Snapshot, at: usize) -> ReferenceSite {
        *snapshot
            .file(0)
            .unwrap()
            .summary
            .references()
            .get(at)
            .unwrap()
    }

    fn in_body() -> Vec<Around> {
        vec![
            Around::Code,
            Around::CallableBody(range(F.0, F.1)),
            Around::TypeBody(range(B.0, B.1)),
            Around::TypeBody(range(A.0, A.1)),
            Around::Unit,
        ]
    }

    fn proved(
        snapshot: &Snapshot,
        at: usize,
        form: SiteForm,
        around: Vec<Around>,
    ) -> Option<String> {
        let resolver = Resolver::new(snapshot);
        let syntax = SiteSyntax {
            form,
            around,
            shadowed: false,
        };
        let hit = resolver.prove(0, &site(snapshot, at), &syntax)?;
        Some(snapshot.qualified(hit))
    }

    #[test]
    fn should_prove_the_outer_field_and_the_inner_type_when_the_frames_are_allowed() {
        // given
        let snapshot = snapshot();
        // when
        let field = proved(&snapshot, 0, SiteForm::Name, in_body());
        let ty = proved(&snapshot, 1, SiteForm::Type, in_body());
        // then: `count` from B's enclosing A; `Inner` B's own member type
        assert_eq!(field.as_deref(), Some("p.A.count"));
        assert_eq!(ty.as_deref(), Some("p.A.B.Inner"));
    }

    #[test]
    fn should_resolve_from_the_enclosing_type_when_the_site_is_in_a_header() {
        // given: `Inner` in B's header, whose scope is A's body (JLS 6.3, the review's N3)
        let snapshot = snapshot();
        let header = vec![
            Around::Code,
            Around::TypeHeader(range(B.0, B.1)),
            Around::TypeBody(range(A.0, A.1)),
            Around::Unit,
        ];
        // when
        let ty = proved(&snapshot, 1, SiteForm::Type, header);
        // then
        assert_eq!(ty.as_deref(), Some("p.A.Inner"));
    }

    #[test]
    fn should_prove_nothing_when_any_frame_or_fact_is_off_the_allowlist() {
        // given
        let snapshot = snapshot();
        let resolver = Resolver::new(&snapshot);
        let count = site(&snapshot, 0);
        let with = |around: Vec<Around>, form: SiteForm, shadowed: bool| SiteSyntax {
            form,
            around,
            shadowed,
        };
        let mut denied = in_body();
        denied.insert(1, Around::Denied("lambda_expression"));
        let mut no_unit = in_body();
        no_unit.pop();
        let mut swapped = in_body();
        swapped.swap(2, 3);
        let initializer = vec![
            Around::Code,
            Around::TypeBody(range(B.0, B.1)),
            Around::TypeBody(range(A.0, A.1)),
            Around::Unit,
        ];
        let mut other_callable = in_body();
        if let Some(frame) = other_callable.get_mut(1) {
            *frame = Around::CallableBody(range(14, 19));
        }
        let mut header_of_field = in_body();
        if let Some(frame) = header_of_field.get_mut(1) {
            *frame = Around::FieldHeader;
        }
        // when
        let refused = [
            with(in_body(), SiteForm::Name, true),
            with(denied, SiteForm::Name, false),
            with(no_unit, SiteForm::Name, false),
            with(swapped, SiteForm::Name, false),
            with(initializer, SiteForm::Name, false),
            with(other_callable, SiteForm::Name, false),
            with(in_body(), SiteForm::Call, false),
            with(in_body(), SiteForm::Other, false),
            with(header_of_field, SiteForm::Name, false),
        ]
        .map(|syntax| resolver.prove(0, &count, &syntax));
        // then: shadowed, a denied node, no unit, types out of order, an initializer, a frame
        // naming no callable, a form the summary's shape contradicts, another form, a bare
        // name in a field's header
        assert_eq!(refused, [None; 9]);
    }
}
