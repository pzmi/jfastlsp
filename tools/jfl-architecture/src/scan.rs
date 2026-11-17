//! The source scan of fnd §3.2 item 3 over every `.rs` file under each member's directory
//! (other members nested in it, the fixture workspaces under `tools/jfl-architecture/fixtures/`,
//! and the root `target/` excepted). Limit: an attribute a macro builds from a fragment
//! (`#[$a]`) is invisible here and binds by review:
//!
//! - `ARCH-test-name`: a `#[test]` (or `#[<path>::test]`) function is named
//!   `should_<outcome>_when_<condition>`;
//! - `ARCH-test-structure`: its body carries `// given`, `// when`, `// then`, in that order;
//! - `ARCH-ignored-test`: no `#[ignore]`, also not through `cfg_attr`;
//! - `ARCH-lint-suppression`: every lint of an `#[allow]` or `#[expect]` (also through
//!   `cfg_attr`), and every `#[macro_export]`, has an `[[exceptions.lints]]` row naming exactly
//!   that lint in that file and citing an existing ADR; a row that matches nothing, or cites no
//!   ADR file, fails too (fnd §3.1);
//! - `ARCH-platform-branch`: no `cfg!(...)` on the platform (`target_os`, `target_family`,
//!   `target_arch`, `target_env`, `target_vendor`, `unix`, `windows`): `#[cfg]` items instead;
//! - `ARCH-hidden-source`: no `#[path = ...]` and no `include!`, which would compile code the
//!   scan does not read (a target's root outside its crate is the graph check's);
//! - `ARCH-domain-purity`: a pure crate's `src/lib.rs` opens with `#![no_std]` (fnd §2.3).
//!
//! The scan reads code with literals and comments removed ([`crate::lexer`]), joined into one
//! text, so an attribute or a `cfg!` spread over several lines is read whole (brackets are
//! counted until they balance) and a rule's name inside a string or a comment never trips it.
//!
//! Pinned by: tests/rules.rs::should_report_each_rule_with_its_id_and_path_when_its_negative_case_runs
//! and tests/rules.rs::should_find_nothing_when_rule_words_sit_in_literals_and_comments.

use std::collections::{BTreeMap, BTreeSet};

use crate::Finding;
use crate::lexer::{self, Line};
use crate::policy::{LintException, Policy};
use crate::tree::Tree;

/// The fixture workspaces of fnd §3.4: other crates, broken on purpose, never this one's source.
const FIXTURES: &str = "tools/jfl-architecture/fixtures/";

const PLATFORM: [&str; 7] = [
    "target_os",
    "target_family",
    "target_arch",
    "target_env",
    "target_vendor",
    "unix",
    "windows",
];

/// Scans every member (`members`: name to directory) and checks the exception rows.
pub fn check(tree: &dyn Tree, policy: &Policy, members: &BTreeMap<String, String>) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut used = BTreeSet::new();
    for (name, dir) in members {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let all = tree.files_under(dir);
        // Another member below this one is scanned as itself, and the fixture workspaces are
        // deliberately broken; nothing else is skipped, so a stray Cargo.toml hides nothing.
        let nested: Vec<String> = members
            .values()
            .filter(|other| *other != dir && !other.is_empty())
            .map(|other| format!("{other}/"))
            .chain(std::iter::once(FIXTURES.to_owned()))
            .filter(|n| n.starts_with(&prefix))
            .collect();
        for path in all.iter().filter(|p| p.ends_with(".rs")) {
            if nested.iter().any(|n| path.starts_with(n.as_str())) {
                continue;
            }
            let Some(text) = tree.read(path) else {
                findings.push(Finding::new(
                    "ARCH-metadata",
                    path.as_str(),
                    "cannot be read",
                ));
                continue;
            };
            findings.extend(source(path, &text, &policy.exceptions, &mut used));
        }
        if policy.has_role(name, "pure") {
            let lib = format!("{prefix}src/lib.rs");
            let first = tree
                .read(&lib)
                .and_then(|t| t.lines().next().map(str::to_owned));
            if first.as_deref().map(str::trim) != Some("#![no_std]") {
                findings.push(Finding::new(
                    "ARCH-domain-purity",
                    format!("{lib}:1"),
                    format!(
                        "the pure crate `{name}` must open its lib.rs with `#![no_std]` (fnd §2.3)"
                    ),
                ));
            }
        }
    }
    let adrs = tree.files_under("doc/adr");
    for (index, exception) in policy.exceptions.iter().enumerate() {
        let cited = exception.adr.len() == 4
            && exception.adr.chars().all(|c| c.is_ascii_digit())
            && adrs.iter().any(|f| {
                f.starts_with(&format!("doc/adr/{}-", exception.adr)) && f.ends_with(".md")
            });
        if !cited {
            findings.push(Finding::new(
                "ARCH-lint-suppression",
                "architecture.toml",
                format!(
                    "the exception for `{}` in {} cites ADR `{}`, which has no file in doc/adr/",
                    exception.lint, exception.path, exception.adr
                ),
            ));
        }
        if !used.contains(&index) {
            findings.push(Finding::new(
                "ARCH-lint-suppression",
                "architecture.toml",
                format!(
                    "the exception for `{}` in {} matches no attribute: remove it",
                    exception.lint, exception.path
                ),
            ));
        }
    }
    findings
}

