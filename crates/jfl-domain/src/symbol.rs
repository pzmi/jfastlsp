//! Declaration kinds and symbol identities in their textual form (plan §2.1:
//! `com.acme.Billing#charge(Money)`).
//!
//! The text form, printed by [`SymbolId`]'s `Display` and read back by [`SymbolId::parse`]:
//!
//! ```text
//! symbol  := types [ '#' member ]
//! types   := { segment '.' } segment { '$' segment }     package segments, then the top-level
//!                                                        type, then nested types
//! member  := segment                                      a field, enum constant or record component
//!          | segment '(' [ type { ',' type } ] ')'        a method or annotation element
//!          | '<init>(' [ type { ',' type } ] ')'          a constructor
//! segment := name, wrapped in backticks when it holds a '$'
//! type    := name { '.' name } { '[]' }                   a parameter type as written, erased
//! ```
//!
//! Examples: `com.acme.Billing`, `com.acme.Billing$Line#amount`,
//! `com.acme.Billing#charge(Money,java.util.List[])`, `Billing#<init>()` (default package),
//! ``com.acme.`Gen$1`#run()`` (a name holding `$`).
//!
//! The text is the *only* spelling of an identity: parsing refuses anything [`SymbolId`] would
//! not print (a quoted name without `$`, spaces, generic arguments), so two equal texts are two
//! equal identities and the text can key a map.
//!
//! The last `.`-separated segment before any `$` or `#` is the top-level type and the ones before
//! it are the package; nested types follow `$`, as in a JVM binary name. Because `$` is also a
//! legal identifier character (JLS §3.8), a name holding one is quoted in backticks, which no
//! identifier holds, so the split is never ambiguous. Parameter types are as the source writes
//! them, with type arguments erased and varargs written `[]`: the syntax index knows no more
//! before resolution, and an overload is told apart by them (plan §2.2's bounded ambiguity).
//!
//! Local and anonymous classes have no identity here: nothing outside their method can name
//! them.
//!
//! Rejected alternatives: SCIP's descriptor syntax (`com/acme/Billing#charge().`) is not the
//! plan's form and names overloads by a disambiguator, not by parameters; JVM descriptors need
//! resolved types, which the syntax index does not have.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::{self, Write as _};

/// The longest name accepted, in bytes; a class file's `CONSTANT_Utf8` holds at most 65 535
/// bytes, but no real identifier comes near this bound on untrusted input.
pub const MAX_NAME_LEN: usize = 1024;

/// The most parameters a method may declare (JVMS §4.3.3: 255 slots).
pub const MAX_PARAMS: usize = 255;

/// The longest symbol text [`SymbolId::parse`] reads, checked before anything is allocated.
pub const MAX_SYMBOL_LEN: usize = 16 * 1024;

/// The most package and type segments one symbol holds.
pub const MAX_SEGMENTS: usize = 128;

/// What a declaration declares. The `u8` code is stable: it is part of the summary format and
/// of the declaration API digest (`summary`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeclKind {
    /// `class`.
    Class,
    /// `interface`.
    Interface,
    /// `enum`.
    Enum,
    /// `record`.
    Record,
    /// `@interface`.
    Annotation,
    /// A method.
    Method,
    /// A constructor (a record's compact constructor included).
    Constructor,
    /// A field (one per declarator).
    Field,
    /// An enum constant.
    EnumConstant,
    /// A record component.
    RecordComponent,
    /// An annotation type's element.
    AnnotationElement,
}

impl DeclKind {
    /// Every kind, in code order.
    pub const ALL: [Self; 11] = [
        Self::Class,
        Self::Interface,
        Self::Enum,
        Self::Record,
        Self::Annotation,
        Self::Method,
        Self::Constructor,
        Self::Field,
        Self::EnumConstant,
        Self::RecordComponent,
        Self::AnnotationElement,
    ];

