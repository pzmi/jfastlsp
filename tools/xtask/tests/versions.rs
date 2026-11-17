//! `tools/versions.toml` against mise.toml and mise.lock, read with a real TOML parser (the `toml`
//! crate), independently of xtask's own pinned-shape reader (tools/xtask/src/versions.rs): until
//! XT-08 removes mise, both install paths must fetch the same assets (XT-02). A version bump that
//! changes one file without the others fails here and in `doctor`.

use std::collections::BTreeMap;
use std::error::Error;
use std::path::Path;

type TestResult = Result<(), Box<dyn Error>>;

/// (version, url, sha256) by (mise key, platform).
type Pins = BTreeMap<(String, String), (String, String, String)>;

fn read(file: &str) -> Result<toml::Table, Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    Ok(std::fs::read_to_string(root.join(file))?.parse()?)
}

fn text(value: Option<&toml::Value>) -> String {
    value
        .and_then(toml::Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// versions.toml's pins, and its tools' mise keys with their versions.
fn manifest_pins(table: &toml::Table) -> (Pins, BTreeMap<String, String>) {
    let mut pins = Pins::new();
    let mut keys = BTreeMap::new();
    let tools = table.get("tools").and_then(toml::Value::as_table);
    for tool in tools.into_iter().flat_map(|t| t.values()) {
        let key = text(tool.get("mise"));
        let version = text(tool.get("version"));
        keys.insert(key.clone(), version.clone());
        for platform in ["linux-x64", "linux-arm64", "macos-arm64"] {
            if let Some(asset) = tool.get(platform) {
                pins.insert(
                    (key.clone(), platform.to_owned()),
                    (
                        version.clone(),
                        text(asset.get("url")),
                        text(asset.get("sha256")),
                    ),
                );
            }
        }
    }
    (pins, keys)
}

/// mise.lock's pins.
fn lock_pins(table: &toml::Table) -> Pins {
    let mut pins = Pins::new();
    let tools = table.get("tools").and_then(toml::Value::as_table);
    for (key, entries) in tools.into_iter().flatten() {
        for entry in entries.as_array().into_iter().flatten() {
            let version = text(entry.get("version"));
            for (name, asset) in entry.as_table().into_iter().flatten() {
                let Some(platform) = name.strip_prefix("platforms.") else {
                    continue;
                };
                let checksum = text(asset.get("checksum"));
                pins.insert(
                    (key.clone(), platform.to_owned()),
                    (
                        version.clone(),
                        text(asset.get("url")),
                        checksum.trim_start_matches("sha256:").to_owned(),
                    ),
                );
            }
        }
    }
    pins
}

/// mise.toml's tools and versions.
fn mise_keys(table: &toml::Table) -> BTreeMap<String, String> {
    table
        .get("tools")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten()
        .map(|(k, v)| (k.clone(), text(Some(v))))
        .collect()
}

#[test]
fn should_pin_the_assets_of_mise_lock_when_versions_toml_is_read_with_a_toml_parser() -> TestResult
{
    // given
    let (pins, keys) = manifest_pins(&read("tools/versions.toml")?);

    // when
    let lock = lock_pins(&read("mise.lock")?);
    let mise = mise_keys(&read("mise.toml")?);

    // then: every tool, platform, version, URL and SHA-256 the same, and nothing extra
    assert_eq!(pins.len(), 33, "11 tools on 3 platforms");
    assert_eq!(pins, lock);
    assert_eq!(keys, mise);
    Ok(())
}

#[test]
fn should_see_a_difference_when_mise_lock_moves_alone() -> TestResult {
    // given: the repository's mise.lock with one checksum changed, as a bump of mise.lock alone
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let lock_text = std::fs::read_to_string(root.join("mise.lock"))?;
    let first = lock_text
        .split("checksum = \"sha256:")
        .nth(1)
        .and_then(|rest| rest.get(..64))
        .ok_or("no checksum in mise.lock")?;
    let moved: toml::Table = lock_text.replacen(first, &"0".repeat(64), 1).parse()?;
    let (pins, _) = manifest_pins(&read("tools/versions.toml")?);

    // when
    let lock = lock_pins(&moved);

    // then
    assert_ne!(pins, lock);
    Ok(())
}