/// What one attribute (or a `cfg_attr` branch of it) does.
#[derive(Debug, PartialEq, Eq)]
enum Effect {
    Test,
    Ignore,
    Suppress(Vec<String>),
    Path,
}

/// The source as one text of code, with each line's start offset.
struct Code {
    chars: Vec<(usize, char)>,
    text: String,
    starts: Vec<usize>,
}

impl Code {
    fn new(lines: &[Line]) -> Self {
        let text = lines
            .iter()
            .map(|l| l.code.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let mut starts = vec![0];
        starts.extend(
            text.char_indices()
                .filter(|(_, c)| *c == '\n')
                .map(|(i, _)| i.saturating_add(1)),
        );
        Self {
            chars: text.char_indices().collect(),
            text,
            starts,
        }
    }

    /// The 1-based line of a byte offset.
    fn line(&self, offset: usize) -> usize {
        self.starts.partition_point(|s| *s <= offset)
    }

    fn char(&self, k: usize) -> Option<char> {
        self.chars.get(k).map(|(_, c)| *c)
    }

    fn offset(&self, k: usize) -> usize {
        self.chars.get(k).map_or(self.text.len(), |(o, _)| *o)
    }

    /// The index of the next non-whitespace char at or after `k`.
    fn skip_space(&self, mut k: usize) -> usize {
        while self.char(k).is_some_and(char::is_whitespace) {
            k = k.saturating_add(1);
        }
        k
    }

    /// From an opening bracket at `k`, the text inside up to its balancing close, and the index
    /// after it; `None` when it never balances.
    fn balanced(&self, k: usize, open: char, close: char) -> Option<(String, usize)> {
        let mut depth: u32 = 0;
        let mut inner = String::new();
        let mut j = k;
        while let Some(c) = self.char(j) {
            if c == open {
                depth = depth.saturating_add(1);
                if depth == 1 {
                    j = j.saturating_add(1);
                    continue;
                }
            } else if c == close {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some((inner, j.saturating_add(1)));
                }
            }
            if !c.is_whitespace() {
                inner.push(c);
            }
            j = j.saturating_add(1);
        }
        None
    }

    /// The identifier starting at `k`, if `k` starts one.
    fn ident(&self, k: usize) -> Option<String> {
        let before = k.checked_sub(1).and_then(|b| self.char(b));
        if before.is_some_and(is_ident) || !self.char(k).is_some_and(is_ident) {
            return None;
        }
        let mut word = String::new();
        let mut j = k;
        while let Some(c) = self.char(j).filter(|c| is_ident(*c)) {
            word.push(c);
            j = j.saturating_add(1);
        }
        Some(word)
    }
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Splits at the commas outside any brackets.
fn top_level(args: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth: i32 = 0;
    let mut start = 0;
    for (i, c) in args.char_indices() {
        match c {
            '(' | '[' | '{' => depth = depth.saturating_add(1),
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(args.get(start..i).unwrap_or_default());
                start = i.saturating_add(1);
            }
            _ => {}
        }
    }
    parts.push(args.get(start..).unwrap_or_default());
    parts.into_iter().filter(|p| !p.is_empty()).collect()
}

/// The effects of one attribute's text (whitespace removed), `cfg_attr` branches included.
fn effects(attribute: &str) -> Vec<Effect> {
    let end = attribute
        .find(|c: char| !(is_ident(c) || c == ':'))
        .unwrap_or(attribute.len());
    let name = attribute.get(..end).unwrap_or_default();
    let rest = attribute.get(end..).unwrap_or_default();
    let args = rest
        .strip_prefix('(')
        .and_then(|r| r.strip_suffix(')'))
        .unwrap_or_default();
    match name {
        "test" => vec![Effect::Test],
        _ if name.ends_with("::test") => vec![Effect::Test],
        "ignore" => vec![Effect::Ignore],
        "allow" | "expect" => vec![Effect::Suppress(
            top_level(args).into_iter().map(str::to_owned).collect(),
        )],
        "macro_export" => vec![Effect::Suppress(vec!["macro_export".to_owned()])],
        "path" => vec![Effect::Path],
        "cfg_attr" => top_level(args)
            .into_iter()
            .skip(1)
            .flat_map(effects)
            .collect(),
        _ => Vec::new(),
    }
}

