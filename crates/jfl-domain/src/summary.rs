//! Per-file syntax summaries (plan §2.1, §5.4, §5.5): what the syntax index keeps of a Java file.
//!
//! A [`FileSummary`] holds the package, the imports, every declaration (kind, name, signature
//! with annotations, modifiers, Javadoc range, supertypes and permitted subtypes by name,
//! parameter types, annotation names) and every reference site (simple
//! name, arity, syntactic shape). The extractor (IDX-07) fills it through [`SummaryBuilder`] and
//! leaves comments and string literals out of the reference sites; the store (IDX-08) persists its
//! [`SummaryParts`] with postcard and reads them back through [`FileSummary::from_parts`], which
//! checks every index, so a corrupt entry is refused and never a panic.
//!
//! **Compact on purpose.** A daemon holds one summary per file for 30k+ files, so every string
//! lives once in one arena (`text`) and everything else is fixed-size plain data that refers to
//! it by a 4-byte [`StrId`]: a [`Declaration`] is 80 bytes, a [`ReferenceSite`] and an [`Import`]
//! 16, and a name repeated across reference sites is stored once. Sizes are pinned by
//! `summary::tests::should_keep_records_compact_when_the_layout_changes`; the bytes of a whole
//! summary are [`FileSummary::heap_bytes`], printed by `benches/envelope.rs` for the corpus's
//! mean file (about 10 KB, reference sites the largest part; plan §5.5 keeps them on disk on
//! the large tier). Rejected alternatives: a `String`
//! per name (an allocation and a 24-byte header each, where a `StrId` is 4 bytes), and a global
//! interner across files (shared mutable state in a pure crate, and summaries keyed by content
//! must stay independent of each other).
//!
//! **The declaration API digest** ([`FileSummary::api_digest`]) is what `SemanticKey` reads
//! (plan §5.4): a declaration's identity and *signature* (kind, modifiers, name, signature text
//! with annotations, parameter types, supertypes, permitted subtypes, annotation names), every
//! enclosing type's header (its type parameters and bounds), the file's package and its imports
//! (sorted), and for a type the signatures of its direct members. It leaves out every byte range,
//! Javadoc, field initializer and anything inside a body, so an edit inside a method body or a
//! shift of lines changes no dependent's key, while a new overload, a changed parameter, an added
//! `@Getter`, a changed owner bound or a changed import does. Pinned by
//! `summary::tests::should_keep_the_api_digest_when_only_a_body_or_position_changes` and
//! `summary::tests::should_change_the_api_digest_when_the_signature_or_member_set_changes`
//! and `summary::tests::should_change_a_member_digest_when_an_annotation_or_owner_bound_changes`.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::mem::size_of;

use crate::digest::{Digest, Encoder, KeyHasher};
use crate::symbol::{DeclKind, MemberId, SymbolError, SymbolId};
use crate::text::ByteRange;

/// A string of a summary: an index into [`SummaryParts::strings`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StrId(pub u32);

/// A declaration of a summary: an index into [`SummaryParts::declarations`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeclId(pub u32);

/// Bytes `start..start + len` of [`SummaryParts::text`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Span {
    /// The first byte.
    pub start: u32,
    /// How many bytes.
    pub len: u32,
}

/// Entries `start..start + len` of [`SummaryParts::type_names`]: a list of type names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ListRef {
    /// The first entry.
    pub start: u32,
    /// How many entries.
    pub len: u32,
}

/// Java's modifiers, plus `varargs` for a method whose last parameter is `T...`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Modifiers(u16);

impl Modifiers {
    /// `public`.
    pub const PUBLIC: Self = Self(1);
    /// `protected`.
    pub const PROTECTED: Self = Self(1 << 1);
    /// `private`.
    pub const PRIVATE: Self = Self(1 << 2);
    /// `static`.
    pub const STATIC: Self = Self(1 << 3);
    /// `final`.
    pub const FINAL: Self = Self(1 << 4);
    /// `abstract`.
    pub const ABSTRACT: Self = Self(1 << 5);
    /// `default` (an interface method with a body).
    pub const DEFAULT: Self = Self(1 << 6);
    /// `sealed`.
    pub const SEALED: Self = Self(1 << 7);
    /// `non-sealed`.
    pub const NON_SEALED: Self = Self(1 << 8);
    /// `synchronized`.
    pub const SYNCHRONIZED: Self = Self(1 << 9);
    /// `native`.
    pub const NATIVE: Self = Self(1 << 10);
    /// `transient`.
    pub const TRANSIENT: Self = Self(1 << 11);
    /// `volatile`.
    pub const VOLATILE: Self = Self(1 << 12);
    /// `strictfp`.
    pub const STRICTFP: Self = Self(1 << 13);
    /// The last parameter is variable-arity (`T...`): a call may pass any number from there.
    pub const VARARGS: Self = Self(1 << 14);

    const KNOWN: u16 = (1 << 15) - 1;

