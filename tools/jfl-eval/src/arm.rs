//! The arm format (TOML): one configuration of the agent that E0 compares (plan §9.2).
//!
//! ```toml
//! schema = 1
//! id = "jdtls-proxy"
//! # The Docker image: the target repository at `workdir` with a pre-warmed Gradle cache, the
//! # language server's binaries and the plugin's files. Built owner-side (ADR 0029); pin by digest.
//! image = "jfl-eval/jdtls-proxy@sha256:..."
//! workdir = "/app"                     # optional, default /app
//! setup = ["git config --global --add safe.directory /app"]   # Dockerfile RUN lines
//! plugins = ["jfl-jdtls@jfl-eval"]     # Claude Code `enabledPlugins` entries
//!
//! [marketplaces.jfl-eval]              # Claude Code `extraKnownMarketplaces`, as JSON
//! source = { source = "directory", path = "/opt/jfl-eval/marketplace" }
//!
//! [settings]                           # any other Claude Code settings, as JSON
//! permissions = { defaultMode = "bypassPermissions" }
//!
//! [env]                                # Claude Code's `env` settings block
//! ENABLE_LSP_TOOL = "1"
//! ```
//!
//! Everything the agent sees goes into one Claude Code settings file per arm, passed to Harbor's
//! `claude-code` agent as `--ak config=<file>` (Harbor 0.26.0 hands it to `claude --settings`);
//! everything the container needs is the image plus `setup`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::bounded::{MAX_TOML_FILE_BYTES, read_text};
use crate::error::{Context, EvalError, Result};
use crate::ids::Id;

