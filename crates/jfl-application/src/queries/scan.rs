//! What the queries read from a file's text beside its summary: the receiver written before a
//! name, a type's qualifier, whether a site may sit inside a class the summary does not hold (an
//! anonymous or local class), a declaration's type parameters, and a Javadoc comment's words.
//!
//! **Every answer here is conservative.** A summary leaves anonymous and local classes out
//! (IDX-07), so a name inside one may bind to a member the summary never saw (an inherited one of
//! the anonymous class's supertype). The scanner looks for such a class around a site, and when
//! it cannot tell (an unterminated comment or literal, a construct it does not follow) it says
//! the scope is open, which costs precision (the answer falls to `possible`) and never
//! correctness. Pinned by `scan::tests`.

use jfl_domain::DeclKind;

/// The byte at `i`, if any.
fn byte(bytes: &[u8], i: usize) -> Option<u8> {
    bytes.get(i).copied()
}

/// Java's separators and identifiers are ASCII; a non-ASCII byte is taken as part of an
/// identifier, which only ever widens a match.
fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80
}

/// The offset before `end` with white space skipped backwards (no comments: a comment between a
/// receiver and its dot makes the receiver unknown, which is conservative).
fn skip_space_back(text: &[u8], mut end: usize) -> usize {
    while let Some(prev) = end.checked_sub(1)
        && byte(text, prev).is_some_and(|b| b.is_ascii_whitespace())
    {
        end = prev;
    }
    end
}

fn skip_space(text: &[u8], mut at: usize) -> usize {
    while byte(text, at).is_some_and(|b| b.is_ascii_whitespace()) {
        at = at.saturating_add(1);
    }
    at
}

/// Past the identifier starting at `at`.
fn ident_end(text: &[u8], mut at: usize) -> usize {
    while byte(text, at).is_some_and(is_ident_byte) {
        at = at.saturating_add(1);
    }
    at
}

/// The identifier ending at `end`: its start.
fn ident_back(text: &[u8], end: usize) -> Option<usize> {
    let mut start = end;
    while let Some(prev) = start.checked_sub(1)
        && byte(text, prev).is_some_and(is_ident_byte)
    {
        start = prev;
    }
    (start < end && !byte(text, start).is_some_and(|b| b.is_ascii_digit())).then_some(start)
}

/// Whether `text[at..]` starts with the two bytes `pair`.
fn starts(text: &[u8], at: usize, pair: [u8; 2]) -> bool {
    text.get(at..at.saturating_add(2)) == Some(&pair[..])
}

/// What is written before a name, as far as the queries use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Before {
    /// Nothing that qualifies it: the name stands alone.
    Nothing,
    /// `this.` or `this::`.
    This,
    /// `Outer.this.`: the qualifier's bytes `start..end` (`Outer`).
    QualifiedThis {
        /// Its first byte.
        start: usize,
        /// Past its last byte.
        end: usize,
    },
    /// An identifier and a dot (or `::`): the identifier's bytes `start..end`.
    Ident {
        /// Its first byte.
        start: usize,
        /// Past its last byte.
        end: usize,
    },
    /// Something else and a dot (or `::`): a call's result, `super`, a literal, an array element.
    Other,
}

/// What precedes the name starting at `start`: `.` or `::` and the receiver before it.
pub(crate) fn before(text: &str, start: usize) -> Before {
    let bytes = text.as_bytes();
    let at = skip_space_back(bytes, start);
    let dot = match (at.checked_sub(1), at.checked_sub(2)) {
        (Some(one), _) if byte(bytes, one) == Some(b'.') => one,
        (_, Some(two)) if starts(bytes, two, *b"::") => two,
        _ => return Before::Nothing,
    };
    let end = skip_space_back(bytes, dot);
    let Some(ident) = ident_back(bytes, end) else {
        return Before::Other;
    };
    match text.get(ident..end) {
        Some("this") => {
            // `Outer.this`: the qualifier names the instance's type (JLS 15.8.4).
            let before_this = skip_space_back(bytes, ident);
            match before_this.checked_sub(1) {
                Some(dot) if byte(bytes, dot) == Some(b'.') => {
                    let q_end = skip_space_back(bytes, dot);
                    match ident_back(bytes, q_end) {
                        Some(q_start) => Before::QualifiedThis {
                            start: q_start,
                            end: q_end,
                        },
                        None => Before::Other,
                    }
                }
                _ => Before::This,
            }
        }
        Some("super") => Before::Other,
        Some(_) => Before::Ident { start: ident, end },
        None => Before::Other,
    }
}