    /// No modifier.
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// The modifiers of these bits; `None` if a bit has no meaning (a stored summary's field).
    #[must_use]
    pub const fn from_bits(bits: u16) -> Option<Self> {
        if bits & !Self::KNOWN == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    /// The bits.
    #[must_use]
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// `self` and `other` together.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every modifier of `other` is in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// The syntactic shape of a reference site: what the `possible` superset matches on beside the
/// name (plan §2.1). The `u8` code is stable, as [`DeclKind::code`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RefShape {
    /// `name(args)`: an unqualified call.
    Call,
    /// `expr.name(args)`: a call on a receiver.
    QualifiedCall,
    /// `new Name(args)`: an instance creation.
    New,
    /// `Expr::name` or `Type::new`: matched by name whatever the arity (plan §2.1).
    MethodRef,
    /// `Name` where a type is expected (a declaration's type, a cast, `extends`, a type argument).
    TypeUse,
    /// A bare `name` in an expression: a local, a field or a type.
    Name,
    /// `expr.name` that is not a call: a field access or a qualified type name.
    QualifiedName,
    /// `@Name`.
    Annotation,
}

impl RefShape {
    /// Every shape, in code order.
    pub const ALL: [Self; 8] = [
        Self::Call,
        Self::QualifiedCall,
        Self::New,
        Self::MethodRef,
        Self::TypeUse,
        Self::Name,
        Self::QualifiedName,
        Self::Annotation,
    ];

    /// The stable code of this shape.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Call => 0,
            Self::QualifiedCall => 1,
            Self::New => 2,
            Self::MethodRef => 3,
            Self::TypeUse => 4,
            Self::Name => 5,
            Self::QualifiedName => 6,
            Self::Annotation => 7,
        }
    }

    /// The shape with this code, if any.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Call),
            1 => Some(Self::QualifiedCall),
            2 => Some(Self::New),
            3 => Some(Self::MethodRef),
            4 => Some(Self::TypeUse),
            5 => Some(Self::Name),
            6 => Some(Self::QualifiedName),
            7 => Some(Self::Annotation),
            _ => None,
        }
    }

    /// Whether a site of this shape passes arguments, so its [`Arity`] means something.
    #[must_use]
    pub const fn has_arity(self) -> bool {
        matches!(self, Self::Call | Self::QualifiedCall | Self::New)
    }
}

/// How many arguments a call passes, saturated at 255.
///
/// A method declares at most 255 parameters (JVMS §4.3.3), so a saturated count still compares
/// correctly with every declaration: equal below 255, and at least as large as any fixed
/// prefix of a varargs method. Saturating keeps a (malformed) call with more arguments in the
/// `possible` superset instead of refusing the whole file. Pinned by
/// `summary::tests::should_saturate_arity_when_a_call_passes_more_than_255`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Arity(pub u8);

impl Arity {
    /// The arity of a call passing `count` arguments.
    #[must_use]
    pub fn of(count: usize) -> Self {
        Self(u8::try_from(count).unwrap_or(u8::MAX))
    }
}

/// An `import` declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Import {
    /// The dotted name, without `.*`.
    pub name: StrId,
    /// `import static`.
    pub is_static: bool,
    /// `.*`: every member of the named package or type.
    pub on_demand: bool,
    /// `import module M;` (JEP 511, Java 25): every package `M` exports, on demand; `name` is the
    /// module's name, and `is_static` and `on_demand` are false.
    pub module: bool,
    /// The whole declaration.
    pub range: ByteRange,
}

/// A declaration: a type, or a member of one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Declaration {
    /// What it declares.
    pub kind: DeclKind,
    /// Its modifiers.
    pub modifiers: Modifiers,
    /// The enclosing declaration, always earlier in the list; `None` for a top-level type.
    pub parent: Option<DeclId>,
    /// The simple name (`<init>` is not used: a constructor's name is its type's).
    pub name: StrId,
    /// The name's bytes.
    pub name_range: ByteRange,
    /// The whole declaration, Javadoc excluded.
    pub range: ByteRange,
    /// The header as written, white space collapsed, without its body, Javadoc or a field's
    /// initializer, and *with* its annotations and their arguments (`@Getter(AccessLevel.NONE)`
    /// changes members, so it is API): what hover shows and what the API digest reads.
    pub signature: StrId,
    /// The parameter types, erased as `SymbolId` writes them (a type variable as its first
    /// bound, [`erase_parameter_type`](crate::symbol::erase_parameter_type)); empty for a
    /// non-callable.
    pub params: ListRef,
    /// The `extends` and `implements` names as written, erased: the declaration's supertypes.
    pub supertypes: ListRef,
    /// The `permits` names as written, erased: its permitted *subtypes*, kept apart so that no
    /// walk up the hierarchy follows one.
    pub permits: ListRef,
    /// The annotations' names as written (`Getter`, `lombok.Data`), without arguments: what a
    /// consumer looks for generated members by. Also in the API digest, beside `signature`.
    pub annotations: ListRef,
    /// The Javadoc comment, if one precedes it.
    pub javadoc: Option<ByteRange>,
}

/// A place a name is used, outside comments and string literals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReferenceSite {
    /// The simple name used.
    pub name: StrId,
    /// The name's bytes.
    pub range: ByteRange,
    /// The syntactic shape.
    pub shape: RefShape,
    /// The number of arguments, for a shape that [`RefShape::has_arity`]; 0 otherwise.
    pub arity: Arity,
}