/// The work directory when an arm names none (Harbor's template uses `/app`).
pub const DEFAULT_WORKDIR: &str = "/app";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawArm {
    schema: u32,
    id: String,
    image: String,
    workdir: Option<String>,
    #[serde(default)]
    setup: Vec<String>,
    #[serde(default)]
    plugins: Vec<String>,
    #[serde(default)]
    marketplaces: toml::Table,
    #[serde(default)]
    settings: toml::Table,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

/// A validated arm.
#[derive(Debug, Clone, PartialEq)]
pub struct Arm {
    /// The arm's id.
    pub id: Id,
    /// The Docker image every task of this arm starts from.
    pub image: String,
    /// The repository's directory inside the image.
    pub workdir: String,
    /// Dockerfile `RUN` lines after `FROM`.
    pub setup: Vec<String>,
    /// The Claude Code settings this arm runs with, plugins, marketplaces and env merged in.
    pub settings: Value,
}

impl Arm {
    /// Read and validate an arm file.
    pub fn load(path: &Path) -> Result<Self> {
        let text = read_text(path, MAX_TOML_FILE_BYTES)?;
        Self::parse(&text).context(format!("arm {}", path.display()))
    }

    /// Parse and validate arm TOML.
    pub fn parse(text: &str) -> Result<Self> {
        let raw: RawArm =
            toml::from_str(text).map_err(|e| crate::taskset::toml_error("not an arm", text, &e))?;
        if raw.schema != 1 {
            return Err(EvalError::new(format!(
                "schema {} (expected 1)",
                raw.schema
            )));
        }
        let id = Id::parse(&raw.id).ok_or_else(|| {
            EvalError::new("id: not [a-z0-9] words joined by single hyphens, 1 to 64 characters")
        })?;
        if !is_image(&raw.image) {
            return Err(EvalError::new(format!(
                "{id}: image: not a Docker image reference"
            )));
        }
        let workdir = raw.workdir.unwrap_or_else(|| DEFAULT_WORKDIR.to_string());
        if !is_workdir(&workdir) {
            return Err(EvalError::new(format!(
                "{id}: workdir: not an absolute path of [A-Za-z0-9._/-]"
            )));
        }
        for line in &raw.setup {
            if line.trim().is_empty() || line.contains(['\n', '\r', '\0']) {
                return Err(EvalError::new(format!(
                    "{id}: setup: each entry is one non-empty line"
                )));
            }
        }
        let settings = merge_settings(&id, raw.settings, raw.plugins, raw.marketplaces, raw.env)?;
        Ok(Self {
            id,
            image: raw.image,
            workdir,
            setup: raw.setup,
            settings,
        })
    }
}

fn merge_settings(
    id: &Id,
    settings: toml::Table,
    plugins: Vec<String>,
    marketplaces: toml::Table,
    env: BTreeMap<String, String>,
) -> Result<Value> {
    let mut out: Map<String, Value> = match to_json(id, settings)? {
        Value::Object(map) => map,
        _ => return Err(EvalError::new(format!("{id}: settings: not a table"))),
    };
    let mut put = |key: &str, value: Value| -> Result<()> {
        if out.contains_key(key) {
            return Err(EvalError::new(format!(
                "{id}: settings.{key}: set it with the arm's own field instead"
            )));
        }
        out.insert(key.to_string(), value);
        Ok(())
    };
    if !plugins.is_empty() {
        let mut enabled = Map::new();
        for plugin in plugins {
            if plugin.trim().is_empty() || plugin.chars().any(char::is_whitespace) {
                return Err(EvalError::new(format!(
                    "{id}: plugins: an entry is not a name"
                )));
            }
            enabled.insert(plugin, Value::Bool(true));
        }
        put("enabledPlugins", Value::Object(enabled))?;
    }
    if !marketplaces.is_empty() {
        put("extraKnownMarketplaces", to_json(id, marketplaces)?)?;
    }
    if !env.is_empty() {
        let mut vars = Map::new();
        for (name, value) in env {
            if !is_env_name(&name) {
                return Err(EvalError::new(format!(
                    "{id}: env: a name is not [A-Z_][A-Z0-9_]*"
                )));
            }
            vars.insert(name, Value::String(value));
        }
        put("env", Value::Object(vars))?;
    }
    Ok(Value::Object(out))
}

/// TOML to JSON, refusing what JSON cannot say plainly (a datetime).
fn to_json(id: &Id, table: toml::Table) -> Result<Value> {
    fn has_datetime(v: &toml::Value) -> bool {
        match v {
            toml::Value::Datetime(_) => true,
            toml::Value::Array(a) => a.iter().any(has_datetime),
            toml::Value::Table(t) => t.values().any(has_datetime),
            _ => false,
        }
    }
    if table.values().any(has_datetime) {
        return Err(EvalError::new(format!(
            "{id}: settings hold a TOML datetime; write it as a string"
        )));
    }
    serde_json::to_value(table).context(format!("{id}: settings"))
}

fn is_image(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._/:@-".contains(&b))
}

fn is_workdir(s: &str) -> bool {
    s.starts_with('/')
        && s.len() <= 256
        && !s.split('/').any(|part| part == "..")
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._/-".contains(&b))
}

fn is_env_name(s: &str) -> bool {
    let mut bytes = s.bytes();
    matches!(bytes.next(), Some(b) if b.is_ascii_uppercase() || b == b'_')
        && bytes.all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
schema = 1
id = "jdtls-proxy"
image = "jfl-eval/jdtls-proxy:2026-11-17"
setup = ["git config --global --add safe.directory /app"]
plugins = ["jfl-jdtls@jfl-eval"]

[marketplaces.jfl-eval]
source = { source = "directory", path = "/opt/jfl-eval/marketplace" }

[settings]
permissions = { defaultMode = "bypassPermissions" }

[env]
ENABLE_LSP_TOOL = "1"
"#;

    #[test]
    fn should_merge_plugins_marketplaces_and_env_into_the_settings_when_the_arm_is_well_formed() {
        // given
        // when
        let arm = Arm::parse(GOOD).unwrap();
        // then
        assert_eq!(arm.workdir, DEFAULT_WORKDIR);
        assert_eq!(
            arm.settings,
            serde_json::json!({
                "permissions": {"defaultMode": "bypassPermissions"},
                "enabledPlugins": {"jfl-jdtls@jfl-eval": true},
                "extraKnownMarketplaces": {
                    "jfl-eval": {"source": {"source": "directory", "path": "/opt/jfl-eval/marketplace"}}
                },
                "env": {"ENABLE_LSP_TOOL": "1"}
            })
        );
    }

    #[test]
    fn should_refuse_the_arm_when_settings_set_a_key_an_arm_field_owns() {
        // given
        let text = GOOD.replace("[settings]", "[settings]\nenabledPlugins = {}");
        // when
        let err = Arm::parse(&text).unwrap_err();
        // then
        assert!(err.to_string().contains("enabledPlugins"), "{err}");
    }

    #[test]
    fn should_refuse_the_arm_when_a_setup_line_holds_a_newline() {
        // given
        let text = GOOD.replace(
            "setup = [\"git config --global --add safe.directory /app\"]",
            "setup = [\"echo a\\nrm -rf /\"]",
        );
        // when
        let err = Arm::parse(&text).unwrap_err();
        // then
        assert!(err.to_string().contains("setup"), "{err}");
    }

    #[test]
    fn should_refuse_the_arm_when_the_workdir_climbs_out() {
        // given
        let text = GOOD.replace("schema = 1", "schema = 1\nworkdir = \"/app/../etc\"");
        // when
        let err = Arm::parse(&text).unwrap_err();
        // then
        assert!(err.to_string().contains("workdir"), "{err}");
    }
}
