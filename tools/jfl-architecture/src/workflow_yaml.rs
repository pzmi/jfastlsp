//! `ARCH-workflow-yaml` (fnd §7.3, FND-09): the decoded check of the YAML under `.github/`, the
//! authority over `scripts/workflows-check.sh`'s line checks, which read raw text and so can be
//! fooled by any spelling YAML decodes (an escaped `"\x72un":` key, a `? run` complex key, a
//! `\x23` in a double-quoted value, a comment that looks like a block start: four review rounds
//! of FND-09). Every `.yml`/`.yaml` file under `.github/` is parsed with `yaml-rust2`, and:
//!
//! - it must parse, hold at most one document, and use no anchor, alias, tag or merge key (`<<`),
//!   so what this check reads is what actionlint and the runner read;
//! - every mapping, at any depth, is walked; for each key that decodes to `run` whose value is a
//!   string, every line of that decoded string holding a shellcheck directive or a zizmor ignore
//!   must be an entry of `allowed_suppressions` in `scripts/workflows-check.sh` (the one list,
//!   pinned in `.config/gate.sha256`) for that file, once; `disable=all` never;
//! - a key that decodes to `shell` holds `bash` or `sh` only, so no step changes its interpreter
//!   to step out of shellcheck;
//! - a key that is not a string (a sequence or mapping key) is refused.
//!
//! Pinned by: tests/workflow_yaml.rs (each review round's payload, built in the test) and
//! tests/architecture.rs (the repository clean).

use std::collections::BTreeMap;

use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust2::scanner::Marker;

use crate::Finding;
use crate::tree::Tree;

/// The rule id.
pub const RULE: &str = "ARCH-workflow-yaml";
/// The script whose `allowed_suppressions` list both layers read.
pub const SUPPRESSIONS: &str = "scripts/workflows-check.sh";
/// The interpreters a `shell:` may name.
pub const SHELLS: [&str; 2] = ["bash", "sh"];

/// One allowed suppression: the file, the directive line (without a leading `# `) and its reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Allowed {
    /// The file, relative to the repository root.
    pub path: String,
    /// The directive line, trimmed, without a leading `# `.
    pub line: String,
}

/// Runs the rule over every YAML file under `.github/` of `tree`.
pub fn check(tree: &dyn Tree) -> Vec<Finding> {
    let allowed = match tree.read(SUPPRESSIONS).map(|text| allowlist(&text)) {
        Some(Ok(allowed)) => allowed,
        Some(Err(why)) => return vec![Finding::new(RULE, SUPPRESSIONS, why)],
        None => return vec![Finding::new(RULE, SUPPRESSIONS, "the file is missing")],
    };
    let mut findings = Vec::new();
    for path in tree.files_under(".github") {
        if !(path.ends_with(".yml") || path.ends_with(".yaml")) {
            continue;
        }
        match tree.read(&path) {
            Some(text) => findings.extend(check_file(&path, &text, &allowed)),
            None => findings.push(Finding::new(RULE, path.as_str(), "the file cannot be read")),
        }
    }
    findings
}

/// The `allowed_suppressions` heredoc of `scripts/workflows-check.sh`: `path|line|reason` lines
/// between `allowed_suppressions="$(cat <<'LIST'` and `LIST`.
pub fn allowlist(script: &str) -> Result<Vec<Allowed>, String> {
    let mut lines = script.lines();
    if !lines
        .by_ref()
        .any(|l| l.trim() == "allowed_suppressions=\"$(cat <<'LIST'")
    {
        return Err("no `allowed_suppressions` list: the suppression allowlist is unknown".into());
    }
    let mut allowed = Vec::new();
    for line in lines {
        if line == "LIST" {
            return Ok(allowed);
        }
        let mut parts = line.splitn(3, '|');
        match (parts.next(), parts.next(), parts.next()) {
            (Some(path), Some(directive), Some(reason)) if !reason.trim().is_empty() => {
                allowed.push(Allowed {
                    path: path.to_owned(),
                    line: directive.to_owned(),
                });
            }
            _ => return Err(format!("an entry that is not `path|line|reason`: `{line}`")),
        }
    }
    Err("the `allowed_suppressions` list has no closing `LIST`".into())
}