    /// The stable code of this kind.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Class => 0,
            Self::Interface => 1,
            Self::Enum => 2,
            Self::Record => 3,
            Self::Annotation => 4,
            Self::Method => 5,
            Self::Constructor => 6,
            Self::Field => 7,
            Self::EnumConstant => 8,
            Self::RecordComponent => 9,
            Self::AnnotationElement => 10,
        }
    }

    /// The kind with this code, if any (a stored summary's byte).
    ///
    /// Pinned by: `symbol::tests::should_read_back_every_kind_when_its_code_is_given`.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Class),
            1 => Some(Self::Interface),
            2 => Some(Self::Enum),
            3 => Some(Self::Record),
            4 => Some(Self::Annotation),
            5 => Some(Self::Method),
            6 => Some(Self::Constructor),
            7 => Some(Self::Field),
            8 => Some(Self::EnumConstant),
            9 => Some(Self::RecordComponent),
            10 => Some(Self::AnnotationElement),
            _ => None,
        }
    }

    /// Whether the kind declares a type (and so may have members).
    #[must_use]
    pub const fn is_type(self) -> bool {
        matches!(
            self,
            Self::Class | Self::Interface | Self::Enum | Self::Record | Self::Annotation
        )
    }

    /// Whether the kind is called with arguments (its identity carries parameter types).
    #[must_use]
    pub const fn is_callable(self) -> bool {
        matches!(
            self,
            Self::Method | Self::Constructor | Self::AnnotationElement
        )
    }
}

/// Why a symbol or one of its parts is refused. The values carry no part of the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SymbolError {
    /// An empty name.
    #[error("empty name")]
    EmptyName,
    /// A name longer than [`MAX_NAME_LEN`].
    #[error("name of {len} bytes is longer than {MAX_NAME_LEN}")]
    NameTooLong {
        /// The name's length in bytes.
        len: usize,
    },
    /// A name holding a separator, a bracket, white space or a control character.
    #[error("name holds a reserved character")]
    ReservedCharacter,
    /// A parameter type that is not `name{.name}{[]}`.
    #[error("malformed parameter type")]
    MalformedType,
    /// More than [`MAX_PARAMS`] parameters.
    #[error("{count} parameters is more than {MAX_PARAMS}")]
    TooManyParams {
        /// How many were given.
        count: usize,
    },
    /// No type, or more than [`MAX_SEGMENTS`] segments.
    #[error("a symbol names one to {MAX_SEGMENTS} package and type segments")]
    SegmentCount,
    /// Text longer than [`MAX_SYMBOL_LEN`].
    #[error("symbol text of {len} bytes is longer than {MAX_SYMBOL_LEN}")]
    TooLong {
        /// The text's length in bytes.
        len: usize,
    },
    /// Text that is not the canonical form [`SymbolId`] prints, at this byte.
    #[error("malformed symbol text at byte {at}")]
    Malformed {
        /// The byte index where parsing stopped.
        at: usize,
    },
    /// A declaration nobody outside its method can name (a local or anonymous class, or a
    /// member of one), or a member inside a member.
    #[error("declaration has no symbol identity")]
    NotAddressable,
}

/// Checks a simple name: non-empty, bounded, and free of every character the text form uses
/// (`$` is allowed: it is quoted when printed).
fn check_name(name: &str) -> Result<(), SymbolError> {
    if name.is_empty() {
        return Err(SymbolError::EmptyName);
    }
    if name.len() > MAX_NAME_LEN {
        return Err(SymbolError::NameTooLong { len: name.len() });
    }
    if name.chars().any(reserved) {
        return Err(SymbolError::ReservedCharacter);
    }
    Ok(())
}

/// A character no name may hold: the text form's punctuation, white space and controls.
fn reserved(c: char) -> bool {
    matches!(
        c,
        '.' | '#' | '(' | ')' | ',' | '`' | '<' | '>' | '[' | ']' | '/' | ';' | '@' | ':'
    ) || c.is_whitespace()
        || c.is_control()
}

/// Checks a parameter type: dotted names, then `[]` pairs.
fn check_type(text: &str) -> Result<(), SymbolError> {
    if text.len() > MAX_NAME_LEN {
        return Err(SymbolError::NameTooLong { len: text.len() });
    }
    let mut base = text;
    while let Some(stripped) = base.strip_suffix("[]") {
        base = stripped;
    }
    if base.is_empty() {
        return Err(SymbolError::MalformedType);
    }
    for part in base.split('.') {
        check_name(part).map_err(|_| SymbolError::MalformedType)?;
    }
    Ok(())
}