/// A summary as plain data: what the store writes and reads (IDX-08, postcard). Build a
/// [`FileSummary`] from it with [`FileSummary::from_parts`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct SummaryParts {
    /// Every string of the summary, once.
    pub text: Box<str>,
    /// Where each [`StrId`]'s string lies in `text`.
    pub strings: Box<[Span]>,
    /// The package name, dotted; `None` for the default package.
    pub package: Option<StrId>,
    /// The imports, in source order.
    pub imports: Box<[Import]>,
    /// The declarations, each after its parent.
    pub declarations: Box<[Declaration]>,
    /// The lists that [`ListRef`]s point into.
    pub type_names: Box<[StrId]>,
    /// The reference sites, in source order.
    pub references: Box<[ReferenceSite]>,
}

/// Why a summary or a part of one is refused. The values carry indices, never source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SummaryError {
    /// More than `u32::MAX` bytes or entries.
    #[error("summary is larger than a 32-bit index can address")]
    TooLarge,
    /// A span outside `text`, or one that splits a character.
    #[error("string {index} does not lie on character boundaries of the text")]
    BadSpan {
        /// The string's index.
        index: u32,
    },
    /// A [`StrId`] with no string.
    #[error("string id {id} is out of range")]
    UnknownString {
        /// The id.
        id: u32,
    },
    /// A [`ListRef`] outside `type_names`.
    #[error("type list at {start} of {len} is out of range")]
    BadList {
        /// The first entry.
        start: u32,
        /// How many entries.
        len: u32,
    },
    /// A declaration whose parent is not an earlier declaration.
    #[error("declaration {index} names a parent that does not precede it")]
    BadParent {
        /// The declaration's index.
        index: u32,
    },
}

/// One declaration to add to a [`SummaryBuilder`].
#[derive(Debug, Clone, Copy)]
pub struct DeclarationInput<'a> {
    /// What it declares.
    pub kind: DeclKind,
    /// Its modifiers.
    pub modifiers: Modifiers,
    /// The enclosing declaration, already added.
    pub parent: Option<DeclId>,
    /// The simple name.
    pub name: &'a str,
    /// The name's bytes.
    pub name_range: ByteRange,
    /// The whole declaration.
    pub range: ByteRange,
    /// The header without body or Javadoc.
    pub signature: &'a str,
    /// The parameter types, erased.
    pub params: &'a [&'a str],
    /// The `extends` and `implements` names, erased.
    pub supertypes: &'a [&'a str],
    /// The `permits` names, erased.
    pub permits: &'a [&'a str],
    /// The annotations' names as written.
    pub annotations: &'a [&'a str],
    /// The Javadoc comment.
    pub javadoc: Option<ByteRange>,
}

fn index(len: usize) -> Result<u32, SummaryError> {
    u32::try_from(len).map_err(|_| SummaryError::TooLarge)
}

/// Builds a [`FileSummary`], interning every string once (IDX-07's extraction fills it).
#[derive(Debug, Default)]
pub struct SummaryBuilder {
    text: String,
    strings: Vec<Span>,
    interned: BTreeMap<Box<str>, StrId>,
    package: Option<StrId>,
    imports: Vec<Import>,
    declarations: Vec<Declaration>,
    type_names: Vec<StrId>,
    references: Vec<ReferenceSite>,
}

impl SummaryBuilder {
    /// An empty summary.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The id of `value`, stored on first use.
    fn intern(&mut self, value: &str) -> Result<StrId, SummaryError> {
        if let Some(&id) = self.interned.get(value) {
            return Ok(id);
        }
        let id = StrId(index(self.strings.len())?);
        let start = index(self.text.len())?;
        let len = index(value.len())?;
        index(self.text.len().saturating_add(value.len()))?;
        self.text.push_str(value);
        self.strings.push(Span { start, len });
        self.interned.insert(Box::from(value), id);
        Ok(id)
    }

    fn list(&mut self, names: &[&str]) -> Result<ListRef, SummaryError> {
        let start = index(self.type_names.len())?;
        let len = index(names.len())?;
        for name in names {
            let id = self.intern(name)?;
            self.type_names.push(id);
        }
        Ok(ListRef { start, len })
    }

    /// Sets the package name (dotted).
    pub fn package(&mut self, name: &str) -> Result<(), SummaryError> {
        self.package = Some(self.intern(name)?);
        Ok(())
    }

    /// Adds an import of `name` (dotted, without `.*`).
    pub fn import(
        &mut self,
        name: &str,
        is_static: bool,
        on_demand: bool,
        range: ByteRange,
    ) -> Result<(), SummaryError> {
        let name = self.intern(name)?;
        self.imports.push(Import {
            name,
            is_static,
            on_demand,
            module: false,
            range,
        });
        Ok(())
    }

    /// Adds `import module name;` (JEP 511).
    pub fn module_import(&mut self, name: &str, range: ByteRange) -> Result<(), SummaryError> {
        let name = self.intern(name)?;
        self.imports.push(Import {
            name,
            is_static: false,
            on_demand: false,
            module: true,
            range,
        });
        Ok(())
    }