/// Whether `::` follows the name ending at `end` (`Type::new`, `Type::method`).
pub(crate) fn followed_by_method_ref(text: &str, end: usize) -> bool {
    let bytes = text.as_bytes();
    starts(bytes, skip_space(bytes, end), *b"::")
}

/// The dotted name ending at `end` (`com.acme.Money` for the `Money` of `com.acme.Money m;`),
/// identifiers and dots with white space between, or `None` when the chain holds anything else.
pub(crate) fn dotted_ending_at<'t>(text: &'t str, start: usize, end: usize) -> Option<String> {
    let bytes = text.as_bytes();
    let mut parts: Vec<&'t str> = vec![text.get(start..end)?];
    let mut at = start;
    loop {
        let before_dot = skip_space_back(bytes, at);
        let Some(dot) = before_dot.checked_sub(1) else {
            break;
        };
        if byte(bytes, dot) != Some(b'.') {
            break;
        }
        let last = skip_space_back(bytes, dot);
        let first = ident_back(bytes, last)?;
        parts.push(text.get(first..last)?);
        at = first;
    }
    parts.reverse();
    Some(parts.join("."))
}

/// Whether the bare name at `start..end` is an annotation element's key (`@Column(name = "n")`):
/// `(` or `,` before it and a single `=` after it. An assignment written the same way
/// (`for (i = 0; …)`) matches too, which is conservative.
///
/// Pinned by: `scan::tests::should_tell_keys_labels_and_binders_when_bare_names_are_read`.
pub(crate) fn is_element_key(text: &str, start: usize, end: usize) -> bool {
    let bytes = text.as_bytes();
    let after = skip_space(bytes, end);
    let assigns =
        byte(bytes, after) == Some(b'=') && byte(bytes, after.saturating_add(1)) != Some(b'=');
    let opened = skip_space_back(bytes, start)
        .checked_sub(1)
        .and_then(|at| byte(bytes, at))
        .is_some_and(|b| b == b'(' || b == b',');
    assigns && opened
}

/// Whether the bare name starting at `start` is a `case` label (`case RED:`, `case A, B ->`): the
/// identifiers, dots, commas and white space before it reach the keyword `case`. A label names
/// a constant of the selector's type, which syntax does not give.
///
/// Pinned by: `scan::tests::should_tell_keys_labels_and_binders_when_bare_names_are_read`.
pub(crate) fn is_case_label(text: &str, start: usize) -> bool {
    let bytes = text.as_bytes();
    let mut at = start;
    loop {
        at = skip_space_back(bytes, at);
        let Some(prev) = at.checked_sub(1) else {
            return false;
        };
        match byte(bytes, prev) {
            Some(b',' | b'.') => at = prev,
            Some(b) if is_ident_byte(b) => {
                let Some(word) = ident_back(bytes, at) else {
                    return false;
                };
                if text.get(word..at) == Some("case") {
                    return true;
                }
                at = word;
            }
            _ => return false,
        }
    }
}

/// Java's keywords (and literals): a name after one of these is not declared by it (`final T x`
/// declares `x`, not `T`; `return x` uses `x`). `var` is left out: `var x` declares `x`.
const KEYWORDS: [&str; 53] = [
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "final",
    "finally",
    "float",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "native",
    "new",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "try",
    "void",
    "volatile",
    "while",
    "yield",
    "sealed",
    "permits",
];