/// A field, method or constructor of a type.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MemberId {
    /// A field, enum constant or record component: `#name`.
    Field {
        /// The simple name.
        name: Box<str>,
    },
    /// A method or annotation element: `#name(T1,T2)`.
    Method {
        /// The simple name.
        name: Box<str>,
        /// The parameter types as written, erased.
        params: Box<[Box<str>]>,
    },
    /// A constructor: `#<init>(T1,T2)`.
    Constructor {
        /// The parameter types as written, erased.
        params: Box<[Box<str>]>,
    },
}

impl MemberId {
    /// A field, enum constant or record component named `name`.
    pub fn field(name: &str) -> Result<Self, SymbolError> {
        check_name(name)?;
        Ok(Self::Field { name: name.into() })
    }

    /// A method or annotation element `name(params)`.
    pub fn method(name: &str, params: &[&str]) -> Result<Self, SymbolError> {
        check_name(name)?;
        Ok(Self::Method {
            name: name.into(),
            params: checked_params(params)?,
        })
    }

    /// A constructor `<init>(params)`.
    pub fn constructor(params: &[&str]) -> Result<Self, SymbolError> {
        Ok(Self::Constructor {
            params: checked_params(params)?,
        })
    }

    /// The member's simple name (`<init>` for a constructor).
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Field { name } | Self::Method { name, .. } => name,
            Self::Constructor { .. } => "<init>",
        }
    }

    /// The parameter types, or `None` for a field.
    #[must_use]
    pub fn params(&self) -> Option<&[Box<str>]> {
        match self {
            Self::Field { .. } => None,
            Self::Method { params, .. } | Self::Constructor { params } => Some(params),
        }
    }
}

fn checked_params(params: &[&str]) -> Result<Box<[Box<str>]>, SymbolError> {
    if params.len() > MAX_PARAMS {
        return Err(SymbolError::TooManyParams {
            count: params.len(),
        });
    }
    params
        .iter()
        .map(|param| check_type(param).map(|()| Box::<str>::from(*param)))
        .collect()
}

/// The identity of a declaration a query can target: package, enclosing types, and a member.
///
/// Pinned by: `tests/properties.rs::should_round_trip_a_symbol_id_when_it_names_any_declaration_kind`
/// and `symbol::tests::should_refuse_symbol_text_when_it_is_not_canonical`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SymbolId {
    package: Box<[Box<str>]>,
    types: Box<[Box<str>]>,
    member: Option<MemberId>,
}

impl SymbolId {
    /// The symbol of `types` (outermost first) in `package` (empty for the default package),
    /// optionally one of the innermost type's members.
    pub fn new(
        package: &[&str],
        types: &[&str],
        member: Option<MemberId>,
    ) -> Result<Self, SymbolError> {
        let segments = package.len().saturating_add(types.len());
        if types.is_empty() || segments > MAX_SEGMENTS {
            return Err(SymbolError::SegmentCount);
        }
        for name in package.iter().chain(types) {
            check_name(name)?;
        }
        let symbol = Self {
            package: package.iter().map(|&s| Box::from(s)).collect(),
            types: types.iter().map(|&s| Box::from(s)).collect(),
            member,
        };
        // What `new` accepts, `parse` must read back: the printed form is bounded too.
        let mut printed = Counter(0);
        write!(printed, "{symbol}").map_err(|_| SymbolError::TooLong { len: printed.0 })?;
        if printed.0 > MAX_SYMBOL_LEN {
            return Err(SymbolError::TooLong { len: printed.0 });
        }
        Ok(symbol)
    }

    /// Parses the text form (module documentation); only what `Display` prints is accepted.
    pub fn parse(text: &str) -> Result<Self, SymbolError> {
        if text.len() > MAX_SYMBOL_LEN {
            return Err(SymbolError::TooLong { len: text.len() });
        }
        Parser { text, at: 0 }.symbol()
    }