    /// Adds a declaration after its parent and returns its id.
    pub fn declaration(&mut self, input: &DeclarationInput<'_>) -> Result<DeclId, SummaryError> {
        let id = DeclId(index(self.declarations.len())?);
        if input.parent.is_some_and(|parent| parent >= id) {
            return Err(SummaryError::BadParent { index: id.0 });
        }
        let declaration = Declaration {
            kind: input.kind,
            modifiers: input.modifiers,
            parent: input.parent,
            name: self.intern(input.name)?,
            name_range: input.name_range,
            range: input.range,
            signature: self.intern(input.signature)?,
            params: self.list(input.params)?,
            supertypes: self.list(input.supertypes)?,
            permits: self.list(input.permits)?,
            annotations: self.list(input.annotations)?,
            javadoc: input.javadoc,
        };
        self.declarations.push(declaration);
        Ok(id)
    }

    /// Adds a reference site.
    pub fn reference(
        &mut self,
        name: &str,
        range: ByteRange,
        shape: RefShape,
        arity: Arity,
    ) -> Result<(), SummaryError> {
        let name = self.intern(name)?;
        self.references.push(ReferenceSite {
            name,
            range,
            shape,
            arity,
        });
        Ok(())
    }

    /// The summary.
    #[must_use]
    pub fn finish(self) -> FileSummary {
        FileSummary {
            parts: SummaryParts {
                text: self.text.into_boxed_str(),
                strings: self.strings.into_boxed_slice(),
                package: self.package,
                imports: self.imports.into_boxed_slice(),
                declarations: self.declarations.into_boxed_slice(),
                type_names: self.type_names.into_boxed_slice(),
                references: self.references.into_boxed_slice(),
            },
        }
    }
}

/// A checked summary: every id and list in range, every span on character boundaries, every
/// parent before its child.
///
/// Pinned by: `summary::tests::should_refuse_parts_when_an_index_is_out_of_range` and
/// `tests/properties.rs::should_rebuild_the_same_summary_when_its_parts_round_trip`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct FileSummary {
    parts: SummaryParts,
}

impl FileSummary {
    /// Checks `parts` (from the store, so untrusted) and wraps them.
    pub fn from_parts(parts: SummaryParts) -> Result<Self, SummaryError> {
        index(parts.text.len())?;
        index(parts.declarations.len())?;
        for (i, span) in parts.strings.iter().enumerate() {
            let start = usize::try_from(span.start).map_err(|_| SummaryError::TooLarge)?;
            let len = usize::try_from(span.len).map_err(|_| SummaryError::TooLarge)?;
            if parts.text.get(start..start.saturating_add(len)).is_none() {
                return Err(SummaryError::BadSpan { index: index(i)? });
            }
        }
        let strings = parts.strings.len();
        let check = |id: StrId| match usize::try_from(id.0) {
            Ok(at) if at < strings => Ok(()),
            _ => Err(SummaryError::UnknownString { id: id.0 }),
        };
        let lists = parts.type_names.len();
        let check_list = |list: ListRef| {
            let start = usize::try_from(list.start).unwrap_or(usize::MAX);
            let len = usize::try_from(list.len).unwrap_or(usize::MAX);
            if start.checked_add(len).is_some_and(|end| end <= lists) {
                Ok(())
            } else {
                Err(SummaryError::BadList {
                    start: list.start,
                    len: list.len,
                })
            }
        };
        parts.package.map_or(Ok(()), check)?;
        for import in &parts.imports {
            check(import.name)?;
        }
        for &name in &parts.type_names {
            check(name)?;
        }
        for reference in &parts.references {
            check(reference.name)?;
        }
        for (i, declaration) in parts.declarations.iter().enumerate() {
            let at = index(i)?;
            if declaration.parent.is_some_and(|parent| parent.0 >= at) {
                return Err(SummaryError::BadParent { index: at });
            }
            check(declaration.name)?;
            check(declaration.signature)?;
            check_list(declaration.params)?;
            check_list(declaration.supertypes)?;
            check_list(declaration.permits)?;
            check_list(declaration.annotations)?;
        }
        Ok(Self { parts })
    }

    /// The parts, to persist.
    #[must_use]
    pub const fn parts(&self) -> &SummaryParts {
        &self.parts
    }

    /// The parts, by value.
    #[must_use]
    pub fn into_parts(self) -> SummaryParts {
        self.parts
    }

    /// The string `id` names (empty for an id of another summary).
    #[must_use]
    pub fn str(&self, id: StrId) -> &str {
        usize::try_from(id.0)
            .ok()
            .and_then(|at| self.parts.strings.get(at))
            .and_then(|span| {
                let start = usize::try_from(span.start).ok()?;
                let len = usize::try_from(span.len).ok()?;
                self.parts.text.get(start..start.checked_add(len)?)
            })
            .unwrap_or("")
    }

    /// The type names `list` holds.
    pub fn list(&self, list: ListRef) -> impl Iterator<Item = &str> {
        let start = usize::try_from(list.start).unwrap_or(usize::MAX);
        let len = usize::try_from(list.len).unwrap_or(0);
        self.parts
            .type_names
            .get(start..start.saturating_add(len))
            .unwrap_or(&[])
            .iter()
            .map(|&id| self.str(id))
    }

    /// The package name, dotted; `None` for the default package.
    #[must_use]
    pub fn package(&self) -> Option<&str> {
        self.parts.package.map(|id| self.str(id))
    }

    /// The imports, in source order.
    #[must_use]
    pub fn imports(&self) -> &[Import] {
        &self.parts.imports
    }