/// Whether `name` may be declared as a local in `text[from..to]` (a member's text): written after
/// a type (`Integer count`, `List<T> count`, `int[] count`, `var count`), as a lambda parameter
/// (`count ->`, `(a, count) ->`). Pattern variables, catch, for and resource variables are of the
/// first form. Strings and comments are not skipped, which only finds more (conservative): a bare
/// use of such a name in the member is then never read as a field.
///
/// Pinned by: `scan::tests::should_tell_keys_labels_and_binders_when_bare_names_are_read`.
pub(crate) fn may_bind_locally(text: &str, from: usize, to: usize, name: &str) -> bool {
    let Some(region) = text.get(..to.min(text.len())) else {
        return true;
    };
    let bytes = region.as_bytes();
    let mut search = from.min(region.len());
    while let Some(found) = region.get(search..).and_then(|rest| rest.find(name)) {
        let start = search.saturating_add(found);
        let end = start.saturating_add(name.len());
        search = end;
        let whole = !start
            .checked_sub(1)
            .and_then(|b| byte(bytes, b))
            .is_some_and(is_ident_byte)
            && !byte(bytes, end).is_some_and(is_ident_byte);
        if !whole {
            continue;
        }
        let next = skip_space(bytes, end);
        if starts(bytes, next, *b"->") {
            return true;
        }
        let prev_end = skip_space_back(bytes, start);
        let Some(prev) = prev_end.checked_sub(1) else {
            continue;
        };
        match byte(bytes, prev) {
            Some(b'>' | b']') => {
                // `List<T> name`, `int[] name`; but not `x -> name` or `a >= name`.
                let arrow = prev.checked_sub(1).and_then(|p| byte(bytes, p));
                if !matches!(arrow, Some(b'-' | b'>' | b'<' | b'=')) {
                    return true;
                }
            }
            Some(b'.') if starts(bytes, prev.saturating_sub(2), *b"..") => return true,
            Some(b) if is_ident_byte(b) => {
                let word_start = ident_back(bytes, prev_end).unwrap_or(prev_end);
                let word = region.get(word_start..prev_end).unwrap_or("");
                // A primitive type declares (`int x`); other keywords do not; `@Name x` is an
                // annotation before a type.
                let primitive = matches!(
                    word,
                    "boolean" | "byte" | "char" | "short" | "int" | "long" | "float" | "double"
                );
                let annotation = word_start
                    .checked_sub(1)
                    .is_some_and(|at| byte(bytes, at) == Some(b'@'));
                if primitive || (!KEYWORDS.contains(&word) && !annotation) {
                    return true;
                }
            }
            Some(b'(' | b',') if matches!(byte(bytes, next), Some(b',' | b')')) => {
                // A parameter of a lambda whose list is followed by `->`.
                let mut depth = 0_usize;
                let mut at = next;
                while let Some(b) = byte(bytes, at) {
                    match b {
                        b'(' => depth = depth.saturating_add(1),
                        b')' if depth == 0 => {
                            if starts(bytes, skip_space(bytes, at.saturating_add(1)), *b"->") {
                                return true;
                            }
                            break;
                        }
                        b')' => depth = depth.saturating_sub(1),
                        _ => {}
                    }
                    at = at.saturating_add(1);
                }
            }
            _ => {}
        }
    }
    false
}

/// What may enclose a site that the summary does not hold.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Scope {
    /// The site may be inside an anonymous or local class body, whose members the summary does
    /// not hold.
    pub(crate) anonymous: bool,
    /// The names of local classes, interfaces, enums and records declared in the region before
    /// the site (all of them, not only those still in scope: wider is conservative).
    pub(crate) local_types: Vec<String>,
    /// The scanner lost track (an unterminated comment or literal, unbalanced brackets): treat
    /// everything as open.
    pub(crate) unknown: bool,
}

impl Scope {
    /// Whether a name could bind to something the summary does not hold.
    pub(crate) fn open(&self) -> bool {
        self.anonymous || self.unknown
    }
}

const LOCAL_TYPE_KEYWORDS: [&str; 4] = ["class", "interface", "enum", "record"];