    /// The package segments, outermost first; empty for the default package.
    pub fn package(&self) -> impl Iterator<Item = &str> {
        self.package.iter().map(|s| &**s)
    }

    /// The type segments, the top-level type first.
    pub fn types(&self) -> impl Iterator<Item = &str> {
        self.types.iter().map(|s| &**s)
    }

    /// The member, if the symbol names one.
    #[must_use]
    pub const fn member(&self) -> Option<&MemberId> {
        self.member.as_ref()
    }
}

/// Counts the bytes written through it.
struct Counter(usize);

impl fmt::Write for Counter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0 = self.0.saturating_add(s.len());
        Ok(())
    }
}

/// A type parameter in scope where a parameter type is written: its name and its first bound
/// as written (`T extends Comparable<T> & Serializable` has first bound `Comparable<T>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeParam<'a> {
    /// The type variable's name.
    pub name: &'a str,
    /// The first bound, if any.
    pub first_bound: Option<&'a str>,
}

/// Erases a parameter type as written to the form a [`SymbolId`] holds: type arguments and
/// annotations dropped, white space removed, varargs `...` written `[]`, and a type variable
/// replaced by the erasure of its first bound (`Object` without one), as javac erases it.
/// `scope` lists the type parameters visible at the declaration, innermost first (the method's,
/// then each enclosing type's), so the first match is the one that shadows.
///
/// Without the bound, `<T extends Number> void f(T)` and `<T extends CharSequence> void f(T)`,
/// legal overloads, would share one identity. Pinned by:
/// `symbol::tests::should_tell_overloads_apart_when_they_differ_by_a_type_variable_bound`.
pub fn erase_parameter_type(written: &str, scope: &[TypeParam<'_>]) -> Result<String, SymbolError> {
    if written.len() > MAX_SYMBOL_LEN {
        return Err(SymbolError::TooLong { len: written.len() });
    }
    let (mut base, dims) = strip_type(written)?;
    // A bound may itself be a type variable (`<T, U extends T>`); a cycle, which javac refuses,
    // ends after one hop per parameter.
    for _ in 0..=scope.len() {
        if base.contains('.') {
            break;
        }
        let Some(param) = scope.iter().find(|p| p.name == base) else {
            break;
        };
        base = match param.first_bound {
            Some(bound) => strip_type(bound)?.0,
            None => String::from("Object"),
        };
    }
    if scope.iter().any(|p| p.name == base) {
        base = String::from("Object");
    }
    for _ in 0..dims {
        base.push_str("[]");
    }
    check_type(&base)?;
    Ok(base)
}

/// `text` without type arguments, annotations and white space, and its array dimensions.
fn strip_type(text: &str) -> Result<(String, usize), SymbolError> {
    let mut base = String::new();
    let mut dims = 0_usize;
    let mut angle = 0_usize;
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        match c {
            '<' => angle = angle.saturating_add(1),
            '>' => angle = angle.saturating_sub(1),
            _ if angle > 0 => {}
            '@' => {
                while chars.next_if(|&(_, c)| !reserved(c) || c == '.').is_some() {}
                while chars.next_if(|&(_, c)| c.is_whitespace()).is_some() {}
                if chars.next_if(|&(_, c)| c == '(').is_some() {
                    let mut depth = 1_usize;
                    while depth > 0 {
                        match chars.next() {
                            Some((_, '(')) => depth = depth.saturating_add(1),
                            Some((_, ')')) => depth = depth.saturating_sub(1),
                            Some(_) => {}
                            None => return Err(SymbolError::MalformedType),
                        }
                    }
                }
            }
            '.' if text.get(at..).is_some_and(|rest| rest.starts_with("...")) => {
                dims = dims.saturating_add(1);
                chars.next();
                chars.next();
            }
            ']' => dims = dims.saturating_add(1),
            '[' => {}
            _ if c.is_whitespace() => {}
            _ if dims > 0 => return Err(SymbolError::MalformedType),
            _ => base.push(c),
        }
    }
    if base.is_empty() || angle > 0 {
        return Err(SymbolError::MalformedType);
    }
    Ok((base, dims))
}