    /// The declarations with their ids, each after its parent.
    pub fn declarations(&self) -> impl Iterator<Item = (DeclId, &Declaration)> {
        (0_u32..)
            .zip(self.parts.declarations.iter())
            .map(|(i, declaration)| (DeclId(i), declaration))
    }

    /// The declaration `id` names.
    #[must_use]
    pub fn declaration(&self, id: DeclId) -> Option<&Declaration> {
        self.parts.declarations.get(usize::try_from(id.0).ok()?)
    }

    /// The reference sites, in source order.
    #[must_use]
    pub fn references(&self) -> &[ReferenceSite] {
        &self.parts.references
    }

    /// The declarations from the top-level type down to `id`, or `None` for an unknown id.
    fn chain(&self, id: DeclId) -> Option<Vec<&Declaration>> {
        let mut chain = Vec::new();
        let mut next = Some(id);
        while let Some(at) = next {
            let declaration = self.declaration(at)?;
            chain.push(declaration);
            // Parents precede children (checked), so this ends; the bound is a second guard.
            if chain.len() > self.parts.declarations.len() {
                return None;
            }
            next = declaration.parent;
        }
        chain.reverse();
        Some(chain)
    }

    /// The symbol identity of declaration `id` (plan §2.1).
    ///
    /// Pinned by: `summary::tests::should_name_every_declaration_kind_when_it_is_addressable`.
    pub fn symbol_id(&self, id: DeclId) -> Result<SymbolId, SymbolError> {
        let chain = self.chain(id).ok_or(SymbolError::NotAddressable)?;
        let (last, outer) = chain.split_last().ok_or(SymbolError::NotAddressable)?;
        let (types, member) = if last.kind.is_type() {
            (&chain[..], None)
        } else {
            (outer, Some(*last))
        };
        if types.is_empty() || !types.iter().all(|d| d.kind.is_type()) {
            return Err(SymbolError::NotAddressable);
        }
        let type_names: Vec<&str> = types.iter().map(|d| self.str(d.name)).collect();
        let package: Vec<&str> = self
            .package()
            .map(|p| p.split('.').collect())
            .unwrap_or_default();
        let member = member
            .map(|m| {
                let params: Vec<&str> = self.list(m.params).collect();
                match m.kind {
                    DeclKind::Constructor => MemberId::constructor(&params),
                    DeclKind::Method | DeclKind::AnnotationElement => {
                        MemberId::method(self.str(m.name), &params)
                    }
                    _ => MemberId::field(self.str(m.name)),
                }
            })
            .transpose()?;
        SymbolId::new(&package, &type_names, member)
    }

    fn encode_header<H: KeyHasher>(&self, encoder: &mut Encoder<H>, declaration: &Declaration) {
        encoder.u32(u32::from(declaration.kind.code()));
        encoder.u32(u32::from(declaration.modifiers.bits()));
        encoder.str(self.str(declaration.name));
        encoder.str(self.str(declaration.signature));
        for list in [
            declaration.params,
            declaration.supertypes,
            declaration.permits,
            declaration.annotations,
        ] {
            encoder.len(usize::try_from(list.len).unwrap_or(usize::MAX));
            for name in self.list(list) {
                encoder.str(name);
            }
        }
    }

    /// The digest of declaration `id`'s API, through `hasher` (module documentation); `None` for
    /// an unknown id. Ranges, Javadoc and bodies are not read.
    pub fn api_digest<H: KeyHasher>(&self, id: DeclId, hasher: H) -> Option<Digest> {
        let chain = self.chain(id)?;
        let mut encoder = Encoder::new(hasher, "jfl.declaration-api.v2");
        encoder.str(self.package().unwrap_or(""));
        // Sorted: the order of imports changes no meaning, so it changes no key.
        let mut imports: Vec<(&str, u32)> = self
            .parts
            .imports
            .iter()
            .map(|i| {
                let flags = u32::from(i.is_static)
                    | (u32::from(i.on_demand) << 1)
                    | (u32::from(i.module) << 2);
                (self.str(i.name), flags)
            })
            .collect();
        imports.sort_unstable();
        encoder.len(imports.len());
        for (name, flags) in imports {
            encoder.str(name);
            encoder.u32(flags);
        }
        // Every enclosing type with its header text: a member's meaning depends on its owners'
        // type parameters and their bounds (`class Box<T extends Number>`).
        encoder.len(chain.len());
        for declaration in &chain {
            encoder.u32(u32::from(declaration.kind.code()));
            encoder.str(self.str(declaration.name));
            encoder.str(self.str(declaration.signature));
        }
        let declaration = chain.last()?;
        self.encode_header(&mut encoder, declaration);
        if declaration.kind.is_type() {
            let members = || {
                self.parts
                    .declarations
                    .iter()
                    .filter(|d| d.parent == Some(id))
            };
            encoder.len(members().count());
            for member in members() {
                self.encode_header(&mut encoder, member);
            }
        }
        Some(encoder.finish())
    }