/// A decoded node, with the line it starts on.
#[derive(Debug)]
enum Node {
    Scalar(String, usize),
    Seq(Vec<Node>),
    Map(Vec<(Node, Node)>),
}

/// Builds the nodes and records what the shape refuses.
#[derive(Default)]
struct Builder {
    stack: Vec<(Node, usize)>,
    roots: Vec<Node>,
    documents: usize,
    refused: Vec<(usize, &'static str)>,
}

impl Builder {
    fn push(&mut self, node: Node) {
        match self.stack.last_mut() {
            Some((Node::Seq(items), _)) => items.push(node),
            Some((Node::Map(pairs), pending)) => {
                // A mapping collects a key, then its value: an odd count waits for a value.
                if *pending == 0 {
                    pairs.push((node, Node::Scalar(String::new(), 0)));
                    *pending = 1;
                } else {
                    if let Some(last) = pairs.last_mut() {
                        last.1 = node;
                    }
                    *pending = 0;
                }
            }
            _ => self.roots.push(node),
        }
    }
}

impl Builder {
    fn refuse_props(&mut self, line: usize, anchor: usize, tagged: bool) {
        if anchor != 0 {
            self.refused.push((line, "an anchor (&)"));
        }
        if tagged {
            self.refused.push((line, "a tag (!)"));
        }
    }
}

impl MarkedEventReceiver for Builder {
    fn on_event(&mut self, event: Event, mark: Marker) {
        let line = mark.line();
        match event {
            Event::DocumentStart => self.documents = self.documents.saturating_add(1),
            Event::Alias(_) => {
                self.refused.push((line, "an alias (*)"));
                self.push(Node::Scalar(String::new(), line));
            }
            Event::Scalar(value, _, anchor, tag) => {
                self.refuse_props(line, anchor, tag.is_some());
                self.push(Node::Scalar(value, line));
            }
            Event::SequenceStart(anchor, tag) => {
                self.refuse_props(line, anchor, tag.is_some());
                self.stack.push((Node::Seq(Vec::new()), 0));
            }
            Event::MappingStart(anchor, tag) => {
                self.refuse_props(line, anchor, tag.is_some());
                self.stack.push((Node::Map(Vec::new()), 0));
            }
            Event::SequenceEnd | Event::MappingEnd => {
                if let Some((node, _)) = self.stack.pop() {
                    self.push(node);
                }
            }
            Event::StreamStart | Event::StreamEnd | Event::DocumentEnd | Event::Nothing => {}
        }
    }
}

/// The findings of one file: its shape, then every `run` and `shell` value.
pub fn check_file(path: &str, text: &str, allowed: &[Allowed]) -> Vec<Finding> {
    let at = |line: usize| format!("{path}:{line}");
    let mut builder = Builder::default();
    if let Err(error) = Parser::new_from_str(text).load(&mut builder, true) {
        return vec![Finding::new(
            RULE,
            path,
            format!("does not parse as YAML: {error}"),
        )];
    }
    let mut findings: Vec<Finding> = builder
        .refused
        .iter()
        .map(|(line, what)| {
            Finding::new(
                RULE,
                at(*line),
                format!("{what}: the YAML under .github/ uses none (fnd §7.3)"),
            )
        })
        .collect();
    if builder.documents > 1 || builder.roots.len() > 1 {
        findings.push(Finding::new(
            RULE,
            path,
            "more than one YAML document (fnd §7.3)",
        ));
    }
    let mine: Vec<&Allowed> = allowed.iter().filter(|a| a.path == path).collect();
    let mut used: BTreeMap<String, usize> = BTreeMap::new();
    let mut walk = Walk {
        path,
        allowed: &mine,
        used: &mut used,
        findings: &mut findings,
    };
    for root in &builder.roots {
        walk.node(root);
    }
    findings
}

struct Walk<'a> {
    path: &'a str,
    allowed: &'a [&'a Allowed],
    used: &'a mut BTreeMap<String, usize>,
    findings: &'a mut Vec<Finding>,
}