/// Writes `name`, in backticks when it holds a `$`.
fn write_segment(f: &mut fmt::Formatter<'_>, name: &str) -> fmt::Result {
    if name.contains('$') {
        write!(f, "`{name}`")
    } else {
        f.write_str(name)
    }
}

impl fmt::Display for SymbolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for segment in &self.package {
            write_segment(f, segment)?;
            f.write_str(".")?;
        }
        for (i, segment) in self.types.iter().enumerate() {
            if i > 0 {
                f.write_str("$")?;
            }
            write_segment(f, segment)?;
        }
        let Some(member) = &self.member else {
            return Ok(());
        };
        f.write_str("#")?;
        match member {
            MemberId::Field { name } => write_segment(f, name),
            MemberId::Method { name, params } => {
                write_segment(f, name)?;
                write_params(f, params)
            }
            MemberId::Constructor { params } => {
                f.write_str("<init>")?;
                write_params(f, params)
            }
        }
    }
}

fn write_params(f: &mut fmt::Formatter<'_>, params: &[Box<str>]) -> fmt::Result {
    f.write_str("(")?;
    for (i, param) in params.iter().enumerate() {
        if i > 0 {
            f.write_str(",")?;
        }
        f.write_str(param)?;
    }
    f.write_str(")")
}

/// A cursor over symbol text; every step either advances or fails with its byte index.
struct Parser<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        self.text.get(self.at..).unwrap_or("")
    }

    fn malformed(&self) -> SymbolError {
        SymbolError::Malformed { at: self.at }
    }

    fn eat(&mut self, token: &str) -> bool {
        if self.rest().starts_with(token) {
            self.at = self.at.saturating_add(token.len());
            true
        } else {
            false
        }
    }

    /// One segment: a plain name without `$`, or a backticked name with one.
    fn segment(&mut self) -> Result<&'a str, SymbolError> {
        let start = self.at;
        let rest = self.rest();
        let (name, used) = if let Some(quoted) = rest.strip_prefix('`') {
            let end = quoted.find('`').ok_or_else(|| self.malformed())?;
            let name = quoted.get(..end).ok_or_else(|| self.malformed())?;
            if !name.contains('$') {
                return Err(self.malformed());
            }
            (name, end.saturating_add(2))
        } else {
            let end = rest
                .find(|c: char| reserved(c) || c == '$')
                .unwrap_or(rest.len());
            let name = rest.get(..end).ok_or_else(|| self.malformed())?;
            (name, end)
        };
        check_name(name).map_err(|_| SymbolError::Malformed { at: start })?;
        self.at = self.at.saturating_add(used);
        Ok(name)
    }

    fn symbol(mut self) -> Result<SymbolId, SymbolError> {
        let mut dotted = Vec::new();
        loop {
            dotted.push(self.segment()?);
            if !self.eat(".") {
                break;
            }
        }
        let mut types = Vec::new();
        if let Some(top) = dotted.pop() {
            types.push(top);
        }
        while self.eat("$") {
            types.push(self.segment()?);
        }
        let member = if self.eat("#") {
            Some(self.member()?)
        } else {
            None
        };
        if !self.rest().is_empty() {
            return Err(self.malformed());
        }
        SymbolId::new(&dotted, &types, member)
    }

    fn member(&mut self) -> Result<MemberId, SymbolError> {
        if self.eat("<init>") {
            let params = self.params()?.ok_or_else(|| self.malformed())?;
            return MemberId::constructor(&params);
        }
        let name = self.segment()?;
        match self.params()? {
            Some(params) => MemberId::method(name, &params),
            None => MemberId::field(name),
        }
    }

    /// `(T1,T2)` if the cursor is at `(`.
    fn params(&mut self) -> Result<Option<Vec<&'a str>>, SymbolError> {
        if !self.eat("(") {
            return Ok(None);
        }
        let rest = self.rest();
        let close = rest.find(')').ok_or_else(|| self.malformed())?;
        let inside = rest.get(..close).ok_or_else(|| self.malformed())?;
        let mut params = Vec::new();
        if !inside.is_empty() {
            for param in inside.split(',') {
                check_type(param).map_err(|_| self.malformed())?;
                params.push(param);
            }
        }
        self.at = self.at.saturating_add(close).saturating_add(1);
        Ok(Some(params))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn should_print_the_plan_form_when_a_method_is_named() {
        // given
        let member = MemberId::method("charge", &["Money"]).unwrap();
        // when
        let id = SymbolId::new(&["com", "acme"], &["Billing"], Some(member)).unwrap();
        // then
        assert_eq!(id.to_string(), "com.acme.Billing#charge(Money)");
        assert_eq!(SymbolId::parse("com.acme.Billing#charge(Money)"), Ok(id));
    }

    #[test]
    fn should_parse_every_shape_when_the_text_is_canonical() {
        // given
        let cases = [
            "Billing",
            "com.acme.Billing",
            "com.acme.Billing$Line$Tax",
            "com.acme.Billing#amount",
            "com.acme.Billing#charge()",
            "com.acme.Billing#charge(Money,java.util.List[],int[][])",
            "Billing#<init>()",
            "com.acme.Billing$Line#<init>(long)",
            "com.`a$b`.`Gen$1`$Inner#`get$x`(Outer$Inner)",
            "café.Été#größe",
        ];
        // when
        let printed: Vec<_> = cases
            .iter()
            .map(|text| SymbolId::parse(text).map(|id| id.to_string()))
            .collect();
        // then
        let expected: Vec<_> = cases.iter().map(|text| Ok(text.to_string())).collect();
        assert_eq!(printed, expected);
    }

    #[test]
    fn should_split_package_from_types_when_names_hold_dollars() {
        // given
        let text = "com.`a$b`.Outer$`In$ner`#f";
        // when
        let id = SymbolId::parse(text).unwrap();
        // then
        assert_eq!(id.package().collect::<Vec<_>>(), ["com", "a$b"]);
        assert_eq!(id.types().collect::<Vec<_>>(), ["Outer", "In$ner"]);
        assert_eq!(id.member(), Some(&MemberId::field("f").unwrap()));
    }

    #[test]
    fn should_refuse_symbol_text_when_it_is_not_canonical() {
        // given
        let cases = [
            "",
            ".A",
            "a..A",
            "A.",
            "A$",
            "A#",
            "A#f(",
            "A#f(int",
            "A#f(int,)",
            "A#f( int)",
            "A#f(List<String>)",
            "A#f(int)x",
            "A#<init>",
            "A#b#c",
            "A#f(`x`)",
            "`A`",
            "`A$",
            "a$b.C",
            "A B",
            "A\u{0}",
        ];
        // when
        let refused: Vec<_> = cases
            .iter()
            .filter(|text| SymbolId::parse(text).is_ok())
            .collect();
        // then
        assert!(refused.is_empty(), "accepted: {refused:?}");
    }

    #[test]
    fn should_refuse_symbol_text_when_it_is_longer_than_the_bound() {
        // given
        let long = "A".repeat(MAX_SYMBOL_LEN.saturating_add(1));
        // when
        let refused = SymbolId::parse(&long);
        // then
        assert_eq!(refused, Err(SymbolError::TooLong { len: long.len() }));
    }

    #[test]
    fn should_refuse_parts_when_they_break_the_bounds() {
        // given
        let long = "a".repeat(MAX_NAME_LEN.saturating_add(1));
        let many = ["int"; MAX_PARAMS + 1];
        let deep = ["a"; MAX_SEGMENTS + 1];
        // when
        let results = [
            SymbolId::new(&[], &[], None).map(|_| ()),
            SymbolId::new(&[], &[&long], None).map(|_| ()),
            SymbolId::new(&deep[1..], &["A"], None).map(|_| ()),
            MemberId::method("f", &many).map(|_| ()),
            MemberId::field("a.b").map(|_| ()),
            MemberId::method("f", &["List<String>"]).map(|_| ()),
            MemberId::method("f", &["[]"]).map(|_| ()),
        ];
        // then
        assert_eq!(
            results,
            [
                Err(SymbolError::SegmentCount),
                Err(SymbolError::NameTooLong { len: long.len() }),
                Err(SymbolError::SegmentCount),
                Err(SymbolError::TooManyParams {
                    count: MAX_PARAMS + 1
                }),
                Err(SymbolError::ReservedCharacter),
                Err(SymbolError::MalformedType),
                Err(SymbolError::MalformedType),
            ]
        );
    }

    #[test]
    fn should_refuse_a_symbol_when_its_printed_form_is_longer_than_parse_reads() {
        // given
        let long = "a".repeat(MAX_NAME_LEN);
        let params: Vec<&str> = core::iter::repeat_n(long.as_str(), 20).collect();
        // when
        let refused = MemberId::method("f", &params)
            .and_then(|member| SymbolId::new(&["p"], &["A"], Some(member)));
        let fits = MemberId::method("f", &params[..10])
            .and_then(|member| SymbolId::new(&["p"], &["A"], Some(member)))
            .unwrap();
        // then
        assert!(matches!(refused, Err(SymbolError::TooLong { .. })));
        assert_eq!(SymbolId::parse(&fits.to_string()), Ok(fits));
    }

    #[test]
    fn should_tell_overloads_apart_when_they_differ_by_a_type_variable_bound() {
        // given
        let number = [TypeParam {
            name: "T",
            first_bound: Some("Number"),
        }];
        let chars = [TypeParam {
            name: "T",
            first_bound: Some("java.lang.CharSequence"),
        }];
        let symbol = |scope: &[TypeParam<'_>]| {
            let param = erase_parameter_type("T", scope).unwrap();
            let member = MemberId::method("f", &[param.as_str()]).unwrap();
            SymbolId::new(&["p"], &["C"], Some(member))
                .unwrap()
                .to_string()
        };
        // when
        let (one, two) = (symbol(&number), symbol(&chars));
        // then
        assert_eq!(one, "p.C#f(Number)");
        assert_eq!(two, "p.C#f(java.lang.CharSequence)");
    }

    #[test]
    fn should_erase_a_parameter_type_when_it_is_written_with_generics_and_varargs() {
        // given
        let scope = [
            TypeParam {
                name: "E",
                first_bound: Some("Comparable<E>"),
            },
            TypeParam {
                name: "U",
                first_bound: Some("T"),
            },
            TypeParam {
                name: "T",
                first_bound: None,
            },
            TypeParam {
                name: "X",
                first_bound: Some("Y"),
            },
            TypeParam {
                name: "Y",
                first_bound: Some("X"),
            },
        ];
        let cases = [
            ("List<String>", "List"),
            (
                "java.util.Map.Entry<K, ? extends V>[]",
                "java.util.Map.Entry[]",
            ),
            ("String...", "String[]"),
            ("int [] []", "int[][]"),
            ("@NonNull String", "String"),
            ("@Size(min = 1) List<@NonNull String>", "List"),
            ("E", "Comparable"),
            ("E[]", "Comparable[]"),
            ("U", "Object"),
            ("X", "Object"),
            ("T...", "Object[]"),
        ];
        // when
        let erased: Vec<_> = cases
            .iter()
            .map(|(written, _)| erase_parameter_type(written, &scope))
            .collect();
        // then
        let expected: Vec<_> = cases.iter().map(|(_, e)| Ok(String::from(*e))).collect();
        assert_eq!(erased, expected);
        assert_eq!(
            erase_parameter_type("List<", &[]),
            Err(SymbolError::MalformedType)
        );
        assert_eq!(
            erase_parameter_type("int[]x", &[]),
            Err(SymbolError::MalformedType)
        );
        assert_eq!(
            erase_parameter_type("", &[]),
            Err(SymbolError::MalformedType)
        );
    }

    #[test]
    fn should_read_back_every_kind_when_its_code_is_given() {
        // given
        let codes: Vec<_> = DeclKind::ALL.iter().map(|kind| kind.code()).collect();
        // when
        let kinds: Vec<_> = codes
            .iter()
            .map(|&code| DeclKind::from_code(code))
            .collect();
        // then
        assert_eq!(kinds, DeclKind::ALL.map(Some));
        assert_eq!(DeclKind::from_code(11), None);
    }
}