    /// The bytes this summary holds on the heap (its arena and tables), without the
    /// `size_of::<FileSummary>()` of the value itself: the memory figure of the size bench.
    #[must_use]
    pub const fn heap_bytes(&self) -> usize {
        let p = &self.parts;
        p.text
            .len()
            .saturating_add(p.strings.len().saturating_mul(size_of::<Span>()))
            .saturating_add(p.imports.len().saturating_mul(size_of::<Import>()))
            .saturating_add(
                p.declarations
                    .len()
                    .saturating_mul(size_of::<Declaration>()),
            )
            .saturating_add(p.type_names.len().saturating_mul(size_of::<StrId>()))
            .saturating_add(
                p.references
                    .len()
                    .saturating_mul(size_of::<ReferenceSite>()),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::testing::RecordingHasher;
    use crate::keys::{EnvironmentKey, SemanticKey, SyntaxKey};
    use crate::text::ByteOffset;
    use alloc::string::ToString;

    fn range(start: u32, end: u32) -> ByteRange {
        ByteRange::new(ByteOffset::new(start), ByteOffset::new(end)).unwrap()
    }

    fn input<'a>(kind: DeclKind, parent: Option<DeclId>, name: &'a str) -> DeclarationInput<'a> {
        DeclarationInput {
            kind,
            modifiers: Modifiers::PUBLIC,
            parent,
            name,
            name_range: range(0, 1),
            range: range(0, 2),
            signature: name,
            params: &[],
            supertypes: &[],
            permits: &[],
            annotations: &[],
            javadoc: None,
        }
    }

    /// `package com.acme; import java.util.List; class Billing { long charge(Money m) {..} }`,
    /// optionally with a second `charge` overload.
    fn billing(signature: &str, overloads: bool, import: &str) -> FileSummary {
        let mut builder = SummaryBuilder::new();
        builder.package("com.acme").unwrap();
        builder.import(import, false, false, range(0, 5)).unwrap();
        let class = builder
            .declaration(&DeclarationInput {
                supertypes: &["Base"],
                range: range(10, 90),
                ..input(DeclKind::Class, None, "Billing")
            })
            .unwrap();
        builder
            .declaration(&DeclarationInput {
                params: &["Money"],
                signature,
                range: range(20, 80),
                javadoc: Some(range(12, 19)),
                ..input(DeclKind::Method, Some(class), "charge")
            })
            .unwrap();
        if overloads {
            builder
                .declaration(&DeclarationInput {
                    params: &["Money", "int"],
                    ..input(DeclKind::Method, Some(class), "charge")
                })
                .unwrap();
        }
        builder
            .reference("total", range(30, 35), RefShape::Call, Arity::of(1))
            .unwrap();
        builder.finish()
    }

    fn digests(summary: &FileSummary) -> (Digest, Digest) {
        (
            summary
                .api_digest(DeclId(0), RecordingHasher::default())
                .unwrap(),
            summary
                .api_digest(DeclId(1), RecordingHasher::default())
                .unwrap(),
        )
    }

    #[test]
    fn should_keep_records_compact_when_the_layout_changes() {
        // given
        // when
        let sizes = [
            size_of::<Declaration>(),
            size_of::<ReferenceSite>(),
            size_of::<Import>(),
            size_of::<Span>(),
            size_of::<StrId>(),
        ];
        // then
        assert_eq!(sizes, [80, 16, 16, 8, 4]);
    }

    #[test]
    fn should_store_a_string_once_when_it_repeats() {
        // given
        let mut builder = SummaryBuilder::new();
        // when
        for at in 0..100 {
            builder
                .reference("total", range(at, at + 5), RefShape::Name, Arity::default())
                .unwrap();
        }
        let summary = builder.finish();
        // then
        assert_eq!(&*summary.parts().text, "total");
        assert_eq!(summary.references().len(), 100);
        assert!(
            summary
                .references()
                .iter()
                .all(|r| summary.str(r.name) == "total")
        );
    }

    #[test]
    fn should_keep_the_api_digest_when_only_a_body_or_position_changes() {
        // given
        let before = billing("long charge(Money m)", false, "java.util.List");
        // A body edit changes the reference sites and the declaration's range; a line added
        // above shifts every range. Neither is in the API.
        let mut after = SummaryBuilder::new();
        after.package("com.acme").unwrap();
        after
            .import("java.util.List", false, false, range(40, 45))
            .unwrap();
        let class = after
            .declaration(&DeclarationInput {
                supertypes: &["Base"],
                range: range(50, 400),
                ..input(DeclKind::Class, None, "Billing")
            })
            .unwrap();
        after
            .declaration(&DeclarationInput {
                params: &["Money"],
                signature: "long charge(Money m)",
                range: range(60, 390),
                javadoc: Some(range(51, 59)),
                ..input(DeclKind::Method, Some(class), "charge")
            })
            .unwrap();
        after
            .reference(
                "audit",
                range(70, 75),
                RefShape::QualifiedCall,
                Arity::of(3),
            )
            .unwrap();
        let after = after.finish();
        // when
        let (before, after) = (digests(&before), digests(&after));
        // then
        assert_eq!(before, after);
    }

    /// `class Person<T> { String name; }` with the class's header and the field's annotations
    /// as given.
    fn person(class_signature: &str, annotations: &[&str], field_signature: &str) -> FileSummary {
        let mut builder = SummaryBuilder::new();
        builder.package("com.acme").unwrap();
        let class = builder
            .declaration(&DeclarationInput {
                signature: class_signature,
                ..input(DeclKind::Class, None, "Person")
            })
            .unwrap();
        builder
            .declaration(&DeclarationInput {
                annotations,
                signature: field_signature,
                ..input(DeclKind::Field, Some(class), "name")
            })
            .unwrap();
        builder.finish()
    }

