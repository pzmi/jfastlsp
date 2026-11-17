//! `tools/versions.toml`: the pinned tools (XT-02, ADR 0035, fnd §8), read in its pinned shape,
//! and its agreement with mise.toml and mise.lock while mise is still the other install path
//! (until XT-08).
//!
//! The file is read line by line against a pinned shape (see its header), not with a TOML parser:
//! xtask's first build stays small (scripts/xtask.sh builds it before anything else can look),
//! and a shape refuses what a parser would accept silently (a second `url`, an inline table, a
//! dotted key, a URL that is not a GitHub release asset). Every download `install-tools` makes
//! is a URL of this file, so the URL shape is the network rule of fnd §8: a GitHub release asset,
//! never `api.github.com`. Ported in spirit from mayfly's `tools/xtask/src/tools.rs` (`parse`,
//! `is_sha256`), whose manifest has no per-platform assets.
//!
//! Pinned by the unit tests below (the shape, the repository's files) and
//! tools/xtask/tests/versions.rs (the same agreement read with a real TOML parser).

use std::collections::BTreeMap;

use regex::Regex;

use crate::tool_config;

/// The platforms of ADR 0006, as mise.lock names them.
pub(crate) const PLATFORMS: [&str; 3] = ["linux-x64", "linux-arm64", "macos-arm64"];

/// The platform this binary runs on, as mise.lock names it; `None` outside ADR 0006.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub(crate) const HOST: Option<&str> = Some("linux-x64");
/// The platform this binary runs on, as mise.lock names it; `None` outside ADR 0006.
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub(crate) const HOST: Option<&str> = Some("linux-arm64");
/// The platform this binary runs on, as mise.lock names it; `None` outside ADR 0006.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) const HOST: Option<&str> = Some("macos-arm64");
/// The platform this binary runs on, as mise.lock names it; `None` outside ADR 0006.
#[cfg(not(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "aarch64"),
    all(target_os = "macos", target_arch = "aarch64")
)))]
pub(crate) const HOST: Option<&str> = None;

/// One platform's release asset of a tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Asset {
    /// One of [`PLATFORMS`].
    pub(crate) platform: String,
    /// The GitHub release asset.
    pub(crate) url: String,
    /// Its SHA-256, 64 lowercase hex digits.
    pub(crate) sha256: String,
    /// The program's path inside the archive (relative, no `..`).
    pub(crate) bin: String,
}

/// One pinned tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tool {
    /// The table name, e.g. `cargo-nextest`.
    pub(crate) name: String,
    /// The exact version.
    pub(crate) version: String,
    /// The mise.toml key it stands for, unquoted (`just`, `aqua:…`, `github:…`).
    pub(crate) mise: String,
    /// The program and arguments that print the installed version; the program is the file name
    /// of every platform's `bin`.
    pub(crate) probe: Vec<String>,
    /// One asset per platform of [`PLATFORMS`], in that order.
    pub(crate) assets: Vec<Asset>,
}

impl Tool {
    /// The program the stages run.
    pub(crate) fn program(&self) -> &str {
        self.probe.first().map_or("", String::as_str)
    }

    /// The asset of `platform`.
    pub(crate) fn asset(&self, platform: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.platform == platform)
    }
}

/// Whether `text` is a SHA-256 as the manifest writes it: 64 lowercase hex digits.
pub(crate) fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The patterns of the shape, compiled once per parse.
struct Shape {
    blank: Regex,
    tool: Regex,
    platform: Regex,
    pair: Regex,
    probe: Regex,
    url: Regex,
    bin: Regex,
    version: Regex,
    mise: Regex,
}