/// Scans `text[region_start..site]`, the innermost member declaration around a site (or the type
/// around it when no member is), for anonymous and local class bodies still open at the site, and
/// local type declarations.
///
/// Pinned by: `scan::tests::should_find_an_anonymous_body_when_the_site_is_inside_one` and
/// `scan::tests::should_close_an_anonymous_body_when_the_site_is_after_it`.
pub(crate) fn scope(text: &str, region_start: usize, site: usize) -> Scope {
    let Some(region) = text.get(..site) else {
        return Scope {
            unknown: true,
            ..Scope::default()
        };
    };
    let bytes = region.as_bytes();
    let mut out = Scope::default();
    // The brace depth of each anonymous body still open.
    let mut open_bodies: Vec<usize> = Vec::new();
    let mut depth = 0_usize;
    // Set after `new`, until its argument list ends and a body may follow.
    let mut after_new: Option<NewState> = None;
    // Set after a local type's name, until its body opens.
    let mut after_local = false;
    let mut i = region_start;
    while let Some(b) = byte(bytes, i) {
        let next = i.saturating_add(1);
        match b {
            b'/' if byte(bytes, next) == Some(b'/') => {
                while byte(bytes, i).is_some_and(|c| c != b'\n') {
                    i = i.saturating_add(1);
                }
                continue;
            }
            b'/' if byte(bytes, next) == Some(b'*') => {
                let after = i.saturating_add(2);
                let Some(close) = region.get(after..).and_then(|rest| rest.find("*/")) else {
                    out.unknown = true;
                    return out;
                };
                i = after.saturating_add(close).saturating_add(2);
                continue;
            }
            b'"' => {
                let Some(past) = skip_string(region, i) else {
                    out.unknown = true;
                    return out;
                };
                i = past;
                continue;
            }
            b'\'' => {
                let Some(past) = skip_char(bytes, i) else {
                    out.unknown = true;
                    return out;
                };
                i = past;
                continue;
            }
            b'(' => {
                if let Some(state) = after_new.as_mut() {
                    state.parens = state.parens.saturating_add(1);
                }
            }
            b')' => {
                if let Some(state) = after_new.as_mut() {
                    match state.parens.checked_sub(1) {
                        None => after_new = None,
                        Some(left) => {
                            state.parens = left;
                            state.args_closed = left == 0;
                        }
                    }
                }
            }
            b'{' => {
                if after_new.is_some_and(|s| s.args_closed || s.annotated) || after_local {
                    open_bodies.push(depth);
                }
                after_local = false;
                after_new = None;
                depth = depth.saturating_add(1);
            }
            b'}' => {
                let Some(outer) = depth.checked_sub(1) else {
                    out.unknown = true;
                    return out;
                };
                depth = outer;
                if open_bodies.last() == Some(&depth) {
                    open_bodies.pop();
                }
                after_new = None;
            }
            b'@' => {
                if let Some(state) = after_new.as_mut() {
                    state.annotated = true;
                }
            }
            b';' => {
                after_new = None;
                after_local = false;
            }
            b'[' => after_new = None,
            _ if is_ident_byte(b) => {
                let start = i;
                i = ident_end(bytes, i);
                let word = region.get(start..i).unwrap_or("");
                let after_dot = skip_space_back(bytes, start)
                    .checked_sub(1)
                    .is_some_and(|dot| byte(bytes, dot) == Some(b'.'));
                if word == "new" {
                    after_new = Some(NewState::default());
                } else if after_new.is_some_and(|s| s.args_closed && !s.annotated) {
                    // `new T() x`: no body follows.
                    after_new = None;
                } else if !after_dot && LOCAL_TYPE_KEYWORDS.contains(&word) {
                    let name_start = skip_space(bytes, i);
                    let name_end = ident_end(bytes, name_start);
                    if let Some(name) = region.get(name_start..name_end)
                        && !name.is_empty()
                    {
                        out.local_types.push(name.to_owned());
                        after_local = true;
                    }
                }
                continue;
            }
            _ => {}
        }
        i = next;
    }
    out.anonymous = !open_bodies.is_empty();
    out
}

#[derive(Debug, Clone, Copy, Default)]
struct NewState {
    parens: usize,
    args_closed: bool,
    /// An annotation came after `new` (`new @A(x) T() {`): the parentheses are not followed,
    /// and the next `{` before a `;` or `[` is taken for a body (conservative).
    annotated: bool,
}

/// Past the string literal (or text block) starting at `at`.
fn skip_string(text: &str, at: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let block = |i: usize| text.get(i..).is_some_and(|rest| rest.starts_with("\"\"\""));
    if block(at) {
        let mut i = at.saturating_add(3);
        loop {
            match byte(bytes, i)? {
                b'\\' => i = i.saturating_add(2),
                b'"' if block(i) => return Some(i.saturating_add(3)),
                _ => i = i.saturating_add(1),
            }
        }
    }
    skip_quoted(bytes, at, b'"')
}

/// Past the character literal starting at `at`.
fn skip_char(bytes: &[u8], at: usize) -> Option<usize> {
    skip_quoted(bytes, at, b'\'')
}

/// Past the one-line literal opened by `quote` at `at`.
fn skip_quoted(bytes: &[u8], at: usize, quote: u8) -> Option<usize> {
    let mut i = at.saturating_add(1);
    loop {
        match byte(bytes, i)? {
            b'\\' => i = i.saturating_add(2),
            b'\n' => return None,
            b if b == quote => return Some(i.saturating_add(1)),
            _ => i = i.saturating_add(1),
        }
    }
}

