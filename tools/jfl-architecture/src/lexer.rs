//! Splits Rust source into lines of code and comment text, with the contents of string and
//! character literals removed, so the line-oriented scan never matches inside a literal or a
//! comment, and can count braces. Deliberately small (fnd §3.2): nested block comments, raw
//! strings with hashes and the lifetime/char-literal ambiguity are handled; anything subtler binds
//! by review.
//!
//! Pinned by: tests/rules.rs::should_find_nothing_when_rule_words_sit_in_literals_and_comments.

/// One source line.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Line {
    /// The code, literal contents dropped (the quotes stay).
    pub code: String,
    /// The text of `//` comments on the line, after the slashes.
    pub comment: String,
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

/// The lines of `text`.
pub fn lines(text: &str) -> Vec<Line> {
    let mut out = vec![Line::default()];
    let mut state = State::Code;
    let mut prev = '\n';
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\n' {
            out.push(Line::default());
            if matches!(state, State::LineComment) {
                state = State::Code;
            }
            prev = c;
            continue;
        }
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
            State::Code => code(c, prev, &mut chars, line),
        };
        prev = c;
    }
    out
}

fn code(
    c: char,
    prev: char,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    line: &mut Line,
) -> State {
    let ident = |ch: char| ch.is_alphanumeric() || ch == '_';
    match c {
        '/' if chars.peek() == Some(&'/') => {
            chars.next();
            State::LineComment
        }
        '/' if chars.peek() == Some(&'*') => {
            chars.next();
            State::Block(1)
        }
        '"' => {
            line.code.push('"');
            State::Str
        }
        'r' if (!ident(prev) || prev == 'b') && matches!(chars.peek(), Some('#' | '"')) => {
            let mut hashes: u32 = 0;
            while chars.peek() == Some(&'#') {
                chars.next();
                hashes = hashes.saturating_add(1);
            }
            if chars.peek() == Some(&'"') {
                chars.next();
                line.code.push('"');
                State::Raw(hashes)
            } else {
                // A raw identifier, `r#name`.
                line.code.push('r');
                line.code.extend(std::iter::repeat_n(
                    '#',
                    usize::try_from(hashes).unwrap_or(0),
                ));
                State::Code
            }
        }
        '\'' => {
            match chars.peek().copied() {
                Some('\\') => {
                    // An escaped char literal: skip to its closing quote.
                    chars.next();
                    chars.next();
                    for next in chars.by_ref() {
                        if next == '\'' || next == '\n' {
                            break;
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
            State::Code
        }
        _ => {
            line.code.push(c);
            State::Code
        }
    }
}