/// The scan of one file; `used` collects the indices of the exceptions that matched.
pub fn source(
    path: &str,
    text: &str,
    exceptions: &[LintException],
    used: &mut BTreeSet<usize>,
) -> Vec<Finding> {
    let lines = lexer::lines(text);
    let code = Code::new(&lines);
    let mut findings = Vec::new();
    let mut test_pending = false;
    let mut k = 0;
    while let Some(c) = code.char(k) {
        let at = format!("{path}:{}", code.line(code.offset(k)));
        if c == '#' {
            let mut j = code.skip_space(k.saturating_add(1));
            if code.char(j) == Some('!') {
                j = code.skip_space(j.saturating_add(1));
            }
            if code.char(j) == Some('[')
                && let Some((attribute, after)) = code.balanced(j, '[', ']')
            {
                for effect in effects(&attribute) {
                    match effect {
                        Effect::Test => test_pending = true,
                        Effect::Ignore => findings.push(Finding::new(
                            "ARCH-ignored-test",
                            at.as_str(),
                            "`#[ignore]`: a test runs or is deleted, never skipped (fnd §3.5)",
                        )),
                        Effect::Path => findings.push(Finding::new(
                            "ARCH-hidden-source",
                            at.as_str(),
                            "`#[path]` compiles a file from where the scan may not look: move the module",
                        )),
                        Effect::Suppress(lints) => {
                            for lint in lints {
                                let matched: Vec<usize> = exceptions
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, e)| e.path == path && e.lint == lint)
                                    .map(|(i, _)| i)
                                    .collect();
                                if matched.is_empty() {
                                    findings.push(Finding::new(
                                        "ARCH-lint-suppression",
                                        at.as_str(),
                                        format!("`#[{attribute}]` suppresses `{lint}` without an [[exceptions.lints]] row for it citing an ADR (fnd §3.5)"),
                                    ));
                                }
                                used.extend(matched);
                            }
                        }
                    }
                }
                k = after;
                continue;
            }
        }
        if let Some(word) = code.ident(k) {
            let after_word = k.saturating_add(word.chars().count());
            let bang = code.skip_space(after_word);
            let is_macro = code.char(bang) == Some('!');
            let open = code.skip_space(bang.saturating_add(1));
            match word.as_str() {
                "cfg" if is_macro && code.char(open) == Some('(') => {
                    if let Some((inner, _)) = code.balanced(open, '(', ')')
                        && inner
                            .split(|c: char| !is_ident(c))
                            .any(|w| PLATFORM.contains(&w))
                    {
                        findings.push(Finding::new(
                            "ARCH-platform-branch",
                            at.as_str(),
                            "a `cfg!(...)` platform branch: use `#[cfg]` items instead (CLAUDE.md, fnd §3.2)",
                        ));
                    }
                }
                "include" if is_macro => findings.push(Finding::new(
                    "ARCH-hidden-source",
                    at.as_str(),
                    "`include!` compiles code from where the scan may not look: make it a module",
                )),
                "fn" if test_pending => {
                    test_pending = false;
                    let name_at = code.skip_space(after_word);
                    if let Some(name) = code.ident(name_at) {
                        test(path, &name, &code, &lines, k, &mut findings);
                    }
                }
                _ => {}
            }
            k = after_word;
            continue;
        }
        k = k.saturating_add(1);
    }
    findings
}

/// Whether `name` reads `should_<outcome>_when_<condition>`.
pub fn is_test_name(name: &str) -> bool {
    let lower = name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    let Some(rest) = name.strip_prefix("should_") else {
        return false;
    };
    let Some((outcome, condition)) = rest.split_once("_when_") else {
        return false;
    };
    lower
        && !outcome.is_empty()
        && !condition.is_empty()
        && !outcome.starts_with('_')
        && !condition.starts_with('_')
}

fn test(
    path: &str,
    name: &str,
    code: &Code,
    lines: &[Line],
    start: usize,
    findings: &mut Vec<Finding>,
) {
    let first = code.line(code.offset(start));
    let at = format!("{path}:{first}");
    if !is_test_name(name) {
        findings.push(Finding::new(
            "ARCH-test-name",
            at.as_str(),
            format!("the test `{name}` is not named `should_<outcome>_when_<condition>` (ver §2)"),
        ));
    }
    let mut j = start;
    while code.char(j).is_some_and(|c| c != '{') {
        j = j.saturating_add(1);
    }
    let last = code
        .balanced(j, '{', '}')
        .map_or(lines.len(), |(_, after)| {
            code.line(code.offset(after.saturating_sub(1)))
        });
    let mut markers = Vec::new();
    for line in lines
        .iter()
        .skip(first.saturating_sub(1))
        .take(last.saturating_sub(first).saturating_add(1))
    {
        let marker = line.comment.trim();
        for word in ["given", "when", "then"] {
            if marker
                .strip_prefix(word)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with([':', ' ', ',', '.', ';']))
            {
                markers.push(word);
            }
        }
    }
    let mut want = ["given", "when", "then"].into_iter().peekable();
    for marker in markers {
        if want.peek() == Some(&marker) {
            want.next();
        }
    }
    let missing: Vec<&str> = want.collect();
    if !missing.is_empty() {
        findings.push(Finding::new(
            "ARCH-test-structure",
            at.as_str(),
            format!(
                "the test `{name}` lacks `// {}`: its body needs `// given`, `// when`, `// then`, in that order (ver §2)",
                missing.join("`, `// ")
            ),
        ));
    }
}