impl Walk<'_> {
    fn finding(&mut self, line: usize, message: String) {
        self.findings
            .push(Finding::new(RULE, format!("{}:{line}", self.path), message));
    }

    fn node(&mut self, node: &Node) {
        match node {
            Node::Scalar(..) => {}
            Node::Seq(items) => {
                for item in items {
                    self.node(item);
                }
            }
            Node::Map(pairs) => {
                for (key, value) in pairs {
                    let Node::Scalar(key, line) = key else {
                        self.finding(
                            line_of(key),
                            "a mapping key that is not a string (fnd §7.3)".to_owned(),
                        );
                        self.node(key);
                        self.node(value);
                        continue;
                    };
                    if key == "<<" {
                        self.finding(*line, "a merge key (<<) (fnd §7.3)".to_owned());
                    }
                    match (key.as_str(), value) {
                        ("run", Node::Scalar(script, start)) => self.script(script, *start),
                        ("shell", Node::Scalar(shell, start))
                            if !SHELLS.contains(&shell.as_str()) =>
                        {
                            self.finding(
                                *start,
                                format!("`shell: {shell}`: only bash or sh, so shellcheck reads every step (fnd §7.3)"),
                            );
                        }
                        _ => self.node(value),
                    }
                }
            }
        }
    }

    fn script(&mut self, script: &str, start: usize) {
        for (offset, raw) in script.lines().enumerate() {
            let line = raw.trim();
            if !is_suppression(line) {
                continue;
            }
            let at = start.saturating_add(offset);
            if disables_all(line) {
                self.finding(
                    at,
                    format!(
                        "`{line}` in a decoded run: disables every check; never allowed (fnd §7.3)"
                    ),
                );
                continue;
            }
            let listed = self
                .allowed
                .iter()
                .any(|a| a.line == line || line.strip_prefix("# ") == Some(a.line.as_str()));
            if !listed {
                self.finding(
                    at,
                    format!("`{line}` in a decoded run: an inline suppression outside the allowlist of {SUPPRESSIONS} (fnd §7.3)"),
                );
                continue;
            }
            let count = self.used.entry(line.to_owned()).or_insert(0);
            *count = count.saturating_add(1);
            if *count > 1 {
                self.finding(
                    at,
                    format!("`{line}` is allowlisted once; this copy is outside the allowlist (fnd §7.3)"),
                );
            }
        }
    }
}

fn line_of(node: &Node) -> usize {
    match node {
        Node::Scalar(_, line) => *line,
        Node::Seq(items) => items.first().map_or(0, line_of),
        Node::Map(pairs) => pairs.first().map_or(0, |(k, _)| line_of(k)),
    }
}

/// A shellcheck directive comment (`#`, optional blanks, `shellcheck`, a blank) or a zizmor
/// ignore (`zizmor:`, optional blanks, `ignore`) anywhere on the line.
pub fn is_suppression(line: &str) -> bool {
    let shellcheck = line.match_indices('#').any(|(i, _)| {
        let rest = line
            .get(i.saturating_add(1)..)
            .unwrap_or_default()
            .trim_start();
        rest.strip_prefix("shellcheck")
            .is_some_and(|after| after.starts_with([' ', '\t']))
    });
    let zizmor = line.match_indices("zizmor:").any(|(i, _)| {
        line.get(i.saturating_add(7)..)
            .unwrap_or_default()
            .trim_start()
            .starts_with("ignore")
    });
    shellcheck || zizmor
}

fn disables_all(line: &str) -> bool {
    line.split("disable=").skip(1).any(|rest| {
        rest.split(|c: char| c.is_whitespace())
            .next()
            .unwrap_or_default()
            .split(',')
            .any(|code| code == "all")
    })
}