/// The modifiers that may precede a callable's type-parameter clause.
const MODIFIERS: [&str; 12] = [
    "public",
    "protected",
    "private",
    "static",
    "final",
    "abstract",
    "default",
    "synchronized",
    "native",
    "strictfp",
    "sealed",
    "non",
];

/// Whether a declaration of `kind` named `declared`, with this summary `signature`, declares a
/// type parameter `name`: a type's clause follows its name (`class Box<E>`), a method's or
/// constructor's precedes its return type or name, after annotations and modifiers
/// (`public <T> T pick(…)`). Other kinds declare none. When the signature cannot be followed (an
/// annotation's arguments that do not end), the answer is yes, which is conservative.
///
/// Pinned by: `scan::tests::should_find_a_type_parameter_when_a_signature_declares_one`.
pub(crate) fn declares_type_parameter(
    signature: &str,
    declared: &str,
    kind: DeclKind,
    name: &str,
) -> bool {
    let bytes = signature.as_bytes();
    let clause = if kind.is_type() {
        let mut at = 0;
        loop {
            let start = skip_space(bytes, at);
            let Some(b) = byte(bytes, start) else {
                return false;
            };
            if !is_ident_byte(b) {
                at = start.saturating_add(1);
                continue;
            }
            let end = ident_end(bytes, start);
            if signature.get(start..end) == Some(declared) {
                break skip_space(bytes, end);
            }
            at = end;
        }
    } else if matches!(kind, DeclKind::Method | DeclKind::Constructor) {
        let mut at = 0;
        loop {
            at = skip_space(bytes, at);
            match byte(bytes, at) {
                Some(b'@') => {
                    let mut end = at.saturating_add(1);
                    while byte(bytes, end).is_some_and(|b| is_ident_byte(b) || b == b'.') {
                        end = ident_end(bytes, end).max(end.saturating_add(1));
                    }
                    let open = skip_space(bytes, end);
                    at = if byte(bytes, open) == Some(b'(') {
                        match skip_parens(signature, open) {
                            Some(past) => past,
                            None => return true,
                        }
                    } else {
                        end
                    };
                }
                Some(b) if is_ident_byte(b) => {
                    let end = ident_end(bytes, at);
                    let word = signature.get(at..end).unwrap_or("");
                    if !MODIFIERS.contains(&word) {
                        break at;
                    }
                    // `non-sealed`.
                    at = if word == "non" && byte(bytes, end) == Some(b'-') {
                        ident_end(bytes, end.saturating_add(1))
                    } else {
                        end
                    };
                }
                _ => break at,
            }
        }
    } else {
        return false;
    };
    if byte(bytes, clause) != Some(b'<') {
        return false;
    }
    let mut depth = 0_usize;
    let mut expect_param = false;
    let mut i = clause;
    while let Some(b) = byte(bytes, i) {
        match b {
            b'<' => {
                depth = depth.saturating_add(1);
                expect_param = depth == 1;
            }
            b'>' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return false;
                }
            }
            b',' if depth == 1 => expect_param = true,
            _ if is_ident_byte(b) => {
                let start = i;
                i = ident_end(bytes, i);
                if expect_param && signature.get(start..i) == Some(name) {
                    return true;
                }
                expect_param = false;
                continue;
            }
            b'@' => {}
            _ if b.is_ascii_whitespace() => {}
            _ => expect_param = false,
        }
        i = i.saturating_add(1);
    }
    false
}

/// Past the parenthesised group opened at `at`, string and character literals skipped.
fn skip_parens(text: &str, at: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0_usize;
    let mut i = at;
    loop {
        match byte(bytes, i)? {
            b'(' => depth = depth.saturating_add(1),
            b')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i.saturating_add(1));
                }
            }
            b'"' => {
                i = skip_string(text, i)?;
                continue;
            }
            b'\'' => {
                i = skip_char(bytes, i)?;
                continue;
            }
            _ => {}
        }
        i = i.saturating_add(1);
    }
}

/// The most bytes of Javadoc a hover carries.
pub(crate) const MAX_DOC_BYTES: usize = 16 * 1024;