    #[test]
    fn should_change_a_member_digest_when_an_annotation_or_owner_bound_changes() {
        // given
        let plain = person("class Person<T>", &[], "T name");
        let getter = person("class Person<T>", &["Getter"], "@Getter T name");
        let bounded = person("class Person<T extends Number>", &[], "T name");
        let key = |summary: &FileSummary| {
            let read = summary
                .api_digest(DeclId(1), RecordingHasher::default())
                .unwrap();
            SemanticKey::derive(
                &SyntaxKey::from_digest(Digest::from_bytes([1; 32])),
                &EnvironmentKey::from_digest(Digest::from_bytes([2; 32])),
                &[read],
                RecordingHasher::default(),
            )
        };
        // when
        let (plain_d, getter_d, bounded_d) = (digests(&plain), digests(&getter), digests(&bounded));
        // then
        assert_ne!(
            key(&plain),
            key(&getter),
            "adding @Getter changes a dependent's key"
        );
        assert_ne!(
            plain_d.0, getter_d.0,
            "the type's API holds its members' annotations"
        );
        assert_ne!(
            plain_d.1, bounded_d.1,
            "a member's API holds its owner's bounds"
        );
        assert_eq!(
            getter
                .list(getter.declaration(DeclId(1)).unwrap().annotations)
                .collect::<Vec<_>>(),
            ["Getter"]
        );
    }