impl Shape {
    fn new() -> Result<Self, String> {
        let r = |p: &str| Regex::new(p).map_err(|e| format!("versions.toml: pattern {p}: {e}"));
        let name = "[a-z][a-z0-9-]*";
        let segment = "[A-Za-z0-9._+-]+";
        Ok(Self {
            blank: r(r"^[ \t]*(#.*)?$")?,
            tool: r(&format!(r"^\[tools\.({name})\]$"))?,
            platform: r(&format!(
                r"^\[tools\.({name})\.(linux-x64|linux-arm64|macos-arm64)\]$"
            ))?,
            pair: r(r#"^([a-z0-9]+) = "([^"\\]*)"$"#)?,
            probe: r(r#"^probe = \[("[A-Za-z0-9._-]+"(, "[A-Za-z0-9._-]+")*)\]$"#)?,
            url: r(&format!(
                "^https://github\\.com/{segment}/{segment}/releases/download/{segment}/{segment}$"
            ))?,
            bin: r(&format!("^{segment}(/{segment})*$"))?,
            version: r(&format!("^{segment}$"))?,
            mise: r(
                "^(just|aqua:[A-Za-z0-9._-]+/[A-Za-z0-9._-]+(/[A-Za-z0-9._-]+)?|github:[A-Za-z0-9._-]+/[A-Za-z0-9._-]+)$",
            )?,
        })
    }
}

/// What a table has collected so far.
#[derive(Default)]
struct Fields(BTreeMap<String, String>);

impl Fields {
    fn set(&mut self, at: usize, key: &str, value: &str) -> Result<(), String> {
        if self.0.insert(key.to_owned(), value.to_owned()).is_some() {
            return Err(format!("versions.toml:{at}: `{key}` twice in one table"));
        }
        Ok(())
    }

    fn take(&mut self, at: &str, key: &str) -> Result<String, String> {
        self.0
            .remove(key)
            .ok_or_else(|| format!("versions.toml: {at} has no `{key}`"))
    }
}

/// Parses the manifest in its pinned shape: every line a blank, a comment, a table header or one
/// of the keys of its table; every tool with `version`, `mise` and `probe` and then the three
/// platforms of [`PLATFORMS`], in that order, each with `url`, `sha256` and `bin`.
pub(crate) fn parse(text: &str) -> Result<Vec<Tool>, String> {
    let shape = Shape::new()?;
    let mut tools: Vec<Tool> = Vec::new();
    // The open table: the tool, and the platform when it is a platform table.
    let mut open: Option<(Tool, Option<String>, Fields)> = None;
    for (number, line) in (1..).zip(text.split('\n')) {
        if shape.blank.is_match(line) {
            continue;
        }
        if let Some(caps) = shape.tool.captures(line) {
            if let Some(done) = open.take() {
                tools.push(close(done, &shape)?);
            }
            let name = caps.get(1).map_or("", |m| m.as_str()).to_owned();
            if tools.iter().any(|t| t.name == name) {
                return Err(format!("versions.toml:{number}: tools.{name} twice"));
            }
            open = Some((
                Tool {
                    name,
                    version: String::new(),
                    mise: String::new(),
                    probe: Vec::new(),
                    assets: Vec::new(),
                },
                None,
                Fields::default(),
            ));
            continue;
        }
        if let Some(caps) = shape.platform.captures(line) {
            let name = caps.get(1).map_or("", |m| m.as_str());
            let platform = caps.get(2).map_or("", |m| m.as_str()).to_owned();
            let Some(current) = open.take() else {
                return Err(format!(
                    "versions.toml:{number}: a platform table outside its tool"
                ));
            };
            if current.0.name != name {
                return Err(format!(
                    "versions.toml:{number}: tools.{name}.{platform} follows tools.{}",
                    current.0.name
                ));
            }
            let tool = close(current, &shape)?;
            let expected = PLATFORMS.get(tool.assets.len()).copied();
            if expected != Some(platform.as_str()) {
                return Err(format!(
                    "versions.toml:{number}: tools.{name}.{platform} out of order (the platforms are {})",
                    PLATFORMS.join(", ")
                ));
            }
            open = Some((tool, Some(platform), Fields::default()));
            continue;
        }
        let Some((_, platform, fields)) = open.as_mut() else {
            return Err(format!("versions.toml:{number}: outside a table: {line}"));
        };
        if platform.is_none()
            && let Some(caps) = shape.probe.captures(line)
        {
            let words = caps.get(1).map_or("", |m| m.as_str());
            fields.set(number, "probe", words)?;
            continue;
        }
        let Some(caps) = shape.pair.captures(line) else {
            return Err(format!(
                "versions.toml:{number}: outside the pinned shape: {line}"
            ));
        };
        let key = caps.get(1).map_or("", |m| m.as_str());
        let value = caps.get(2).map_or("", |m| m.as_str());
        let allowed: &[&str] = if platform.is_some() {
            &["url", "sha256", "bin"]
        } else {
            &["version", "mise"]
        };
        if !allowed.contains(&key) {
            return Err(format!(
                "versions.toml:{number}: `{key}` is not a key of this table"
            ));
        }
        fields.set(number, key, value)?;
    }
    if let Some(done) = open.take() {
        tools.push(close(done, &shape)?);
    }
    if let Some(short) = tools.iter().find(|t| t.assets.len() != PLATFORMS.len()) {
        return Err(format!(
            "versions.toml: tools.{} pins {} of the platforms {}",
            short.name,
            short.assets.len(),
            PLATFORMS.join(", ")
        ));
    }
    if tools.is_empty() {
        return Err("versions.toml: no tool".to_owned());
    }
    Ok(tools)
}

/// Closes a table: moves its fields into the tool, checking each value's shape.
fn close(open: (Tool, Option<String>, Fields), shape: &Shape) -> Result<Tool, String> {
    let (mut tool, platform, mut fields) = open;
    match platform {
        None => {
            let at = format!("tools.{}", tool.name);
            tool.version = fields.take(&at, "version")?;
            tool.mise = fields.take(&at, "mise")?;
            let probe = fields.take(&at, "probe")?;
            tool.probe = probe
                .split(", ")
                .map(|w| w.trim_matches('"').to_owned())
                .collect();
            if !shape.version.is_match(&tool.version) {
                return Err(format!(
                    "versions.toml: {at}.version is not a plain version"
                ));
            }
            if !shape.mise.is_match(&tool.mise) {
                return Err(format!(
                    "versions.toml: {at}.mise is not a mise.toml key (just, aqua:, github:)"
                ));
            }
        }
        Some(platform) => {
            let at = format!("tools.{}.{platform}", tool.name);
            let asset = Asset {
                url: fields.take(&at, "url")?,
                sha256: fields.take(&at, "sha256")?,
                bin: fields.take(&at, "bin")?,
                platform,
            };
            if !shape.url.is_match(&asset.url) {
                return Err(format!(
                    "versions.toml: {at}.url is not a GitHub release asset (https://github.com/<owner>/<repo>/releases/download/<tag>/<asset>; fnd §8)"
                ));
            }
            if !is_sha256(&asset.sha256) {
                return Err(format!(
                    "versions.toml: {at}.sha256 is not 64 lowercase hex digits"
                ));
            }
            let program = asset.bin.rsplit('/').next().unwrap_or_default();
            if !shape.bin.is_match(&asset.bin)
                || asset.bin.split('/').any(|s| s == "." || s == "..")
                || program != tool.program()
            {
                return Err(format!(
                    "versions.toml: {at}.bin must be a relative path inside the archive ending in the probe's program `{}`",
                    tool.program()
                ));
            }
            tool.assets.push(asset);
        }
    }
    Ok(tool)
}

/// One platform entry of mise.lock: (version, url, sha256).
type LockEntry = (String, String, String);

/// mise.lock's entries by (tool key, platform), read after its shape passed
/// [`tool_config::mise_lock_shape`].
fn lock_entries(text: &str) -> Result<BTreeMap<(String, String), LockEntry>, String> {
    if let Some(line) = tool_config::mise_lock_shape(text) {
        return Err(format!("mise.lock: outside its shape at {line}"));
    }
    let unquote = |s: &str| s.trim().trim_matches('"').to_owned();
    let mut out = BTreeMap::new();
    let mut key = String::new();
    let mut version = String::new();
    let mut platform: Option<String> = None;
    let mut fields: (String, String) = (String::new(), String::new());
    let mut flush =
        |platform: &mut Option<String>, key: &str, version: &str, fields: &mut (String, String)| {
            if let Some(p) = platform.take() {
                out.insert(
                    (key.to_owned(), p),
                    (
                        version.to_owned(),
                        std::mem::take(&mut fields.0),
                        std::mem::take(&mut fields.1),
                    ),
                );
            }
        };
    for line in text.lines() {
        let line = line.split(" #").next().unwrap_or_default().trim_end();
        if let Some(inner) = line
            .strip_prefix("[[tools.")
            .and_then(|l| l.strip_suffix("]]"))
        {
            flush(&mut platform, &key, &version, &mut fields);
            key = unquote(inner);
            version.clear();
        } else if let Some(inner) = line
            .strip_prefix("[tools.")
            .and_then(|l| l.strip_suffix(']'))
        {
            flush(&mut platform, &key, &version, &mut fields);
            let p = inner
                .rsplit_once(".\"platforms.")
                .map(|(_, p)| p.trim_end_matches('"').to_owned());
            platform = p;
        } else if let Some(v) = line.strip_prefix("version = ") {
            version = unquote(v);
        } else if let Some(v) = line.strip_prefix("url = ") {
            fields.0 = unquote(v);
        } else if let Some(v) = line.strip_prefix("checksum = ") {
            fields.1 = unquote(v).trim_start_matches("sha256:").to_owned();
        }
    }
    flush(&mut platform, &key, &version, &mut fields);
    Ok(out)
}

/// mise.toml's tools by key, read after its shape passed [`tool_config::mise_toml_shape`].
fn mise_toml_entries(text: &str) -> Result<BTreeMap<String, String>, String> {
    if let Some(line) = tool_config::mise_toml_shape(text) {
        return Err(format!("mise.toml: outside its shape at {line}"));
    }
    Ok(text
        .lines()
        .map(|l| l.split('#').next().unwrap_or_default().trim())
        .filter(|l| !l.is_empty() && *l != "[tools]")
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| {
            (
                k.trim().trim_matches('"').to_owned(),
                v.trim().trim_matches('"').to_owned(),
            )
        })
        .collect())
}

/// Every disagreement between the manifest and mise.toml and mise.lock: a tool or a platform one
/// of them lacks, or a version, URL or SHA-256 that differs. Empty when they agree.
pub(crate) fn disagreements(tools: &[Tool], mise_toml: &str, mise_lock: &str) -> Vec<String> {
    let (pins, lock) = match (mise_toml_entries(mise_toml), lock_entries(mise_lock)) {
        (Ok(pins), Ok(lock)) => (pins, lock),
        (Err(e), _) | (_, Err(e)) => return vec![e],
    };
    let mut out = Vec::new();
    for tool in tools {
        match pins.get(&tool.mise) {
            None => out.push(format!(
                "tools.{} stands for {}, which mise.toml does not pin",
                tool.name, tool.mise
            )),
            Some(v) if *v != tool.version => out.push(format!(
                "tools.{} is {}, mise.toml pins {v}",
                tool.name, tool.version
            )),
            Some(_) => {}
        }
        for asset in &tool.assets {
            let at = format!("tools.{}.{}", tool.name, asset.platform);
            match lock.get(&(tool.mise.clone(), asset.platform.clone())) {
                None => out.push(format!("{at}: mise.lock has no such entry")),
                Some((version, url, sha256)) => {
                    if *version != tool.version {
                        out.push(format!(
                            "{at}: version {}, mise.lock {version}",
                            tool.version
                        ));
                    }
                    if *url != asset.url {
                        out.push(format!("{at}: url {}, mise.lock {url}", asset.url));
                    }
                    if *sha256 != asset.sha256 {
                        out.push(format!("{at}: sha256 {}, mise.lock {sha256}", asset.sha256));
                    }
                }
            }
        }
    }
    for key in pins.keys() {
        if !tools.iter().any(|t| t.mise == *key) {
            out.push(format!(
                "mise.toml pins {key}, which tools/versions.toml lacks"
            ));
        }
    }
    for (key, platform) in lock.keys() {
        if !tools
            .iter()
            .any(|t| t.mise == *key && t.asset(platform).is_some())
        {
            out.push(format!(
                "mise.lock has {key} for {platform}, which tools/versions.toml lacks"
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(file: &str) -> String {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        std::fs::read_to_string(root.join(file)).unwrap_or_default()
    }

    const ONE: &str = r#"# a comment
[tools.x]
version = "1.2.3"
mise = "aqua:o/x"
probe = ["x", "--version"]

[tools.x.linux-x64]
url = "https://github.com/o/x/releases/download/v1.2.3/x-linux.tar.gz"
sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
bin = "x-1.2.3/x"

[tools.x.linux-arm64]
url = "https://github.com/o/x/releases/download/v1.2.3/x-arm.tar.gz"
sha256 = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
bin = "x"

[tools.x.macos-arm64]
url = "https://github.com/o/x/releases/download/v1.2.3/x-mac.tar.gz"
sha256 = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
bin = "x"
"#;

    const ONE_TOML: &str = "[tools]\n\"aqua:o/x\" = \"1.2.3\"\n";

    const ONE_LOCK: &str = r#"lockfile_version = 3

[[tools."aqua:o/x"]]
version = "1.2.3"
backend = "aqua:o/x"
specifiers = ["1.2.3"]

[tools."aqua:o/x"."platforms.linux-arm64"]
checksum = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
url = "https://github.com/o/x/releases/download/v1.2.3/x-arm.tar.gz"

[tools."aqua:o/x"."platforms.linux-x64"]
checksum = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
url = "https://github.com/o/x/releases/download/v1.2.3/x-linux.tar.gz"
url_api = "https://api.github.com/repos/o/x/releases/assets/1"

[tools."aqua:o/x"."platforms.macos-arm64"]
checksum = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
url = "https://github.com/o/x/releases/download/v1.2.3/x-mac.tar.gz"
"#;

    #[test]
    fn should_read_every_tool_of_the_repository_when_versions_toml_is_parsed() {
        // given
        let text = read("tools/versions.toml");

        // when
        let tools = parse(&text);

        // then
        let tools = tools.unwrap_or_default();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "just",
                "cargo-nextest",
                "cargo-deny",
                "adrs",
                "gitleaks",
                "actionlint",
                "zizmor",
                "pinact",
                "shellcheck",
                "hyperfine",
                "ripgrep"
            ]
        );
        assert!(tools.iter().all(|t| t.assets.len() == PLATFORMS.len()));
    }

    #[test]
    fn should_agree_with_mise_when_the_repository_s_pins_are_read() {
        // given
        let tools = parse(&read("tools/versions.toml")).unwrap_or_default();

        // when
        let found = disagreements(&tools, &read("mise.toml"), &read("mise.lock"));

        // then
        assert!(!tools.is_empty());
        assert_eq!(found, Vec::<String>::new());
    }

    #[test]
    fn should_pin_the_bootstrap_s_just_when_versions_toml_names_it() {
        // given: scripts/bootstrap-tools.sh installs just before anything else can (fnd §8 rule 0)
        let script = read("scripts/bootstrap-tools.sh");
        let tools = parse(&read("tools/versions.toml")).unwrap_or_default();
        let just = tools.iter().find(|t| t.name == "just");

        // when
        let pinned = script
            .lines()
            .find_map(|l| l.strip_prefix("JUST_VERSION=\"")?.strip_suffix('"'));

        // then: the same version, and each platform's digest appears in the script
        assert_eq!(pinned, just.map(|t| t.version.as_str()));
        for asset in just.map(|t| t.assets.as_slice()).unwrap_or_default() {
            assert!(
                script.contains(&format!("just_sha=\"{}\"", asset.sha256)),
                "{}",
                asset.platform
            );
        }
    }

    #[test]
    fn should_parse_a_tool_when_the_shape_holds() {
        // given
        let text = ONE;

        // when
        let tools = parse(text);

        // then
        let tools = tools.unwrap_or_default();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools.first().map(Tool::program), Some("x"));
        assert_eq!(
            disagreements(&tools, ONE_TOML, ONE_LOCK),
            Vec::<String>::new()
        );
    }

    #[test]
    fn should_refuse_the_manifest_when_a_line_leaves_the_shape() {
        // given: each edit of a valid manifest, and what the refusal names
        let edits: [(&str, &str, &str); 12] = [
            (
                "https://github.com/o/x/releases/download/v1.2.3/x-linux.tar.gz",
                "https://api.github.com/repos/o/x/releases/assets/1",
                "GitHub release asset",
            ),
            (
                "https://github.com/o/x/releases/download/v1.2.3/x-linux.tar.gz",
                "http://github.com/o/x/releases/download/v1.2.3/x-linux.tar.gz",
                "GitHub release asset",
            ),
            (
                "https://github.com/o/x/releases/download/v1.2.3/x-linux.tar.gz",
                "https://github.com/o/x/releases/download/v1.2.3/../../../x",
                "GitHub release asset",
            ),
            (
                "https://github.com/o/x/releases/download/v1.2.3/x-linux.tar.gz",
                "https://github.com.evil.example/o/x/releases/download/v1/x",
                "GitHub release asset",
            ),
            ("sha256 = \"aaaa", "sha256 = \"AAAA", "64 lowercase"),
            ("bin = \"x-1.2.3/x\"", "bin = \"../x\"", "relative path"),
            (
                "bin = \"x-1.2.3/x\"",
                "bin = \"x-1.2.3/y\"",
                "relative path",
            ),
            (
                "version = \"1.2.3\"",
                "version = \"1.2.3\"\nversion = \"2\"",
                "twice",
            ),
            ("mise = \"aqua:o/x\"", "mise = \"cargo:x\"", "mise.toml key"),
            (
                "[tools.x.macos-arm64]",
                "[tools.x.linux-x64]",
                "out of order",
            ),
            ("bin = \"x\"\n", "bin = \"x\"\nrun = \"sh\"\n", "not a key"),
            ("# a comment", "x = { url = \"u\" }", "outside a table"),
        ];

        // when
        let results: Vec<_> = edits
            .iter()
            .map(|(from, to, _)| parse(&ONE.replacen(from, to, 1)))
            .collect();

        // then
        for ((from, to, why), result) in edits.iter().zip(&results) {
            let refusal = result.as_ref().err().cloned().unwrap_or_default();
            assert!(refusal.contains(why), "{from} -> {to}: {result:?}");
        }
    }

    #[test]
    fn should_refuse_the_manifest_when_a_platform_is_missing() {
        // given
        let short = ONE
            .split("[tools.x.macos-arm64]")
            .next()
            .unwrap_or_default();

        // when
        let result = parse(short);

        // then
        assert!(result.is_err_and(|e| e.contains("pins 2 of the platforms")));
    }

    #[test]
    fn should_name_each_disagreement_when_mise_lock_differs() {
        // given: a lock with another SHA-256, URL and version, and a mise.toml with another tool
        let tools = parse(ONE).unwrap_or_default();
        let lock = ONE_LOCK
            .replacen("sha256:bbbb", "sha256:dddd", 1)
            .replacen("x-mac.tar.gz", "x-mac2.tar.gz", 1)
            .replacen("version = \"1.2.3\"", "version = \"1.2.4\"", 1);
        let toml = format!("{ONE_TOML}\"aqua:o/y\" = \"1\"\n");

        // when
        let found = disagreements(&tools, &toml, &lock);

        // then
        let all = found.join("\n");
        for needle in [
            "tools.x.linux-arm64: sha256",
            "tools.x.macos-arm64: url",
            "tools.x.linux-x64: version 1.2.3, mise.lock 1.2.4",
            "mise.toml pins aqua:o/y",
        ] {
            assert!(all.contains(needle), "{needle} missing in:\n{all}");
        }
    }

    #[test]
    fn should_name_the_missing_entry_when_mise_lock_lacks_a_platform() {
        // given
        let tools = parse(ONE).unwrap_or_default();
        let lock = ONE_LOCK
            .split("[tools.\"aqua:o/x\".\"platforms.macos-arm64\"]")
            .next()
            .unwrap_or_default();

        // when
        let found = disagreements(&tools, ONE_TOML, lock);

        // then
        assert_eq!(
            found,
            ["tools.x.macos-arm64: mise.lock has no such entry".to_owned()]
        );
    }
}