/// A Javadoc comment's words: `/**`, `*/` and each line's leading `*` dropped, lines trimmed,
/// blank lines at either end dropped; cut at [`MAX_DOC_BYTES`] on a character boundary.
///
/// Pinned by: `scan::tests::should_strip_the_comment_markers_when_javadoc_is_read`.
pub(crate) fn javadoc_words(comment: &str) -> String {
    let body = comment
        .strip_prefix("/**")
        .unwrap_or(comment)
        .strip_suffix("*/")
        .unwrap_or(comment);
    let lines: Vec<&str> = body
        .lines()
        .map(|line| {
            let line = line.trim();
            line.strip_prefix('*').map_or(line, str::trim_start)
        })
        .collect();
    let first = lines
        .iter()
        .position(|l| !l.is_empty())
        .unwrap_or(lines.len());
    let last = lines
        .iter()
        .rposition(|l| !l.is_empty())
        .map_or(first, |l| l.saturating_add(1));
    let mut words = lines.get(first..last).unwrap_or(&[]).join("\n");
    if words.len() > MAX_DOC_BYTES {
        let mut end = MAX_DOC_BYTES;
        while !words.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        words.truncate(end);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, needle: &str) -> usize {
        text.find(needle).unwrap()
    }

    #[test]
    fn should_read_the_receiver_when_a_name_follows_a_dot() {
        // given
        let text = "a = Billing . standard(); this.audit(x); f().g(); x::y; super.z(); h();";
        // when
        let found = [
            before(text, at(text, "standard")),
            before(text, at(text, "audit")),
            before(text, at(text, "g()")),
            before(text, at(text, "y;")),
            before(text, at(text, "z()")),
            before(text, at(text, "h()")),
        ];
        // then
        let billing = at(text, "Billing");
        assert_eq!(
            found,
            [
                Before::Ident {
                    start: billing,
                    end: billing + 7
                },
                Before::This,
                Before::Other,
                Before::Ident {
                    start: at(text, "x::"),
                    end: at(text, "x::") + 1
                },
                Before::Other,
                Before::Nothing,
            ]
        );
    }

    #[test]
    fn should_join_the_qualifier_when_a_type_name_is_qualified() {
        // given
        let text = "com . acme.Money m; Money n;";
        let first = at(text, "Money");
        let second = at(text, "Money n");
        // when
        let qualified = dotted_ending_at(text, first, first + 5);
        let simple = dotted_ending_at(text, second, second + 5);
        // then
        assert_eq!(qualified.as_deref(), Some("com.acme.Money"));
        assert_eq!(simple.as_deref(), Some("Money"));
    }

    #[test]
    fn should_find_an_anonymous_body_when_the_site_is_inside_one() {
        // given
        let text = "Runnable task() { return new Runnable() { public void run() { size(); } }; }";
        // when
        let scope = scope(text, 0, at(text, "size"));
        // then
        assert!(scope.anonymous);
        assert!(scope.open());
    }

    #[test]
    fn should_find_an_anonymous_body_when_its_type_is_annotated() {
        // given
        let text = "void m() { Object o = new @A(x = 1) Object() { void z() { size(); } }; }";
        // when
        let scope = scope(text, 0, at(text, "size"));
        // then
        assert!(scope.open());
    }

    #[test]
    fn should_close_an_anonymous_body_when_the_site_is_after_it() {
        // given
        let text = "void m() { Object o = new Object() { int x() { return 1; } }; size(); \
                    int[] a = new int[] {1}; String s = \"new X() {\"; char c = '{'; \
                    /* new Y() { */ new Z(); after(); }";
        // when
        let first = scope(text, 0, at(text, "size"));
        let second = scope(text, 0, at(text, "after"));
        // then
        assert_eq!(first, Scope::default());
        assert_eq!(second, Scope::default());
    }

    #[test]
    fn should_tell_keys_labels_and_binders_when_bare_names_are_read() {
        // given
        let key = "@Column(name = \"n\") String name; x == name;";
        let label = "switch (c) { case RED: f(RED); case A, B -> g(); }";
        let member = "int f(Object o, List<String> vs) { if (o instanceof Integer count) return count; \
                      vs.forEach(text -> text.length()); h((a, item) -> 0); return field + other; }";
        let field = "@Nullable private final Billing billing = Billing.standard();";
        // when
        let keys = [
            is_element_key(key, at(key, "name ="), at(key, "name =") + 4),
            is_element_key(key, at(key, "name;"), at(key, "name;") + 4),
        ];
        let labels = [
            is_case_label(label, at(label, "RED:")),
            is_case_label(label, at(label, "RED)")),
            is_case_label(label, at(label, "B ->")),
        ];
        let binders = [
            may_bind_locally(member, 0, member.len(), "count"),
            may_bind_locally(member, 0, member.len(), "text"),
            may_bind_locally(member, 0, member.len(), "item"),
            may_bind_locally(member, 0, member.len(), "field"),
            may_bind_locally(member, 0, member.len(), "other"),
            may_bind_locally(field, 0, field.len(), "Billing"),
            may_bind_locally(field, 0, field.len(), "billing"),
        ];
        // then
        assert_eq!(keys, [true, false]);
        assert_eq!(labels, [true, false, true]);
        assert_eq!(binders, [true, true, true, false, false, false, true]);
    }

    #[test]
    fn should_read_the_qualifier_when_this_is_qualified() {
        // given
        let text = "Outer.this.close(); this.open();";
        // when
        let qualified = before(text, at(text, "close"));
        let plain = before(text, at(text, "open"));
        // then
        assert_eq!(qualified, Before::QualifiedThis { start: 0, end: 5 });
        assert_eq!(plain, Before::This);
    }

    #[test]
    fn should_find_a_local_class_body_when_the_site_is_inside_one() {
        // given
        let text = "void m() { class Local extends Base { void run() { size(); } } after(); }";
        // when
        let inside = scope(text, 0, at(text, "size"));
        let after = scope(text, 0, at(text, "after"));
        // then
        assert!(inside.open());
        assert!(!after.open());
        assert_eq!(after.local_types, vec!["Local".to_owned()]);
    }

    #[test]
    fn should_record_local_types_when_a_method_declares_one() {
        // given
        let text = "void m() { class Local {} Object c = Local.class; use(); }";
        // when
        let scope = scope(text, 0, at(text, "use"));
        // then
        assert_eq!(scope.local_types, vec!["Local".to_owned()]);
        assert!(!scope.open());
    }

    #[test]
    fn should_call_the_scope_unknown_when_a_comment_or_literal_never_ends() {
        // given
        let comment = "void m() { /* never closed x(); ";
        let literal = "void m() { String s = \"never closed\n x(); }";
        // when
        let first = scope(comment, 0, comment.len());
        let second = scope(literal, 0, at(literal, "x()"));
        // then
        assert!(first.open());
        assert!(second.open());
    }

    #[test]
    fn should_find_a_type_parameter_when_a_signature_declares_one() {
        // given
        let method =
            "@SafeVarargs(x = \")\") public static <T extends Comparable<T>, U> T pick(T a, U b)";
        let generic_return = "public List<Receipt> receipts()";
        let class = "class Box<E extends Base<Money>> extends Base<Receipt>";
        let field = "private final List<Receipt> receipts";
        // when
        let found = [
            declares_type_parameter(method, "pick", DeclKind::Method, "T"),
            declares_type_parameter(method, "pick", DeclKind::Method, "U"),
            declares_type_parameter(method, "pick", DeclKind::Method, "Comparable"),
            declares_type_parameter(generic_return, "receipts", DeclKind::Method, "Receipt"),
            declares_type_parameter(class, "Box", DeclKind::Class, "E"),
            declares_type_parameter(class, "Box", DeclKind::Class, "Money"),
            declares_type_parameter(class, "Box", DeclKind::Class, "Receipt"),
            declares_type_parameter(field, "receipts", DeclKind::Field, "Receipt"),
        ];
        // then
        assert_eq!(found, [true, true, false, false, true, false, false, false]);
    }

    #[test]
    fn should_strip_the_comment_markers_when_javadoc_is_read() {
        // given
        let comment =
            "/**\n     * Charges {@code amount} once.\n     *\n     * Twice is wrong.\n     */";
        // when
        let words = javadoc_words(comment);
        // then
        assert_eq!(words, "Charges {@code amount} once.\n\nTwice is wrong.");
        assert_eq!(javadoc_words("/** One line. */"), "One line.");
    }

    #[test]
    fn should_see_a_method_reference_when_two_colons_follow() {
        // given
        let text = "Function<String, R> f = R :: new;";
        let r = at(text, "R ::");
        // when
        let found = followed_by_method_ref(text, r + 1);
        // then
        assert!(found);
        assert!(!followed_by_method_ref(text, at(text, "f =") + 1));
    }
}