    #[test]
    fn should_keep_permits_apart_from_supertypes_when_a_type_is_sealed() {
        // given
        let shape = |permits: &'static [&'static str]| {
            let mut builder = SummaryBuilder::new();
            builder
                .declaration(&DeclarationInput {
                    supertypes: &["Figure"],
                    permits,
                    modifiers: Modifiers::SEALED,
                    ..input(DeclKind::Interface, None, "Shape")
                })
                .unwrap();
            builder.finish()
        };
        // when
        let one = shape(&["Circle"]);
        let two = shape(&["Circle", "Square"]);
        // then
        let declaration = one.declaration(DeclId(0)).unwrap();
        assert_eq!(
            one.list(declaration.supertypes).collect::<Vec<_>>(),
            ["Figure"]
        );
        assert_eq!(
            one.list(declaration.permits).collect::<Vec<_>>(),
            ["Circle"]
        );
        assert_ne!(
            one.api_digest(DeclId(0), RecordingHasher::default()),
            two.api_digest(DeclId(0), RecordingHasher::default())
        );
    }

    #[test]
    fn should_keep_the_api_digest_when_imports_are_reordered() {
        // given
        let with_imports = |imports: &[&str]| {
            let mut builder = SummaryBuilder::new();
            for name in imports {
                builder.import(name, false, false, range(0, 1)).unwrap();
            }
            builder
                .declaration(&input(DeclKind::Class, None, "A"))
                .unwrap();
            builder.finish()
        };
        // when
        let one = with_imports(&["a.X", "b.Y"]).api_digest(DeclId(0), RecordingHasher::default());
        let two = with_imports(&["b.Y", "a.X"]).api_digest(DeclId(0), RecordingHasher::default());
        // then
        assert_eq!(one, two);
    }

    #[test]
    fn should_change_the_api_digest_when_a_module_import_replaces_a_package_import() {
        // given: `import module java.base;` and `import java.base.*;` name the same text
        let with = |module: bool| {
            let mut builder = SummaryBuilder::new();
            if module {
                builder.module_import("java.base", range(0, 1)).unwrap();
            } else {
                builder
                    .import("java.base", false, true, range(0, 1))
                    .unwrap();
            }
            builder
                .declaration(&input(DeclKind::Class, None, "A"))
                .unwrap();
            builder.finish()
        };
        // when
        let module = with(true);
        let package = with(false);
        // then
        assert!(
            module
                .imports()
                .first()
                .is_some_and(|i| i.module && !i.on_demand)
        );
        assert_ne!(
            module.api_digest(DeclId(0), RecordingHasher::default()),
            package.api_digest(DeclId(0), RecordingHasher::default())
        );
    }

    #[test]
    fn should_change_the_api_digest_when_the_signature_or_member_set_changes() {
        // given
        let base = digests(&billing("long charge(Money m)", false, "java.util.List"));
        // when
        let signature = digests(&billing("int charge(Money m)", false, "java.util.List"));
        let overload = digests(&billing("long charge(Money m)", true, "java.util.List"));
        let import = digests(&billing("long charge(Money m)", false, "java.util.Map"));
        // then
        assert_ne!(
            base.0, signature.0,
            "a member's signature is in its type's API"
        );
        assert_ne!(base.1, signature.1);
        assert_ne!(base.0, overload.0, "a new overload is in its type's API");
        assert_eq!(
            base.1, overload.1,
            "a sibling overload is not in the method's API"
        );
        assert_ne!(
            base.0, import.0,
            "imports decide what the names in a signature mean"
        );
        assert_ne!(base.1, import.1);
        assert_ne!(base.0, base.1);
    }

    #[test]
    fn should_name_every_declaration_kind_when_it_is_addressable() {
        // given
        let mut builder = SummaryBuilder::new();
        builder.package("com.acme").unwrap();
        let outer = builder
            .declaration(&input(DeclKind::Class, None, "Billing"))
            .unwrap();
        let mut ids = Vec::new();
        for kind in DeclKind::ALL {
            let name = if kind == DeclKind::Constructor {
                "Billing"
            } else {
                "x"
            };
            let params: &[&str] = if kind.is_callable() {
                &["Money", "int[]"]
            } else {
                &[]
            };
            let id = builder
                .declaration(&DeclarationInput {
                    params,
                    ..input(kind, Some(outer), name)
                })
                .unwrap();
            ids.push(id);
        }
        let method = ids[5];
        let local = builder
            .declaration(&input(DeclKind::Class, Some(method), "Local"))
            .unwrap();
        let summary = builder.finish();
        // when
        let printed: Vec<_> = ids
            .iter()
            .map(|&id| summary.symbol_id(id).map(|s| s.to_string()))
            .collect();
        // then
        let expected = [
            "com.acme.Billing$x",
            "com.acme.Billing$x",
            "com.acme.Billing$x",
            "com.acme.Billing$x",
            "com.acme.Billing$x",
            "com.acme.Billing#x(Money,int[])",
            "com.acme.Billing#<init>(Money,int[])",
            "com.acme.Billing#x",
            "com.acme.Billing#x",
            "com.acme.Billing#x",
            "com.acme.Billing#x(Money,int[])",
        ];
        assert_eq!(printed, expected.map(|s| Ok(s.to_string())));
        assert_eq!(summary.symbol_id(local), Err(SymbolError::NotAddressable));
        assert_eq!(
            summary.symbol_id(DeclId(99)),
            Err(SymbolError::NotAddressable)
        );
    }

    #[test]
    fn should_refuse_parts_when_an_index_is_out_of_range() {
        // given
        let good = billing("long charge(Money m)", true, "java.util.List").into_parts();
        let mut cases = Vec::new();
        let mut parts = good.clone();
        parts.strings[0].len = 999;
        cases.push((parts, SummaryError::BadSpan { index: 0 }));
        let mut parts = good.clone();
        parts.text = "é".into();
        parts.strings = [Span { start: 1, len: 1 }].into();
        cases.push((parts, SummaryError::BadSpan { index: 0 }));
        let mut parts = good.clone();
        parts.package = Some(StrId(999));
        cases.push((parts, SummaryError::UnknownString { id: 999 }));
        let mut parts = good.clone();
        parts.references[0].name = StrId(u32::MAX);
        cases.push((parts, SummaryError::UnknownString { id: u32::MAX }));
        let mut parts = good.clone();
        parts.declarations[1].params = ListRef {
            start: u32::MAX,
            len: 2,
        };
        cases.push((
            parts,
            SummaryError::BadList {
                start: u32::MAX,
                len: 2,
            },
        ));
        let mut parts = good.clone();
        parts.declarations[0].parent = Some(DeclId(0));
        cases.push((parts, SummaryError::BadParent { index: 0 }));
        let mut parts = good.clone();
        parts.declarations[1].parent = Some(DeclId(2));
        cases.push((parts, SummaryError::BadParent { index: 1 }));
        // when
        let results: Vec<_> = cases
            .iter()
            .map(|(parts, _)| FileSummary::from_parts(parts.clone()).err())
            .collect();
        // then
        let expected: Vec<_> = cases.iter().map(|(_, error)| Some(*error)).collect();
        assert_eq!(results, expected);
        assert!(FileSummary::from_parts(good).is_ok());
    }

    #[test]
    fn should_refuse_a_declaration_when_its_parent_is_not_added_yet() {
        // given
        let mut builder = SummaryBuilder::new();
        // when
        let refused = builder.declaration(&input(DeclKind::Method, Some(DeclId(0)), "f"));
        // then
        assert_eq!(refused, Err(SummaryError::BadParent { index: 0 }));
    }

    #[test]
    fn should_saturate_arity_when_a_call_passes_more_than_255() {
        // given
        // when
        let arities = [
            Arity::of(0),
            Arity::of(254),
            Arity::of(255),
            Arity::of(70_000),
        ];
        // then
        assert_eq!(arities.map(|a| a.0), [0, 254, 255, 255]);
    }

    #[test]
    fn should_read_back_codes_and_bits_when_they_are_stored() {
        // given
        let all = Modifiers::ALL_FOR_TEST;
        // when
        let shapes: Vec<_> = RefShape::ALL
            .iter()
            .map(|s| RefShape::from_code(s.code()))
            .collect();
        // then
        assert_eq!(shapes, RefShape::ALL.map(Some));
        assert_eq!(RefShape::from_code(8), None);
        assert_eq!(Modifiers::from_bits(all.bits()), Some(all));
        assert_eq!(Modifiers::from_bits(1 << 15), None);
        assert!(all.contains(Modifiers::VARARGS.with(Modifiers::PUBLIC)));
        assert!(!Modifiers::empty().contains(Modifiers::STATIC));
    }

    impl Modifiers {
        const ALL_FOR_TEST: Self = Self(Self::KNOWN);
    }
}
