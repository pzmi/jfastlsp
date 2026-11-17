//! Splits Rust source into lines of code and comment text, with the contents of string and
//! character literals removed, so the line-oriented scan never matches inside a literal or a
//! comment, and can count braces. Deliberately small (fnd §3.2): nested block comments, the
//! literal prefixes `b`, `c`, `r`, `br` and `cr` (raw strings with any number of hashes), raw
//! identifiers and the lifetime/char-literal ambiguity are handled.
//!
//! It fails closed (fnd §3, FND-08): an identifier glued to a quote or a `#` that is not one of
//! those prefixes (edition 2021 reserves every other prefix, so such code does not compile), an
//! escaped char literal that does not close on its line, and a file that ends inside a string or
//! a block comment are a [`LexError`] rather than a guess, because a desynchronised lexer hides
//! the code after it from the scan (`cr"\"` once hid an `#[allow]`).
//!
//! Pinned by: tests/rules.rs::should_find_nothing_when_rule_words_sit_in_literals_and_comments,
//! ::should_see_the_attribute_after_each_tricky_literal_when_the_lexer_reads_it and
//! ::should_report_hidden_source_when_the_lexer_cannot_read_a_file.

/// One source line.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Line {
    /// The code, literal contents dropped (the quotes stay).
    pub code: String,
    /// The text of `//` comments on the line, after the slashes.
    pub comment: String,
}

/// Where and why the lexer stopped: the code from there on cannot be read reliably.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    /// The 1-based line.
    pub line: usize,
    /// What could not be read.
    pub reason: &'static str,
}

#[derive(Clone, Copy)]
enum State {
    Code,
    LineComment,
    Block(u32),
    Str,
    StrEscape,
    Raw(u32),
    RawClosing { hashes: u32, left: u32 },
}

type Chars<'a> = std::iter::Peekable<std::str::Chars<'a>>;

/// The lines of `text`, or where the lexer could not follow it.
pub fn lines(text: &str) -> Result<Vec<Line>, LexError> {
    let mut out = vec![Line::default()];
    let mut state = State::Code;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\n' {
            out.push(Line::default());
            if matches!(state, State::LineComment) {
                state = State::Code;
            }
            continue;
        }
        let number = out.len();
        let Some(line) = out.last_mut() else { break };
        state = match state {
            State::LineComment => {
                line.comment.push(c);
                State::LineComment
            }
            State::Block(depth) => {
                if c == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    match depth.checked_sub(1) {
                        Some(0) | None => State::Code,
                        Some(rest) => State::Block(rest),
                    }
                } else if c == '/' && chars.peek() == Some(&'*') {
                    chars.next();
                    State::Block(depth.saturating_add(1))
                } else {
                    State::Block(depth)
                }
            }
            State::Str => match c {
                '\\' => State::StrEscape,
                '"' => {
                    line.code.push('"');
                    State::Code
                }
                _ => State::Str,
            },
            State::StrEscape => State::Str,
            State::Raw(hashes) => {
                if c == '"' {
                    if hashes == 0 {
                        line.code.push('"');
                        State::Code
                    } else {
                        State::RawClosing {
                            hashes,
                            left: hashes,
                        }
                    }
                } else {
                    State::Raw(hashes)
                }
            }
            State::RawClosing { hashes, left } => match (c, left.checked_sub(1)) {
                ('#', Some(0)) => {
                    line.code.push('"');
                    State::Code
                }
                ('#', Some(rest)) => State::RawClosing { hashes, left: rest },
                ('"', _) => State::RawClosing {
                    hashes,
                    left: hashes,
                },
                _ => State::Raw(hashes),
            },
            State::Code => code(c, &mut chars, line).map_err(|reason| LexError {
                line: number,
                reason,
            })?,
        };
    }
    let line = out.len();
    match state {
        State::Code | State::LineComment => Ok(out),
        State::Block(_) => Err(LexError {
            line,
            reason: "the file ends inside a block comment",
        }),
        State::Str | State::StrEscape | State::Raw(_) | State::RawClosing { .. } => Err(LexError {
            line,
            reason: "the file ends inside a string literal",
        }),
    }
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn code(c: char, chars: &mut Chars<'_>, line: &mut Line) -> Result<State, &'static str> {
    match c {
        '/' if chars.peek() == Some(&'/') => {
            chars.next();
            Ok(State::LineComment)
        }
        '/' if chars.peek() == Some(&'*') => {
            chars.next();
            Ok(State::Block(1))
        }
        '"' => {
            line.code.push('"');
            Ok(State::Str)
        }
        '\'' => {
            quote(chars, line)?;
            Ok(State::Code)
        }
        _ if is_ident(c) => word(c, chars, line),
        _ => {
            line.code.push(c);
            Ok(State::Code)
        }
    }
}

/// A whole identifier or number starting with `first`, and the literal it may prefix.
fn word(first: char, chars: &mut Chars<'_>, line: &mut Line) -> Result<State, &'static str> {
    let mut word = String::from(first);
    while let Some(&next) = chars.peek().filter(|n| is_ident(**n)) {
        word.push(next);
        chars.next();
    }
    match (word.as_str(), chars.peek().copied()) {
        ("b" | "c", Some('"')) => {
            chars.next();
            line.code.push('"');
            Ok(State::Str)
        }
        ("r" | "br" | "cr", Some('"' | '#')) => {
            let mut hashes: u32 = 0;
            while chars.peek() == Some(&'#') {
                chars.next();
                hashes = hashes.saturating_add(1);
            }
            match chars.peek().copied() {
                Some('"') => {
                    chars.next();
                    line.code.push('"');
                    Ok(State::Raw(hashes))
                }
                // A raw identifier, `r#name`.
                Some(next) if word == "r" && hashes == 1 && is_ident(next) => {
                    line.code.push_str("r#");
                    Ok(State::Code)
                }
                _ => Err("a raw string prefix that opens no string"),
            }
        }
        ("b", Some('\'')) => {
            line.code.push('b');
            Ok(State::Code)
        }
        (_, Some('"' | '\'' | '#')) => {
            Err("an identifier glued to a quote or `#` that is no Rust literal prefix")
        }
        _ => {
            line.code.push_str(&word);
            Ok(State::Code)
        }
    }
}

/// After a `'`: a char literal (dropped) or a lifetime or label (kept).
fn quote(chars: &mut Chars<'_>, line: &mut Line) -> Result<(), &'static str> {
    match chars.peek().copied() {
        Some('\\') => {
            // An escaped char literal: skip the escape, then to its closing quote on this line.
            chars.next();
            match chars.next() {
                None | Some('\n') => return Err("an escaped char literal that does not close"),
                Some(_) => {}
            }
            loop {
                match chars.next() {
                    Some('\'') => break,
                    None | Some('\n') => return Err("an escaped char literal that does not close"),
                    Some(_) => {}
                }
            }
            line.code.push_str("''");
        }
        Some(first) if first != '\n' => {
            chars.next();
            if chars.peek() == Some(&'\'') {
                chars.next();
                line.code.push_str("''");
            } else {
                // A lifetime or a label.
                line.code.push('\'');
                line.code.push(first);
            }
        }
        _ => line.code.push('\''),
    }
    Ok(())
}
