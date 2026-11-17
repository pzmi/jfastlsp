//! The pinned tools at work (XT-02, ADR 0035, fnd §8): where `install-tools` puts them, the PATH
//! entries the stages run them from, `doctor` and `install-tools`. Started from mayfly's
//! `tools/xtask/src/tools.rs` (`doctor`, `install`, the probe and its version match) and
//! `apalache.rs` (download, verify, unpack, move into place), adapted: every tool here is a
//! checksummed release asset of `tools/versions.toml`, installed per version and platform.
//!
//! - **The cache** is `$XDG_CACHE_HOME/jfastlsp/tools`, else `$HOME/.cache/jfastlsp/tools`: outside
//!   any checkout (one inside the repository is refused), shared by every worktree. A tool lives in
//!   `<cache>/<name>/<version>/<platform>/bin/<program>`, holding the one program of its archive,
//!   so the stages' PATH entry exposes nothing else, and a version bump is a new directory. A
//!   `kind = "tree"` tool (Node, the JDK, Apalache; FORMAL-01) keeps its whole archive in
//!   `<cache>/<name>/<version>/<platform>/tree/`, and the directory that holds its program is the
//!   PATH entry; a `kind = "npm"` tool (Quint) is refused, naming `mise install --locked`, until
//!   XT-08.
//! - **`install-tools [--archives DIR] [TOOL...]`** downloads each asset (`curl --proto =https`,
//!   from a URL whose shape `versions.rs` pins: a GitHub release asset, never `api.github.com`),
//!   or takes it from DIR, checks its SHA-256 in this process (no hashing program from PATH), and
//!   refuses a mismatch with both digests and installs nothing; only then unpacks it (`tar` from
//!   `/usr/bin:/bin`, not the caller's PATH) into a staging directory and moves the program into
//!   place with one rename. A tool already present at its pinned version is left alone.
//! - **`doctor`** fails when versions.toml and mise.lock (or mise.toml) disagree, then resolves
//!   every pinned program on the PATH the stages get (the cache first, then `mise bin-paths`,
//!   mise's shims, the inherited PATH: [`crate::stage_path`]) and runs its probe: one line per tool,
//!   `ok`, `missing` or `wrong`, with where it was found and the fix; exit 1 on any problem. A tool
//!   with `only` (the JDK and Apalache, which only `formal-verify` needs) is reported as `ok` or
//!   `absent` (optional) and never fails it; `formal-verify` probes them itself (FORMAL-01).
//!
//! Pinned by: tools/xtask/tests/tools.rs and the unit tests below.

use std::ffi::OsStr;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest as _, Sha256};

use crate::versions::{self, Kind, Tool};

/// The PATH of the programs this module starts itself (`curl`, `tar`, and `xz` under it): the
/// system's, never the caller's, where a planted one could replace a verified archive's content.
const SYSTEM_PATH: &str = "/usr/bin:/bin";

/// The fix `doctor` and a failed stage name.
pub(crate) const INSTALL_HINT: &str =
    "run 'bash scripts/xtask.sh install-tools' (or 'mise install --locked')";

/// The fix for `tool`: [`INSTALL_HINT`], or mise alone for an npm tool, which `install-tools`
/// does not install (until XT-08).
pub(crate) fn hint(tool: &Tool) -> &'static str {
    match tool.kind {
        Kind::Npm => NPM_HINT,
        Kind::Program | Kind::Tree => INSTALL_HINT,
    }
}

/// The fix for a missing or wrong tool that only `formal-verify` needs (the JDK, Apalache).
pub(crate) const JVM_HINT: &str = "for it run 'bash scripts/bootstrap-tools.sh --formal-verify' (or 'mise install --locked java github:apalache-mc/apalache')";

/// The fix for a missing npm tool (Quint).
pub(crate) const NPM_HINT: &str =
    "run 'mise install --locked' (or 'bash scripts/bootstrap-tools.sh')";

/// The tool cache: `$XDG_CACHE_HOME/jfastlsp/tools`, else `$HOME/.cache/jfastlsp/tools`; an
/// absolute path only.
pub(crate) fn cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|h| PathBuf::from(h).join(".cache"))
                .filter(|p| p.is_absolute())
        })?;
    Some(base.join("jfastlsp").join("tools"))
}

/// Where `tool` is installed for `platform` in `cache`.
pub(crate) fn install_dir(cache: &Path, tool: &Tool, platform: &str) -> PathBuf {
    cache.join(&tool.name).join(&tool.version).join(platform)
}

/// The directory of an installed tool that holds its program: `bin` for one program, the
/// directory of the asset's `bin` inside `tree` for a whole archive; `None` for an npm tool.
pub(crate) fn program_dir(cache: &Path, tool: &Tool, platform: &str) -> Option<PathBuf> {
    let dir = install_dir(cache, tool, platform);
    match tool.kind {
        Kind::Program => Some(dir.join("bin")),
        Kind::Tree => {
            let bin = Path::new(&tool.asset(platform)?.bin);
            Some(dir.join("tree").join(bin.parent()?))
        }
        Kind::Npm => None,
    }
}

/// The PATH entries of the installed tools: each tool's directory that holds its program, for
/// this platform, in manifest order.
pub(crate) fn path_dirs(cache: Option<&Path>, tools: &[Tool]) -> Vec<PathBuf> {
    let (Some(cache), Some(platform)) = (cache, versions::HOST) else {
        return Vec::new();
    };
    tools
        .iter()
        .filter_map(|t| Some((program_dir(cache, t, platform)?, t)))
        .filter(|(dir, t)| dir.join(t.program()).is_file())
        .map(|(dir, _)| dir)
        .collect()
}

/// Reads `ROOT/tools/versions.toml`.
pub(crate) fn manifest(root: &Path) -> Result<Vec<Tool>, String> {
    let file = root.join("tools/versions.toml");
    let text = std::fs::read_to_string(&file)
        .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    versions::parse(&text)
}

/// What a probe found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Found {
    /// The pinned version, at this path.
    Ok(PathBuf),
    /// Not on the PATH, or its probe failed to run.
    Missing,
    /// Another version: the path and the probe's first line.
    Wrong(PathBuf, String),
}

/// Whether `output` names `version` as a whitespace-separated word (a leading `v` ignored); the
/// word is the tool's `prints` when it has one.
pub(crate) fn names_version(output: &str, version: &str) -> bool {
    output
        .split_whitespace()
        .any(|word| word.strip_prefix('v').unwrap_or(word) == version)
}

/// Finds `program` in the directories of `path`.
pub(crate) fn find(program: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path?)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

/// Resolves the tool's program on `path` and runs its probe there.
pub(crate) fn probe(tool: &Tool, path: Option<&OsStr>) -> Found {
    let Some(program) = find(tool.program(), path) else {
        return Found::Missing;
    };
    let mut command = Command::new(&program);
    command.args(tool.probe.iter().skip(1)).stdin(Stdio::null());
    if let Some(path) = path {
        command.env("PATH", path);
    }
    let Ok(output) = command.output() else {
        return Found::Missing;
    };
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() && names_version(&text, tool.expected_word()) {
        Found::Ok(program)
    } else {
        let first = text.lines().next().unwrap_or_default().trim().to_owned();
        Found::Wrong(program, first)
    }
}

/// `doctor`: the agreement with mise.toml and mise.lock, then every pinned tool on `path`. Prints
/// one line per finding; returns the exit code (1 on any problem).
pub(crate) fn doctor(root: &Path, tools: &[Tool], path: Option<&OsStr>) -> i32 {
    let mut bad: u32 = 0;
    let read = |name: &str| std::fs::read_to_string(root.join(name)).unwrap_or_default();
    for problem in versions::disagreements(tools, &read("mise.toml"), &read("mise.lock")) {
        eprintln!(
            "doctor: tools/versions.toml and mise disagree: {problem}; change versions.toml, mise.toml and mise.lock in one commit"
        );
        bad = bad.saturating_add(1);
    }
    for tool in tools {
        let head = format!("{} {}", tool.name, tool.version);
        if let Some(command) = &tool.only {
            match probe(tool, path) {
                Found::Ok(at) => println!("ok      {head} ({})", at.display()),
                Found::Missing => {
                    println!("absent  {head} (optional: only `{command}` needs it; {JVM_HINT})")
                }
                Found::Wrong(at, said) => println!(
                    "absent  {head} (optional: only `{command}` needs it; {} says '{said}'; {JVM_HINT})",
                    at.display()
                ),
            }
            continue;
        }
        match probe(tool, path) {
            Found::Ok(at) => println!("ok      {head} ({})", at.display()),
            Found::Missing => {
                eprintln!(
                    "missing {head}: `{}` is not on the stages' PATH: {}",
                    tool.program(),
                    hint(tool)
                );
                bad = bad.saturating_add(1);
            }
            Found::Wrong(at, said) => {
                eprintln!(
                    "wrong   {head}: {} says '{said}': {}",
                    at.display(),
                    hint(tool)
                );
                bad = bad.saturating_add(1);
            }
        }
    }
    if bad > 0 {
        eprintln!("doctor: {bad} problem(s) with the pinned tools (fnd §8)");
        1
    } else {
        0
    }
}

/// The SHA-256 of a file, as lowercase hex, computed in this process.
pub(crate) fn sha256_of(file: &Path) -> Result<String, String> {
    let mut reader =
        std::fs::File::open(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1 << 16];
    loop {
        let n = reader
            .read(&mut buffer)
            .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
        let Some(chunk) = buffer.get(..n) else {
            break;
        };
        if chunk.is_empty() {
            break;
        }
        hasher.update(chunk);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Where an install gets its archives: downloaded, or a directory holding each asset by its file
/// name.
#[derive(Debug, Clone)]
pub(crate) enum Source {
    /// `curl` from the asset's URL.
    Download,
    /// `DIR/<the URL's last segment>`.
    Archives(PathBuf),
}

/// `install-tools`: installs the named tools (all when none is named) into `cache` for this
/// platform. Returns the exit code: 0 all installed, 1 a failure, 2 an unknown tool.
pub(crate) fn install(
    root: &Path,
    tools: &[Tool],
    names: &[String],
    cache: Option<&Path>,
    source: &Source,
) -> i32 {
    let Some(platform) = versions::HOST else {
        eprintln!("install-tools: this platform is not one of ADR 0006's");
        return 1;
    };
    let Some(cache) = cache else {
        eprintln!("install-tools: neither XDG_CACHE_HOME nor HOME names an absolute directory");
        return 1;
    };
    if inside(cache, root) {
        eprintln!(
            "install-tools: the tool cache {} is inside the repository {}; set XDG_CACHE_HOME elsewhere",
            cache.display(),
            root.display()
        );
        return 1;
    }
    let mut selected = Vec::new();
    for name in names {
        match tools.iter().find(|t| &t.name == name) {
            Some(tool) => selected.push(tool),
            None => {
                eprintln!("install-tools: unknown tool {name}: see tools/versions.toml");
                return 2;
            }
        }
    }
    if names.is_empty() {
        selected = tools.iter().collect();
    }
    let mut failed: u32 = 0;
    for tool in selected {
        match install_one(tool, tools, platform, cache, source) {
            Ok(message) => println!("{message}"),
            Err(message) => {
                eprintln!("install-tools: {message}");
                failed = failed.saturating_add(1);
            }
        }
    }
    i32::from(failed > 0)
}

/// Whether `dir` is `root` or below it, compared by canonical paths where they exist.
fn inside(dir: &Path, root: &Path) -> bool {
    let canonical = |p: &Path| {
        p.ancestors()
            .find_map(|a| std::fs::canonicalize(a).ok().map(|c| (a.to_path_buf(), c)))
            .map_or_else(
                || p.to_path_buf(),
                |(a, c)| c.join(p.strip_prefix(&a).unwrap_or(Path::new(""))),
            )
    };
    canonical(dir).starts_with(canonical(root))
}

/// The PATH a freshly installed tool's probe runs with: its own directory first, then the other
/// installed tools of the cache and the inherited PATH, so a program that starts another (Apalache
/// starts `java`) finds it.
fn probe_path(own: &Path, cache: &Path, tools: &[Tool]) -> std::ffi::OsString {
    let inherited = std::env::var_os("PATH");
    let dirs = std::iter::once(own.to_path_buf())
        .chain(path_dirs(Some(cache), tools))
        .chain(inherited.iter().flat_map(std::env::split_paths));
    std::env::join_paths(dirs).unwrap_or_else(|_| own.as_os_str().to_owned())
}

/// Installs one tool; the message says what happened.
fn install_one(
    tool: &Tool,
    tools: &[Tool],
    platform: &str,
    cache: &Path,
    source: &Source,
) -> Result<String, String> {
    let head = format!("{} {}", tool.name, tool.version);
    if tool.kind == Kind::Npm {
        return Err(format!(
            "{head}: an npm tool, installed from its locked graph by 'mise install --locked' until XT-08 gives xtask an npm install"
        ));
    }
    let asset = tool
        .asset(platform)
        .ok_or_else(|| format!("{} pins no asset for {platform}", tool.name))?;
    let dir = install_dir(cache, tool, platform);
    let bin_dir = program_dir(cache, tool, platform)
        .ok_or_else(|| format!("{head}: no program directory for {platform}"))?;
    if bin_dir.join(tool.program()).is_file()
        && let Found::Ok(_) = probe(tool, Some(&probe_path(&bin_dir, cache, tools)))
    {
        return Ok(format!("ok {head} (already installed)"));
    }
    let parent = dir
        .parent()
        .ok_or_else(|| format!("{} has no parent", dir.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    let work = parent.join(format!(".install-{platform}-{}", std::process::id()));
    remove_dir(&work)?;
    std::fs::create_dir_all(&work).map_err(|e| format!("cannot create {}: {e}", work.display()))?;
    let result = fetch_verify_unpack(tool, asset, &work, &dir, source);
    let cleaned = remove_dir(&work);
    result?;
    cleaned?;
    match probe(tool, Some(&probe_path(&bin_dir, cache, tools))) {
        Found::Ok(at) => Ok(format!("ok {head} (installed: {})", at.display())),
        Found::Missing => Err(format!("installed {head}, but it does not run")),
        Found::Wrong(_, said) => Err(format!("installed {head}, but it says '{said}'")),
    }
}

/// Fetches the asset into `work`, refuses it unless its SHA-256 is the pinned one, unpacks it and
/// moves its program into `dir/bin` (a `tree` tool: the whole unpacked archive into `dir/tree`)
/// with one rename of `dir`.
fn fetch_verify_unpack(
    tool: &Tool,
    asset: &versions::Asset,
    work: &Path,
    dir: &Path,
    source: &Source,
) -> Result<(), String> {
    let file = asset.url.rsplit('/').next().unwrap_or("asset");
    let archive = work.join(file);
    match source {
        Source::Download => download(&asset.url, &archive)?,
        Source::Archives(from) => {
            let given = from.join(file);
            std::fs::copy(&given, &archive)
                .map_err(|e| format!("cannot read {}: {e}", given.display()))?;
        }
    }
    let found = sha256_of(&archive)?;
    if found != asset.sha256 {
        return Err(format!(
            "{} {}: {file} has SHA-256 {found}, but tools/versions.toml pins {}; refused, nothing installed",
            tool.name, tool.version, asset.sha256
        ));
    }
    let unpacked = work.join("unpacked");
    std::fs::create_dir_all(&unpacked)
        .map_err(|e| format!("cannot create {}: {e}", unpacked.display()))?;
    let status = Command::new("tar")
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(&unpacked)
        .env("PATH", SYSTEM_PATH)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| format!("cannot run tar: {e}"))?;
    let program = unpacked.join(&asset.bin);
    if !status.success() || !program.is_file() || program.is_symlink() {
        return Err(format!(
            "{file} did not unpack to the file {} (tar: {status})",
            asset.bin
        ));
    }
    let staged = work.join("staged");
    if tool.kind == Kind::Tree {
        std::fs::create_dir_all(&staged)
            .map_err(|e| format!("cannot create {}: {e}", staged.display()))?;
        std::fs::rename(&unpacked, staged.join("tree"))
            .map_err(|e| format!("cannot move the unpacked {file}: {e}"))?;
    } else {
        std::fs::create_dir_all(staged.join("bin"))
            .map_err(|e| format!("cannot create {}: {e}", staged.display()))?;
        let placed = staged.join("bin").join(tool.program());
        std::fs::rename(&program, &placed)
            .map_err(|e| format!("cannot move {}: {e}", asset.bin))?;
        make_executable(&placed)?;
    }
    remove_dir(dir)?;
    std::fs::rename(&staged, dir)
        .map_err(|e| format!("cannot move {} into {}: {e}", tool.name, dir.display()))
}

#[cfg(unix)]
fn make_executable(file: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("cannot make {} executable: {e}", file.display()))
}

#[cfg(not(unix))]
fn make_executable(_file: &Path) -> Result<(), String> {
    Ok(())
}

/// Downloads `url` to `dest` over HTTPS only (redirects too), with the system's curl.
fn download(url: &str, dest: &Path) -> Result<(), String> {
    eprintln!("install-tools: fetching {url}");
    let status = Command::new("curl")
        .args(["--fail", "--silent", "--show-error", "--location"])
        .args([
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--retry",
            "3",
        ])
        .arg("--output")
        .arg(dest)
        .arg(url)
        .env("PATH", SYSTEM_PATH)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| format!("cannot run curl: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "cannot download {url} ({status}); GitHub release downloads must be reachable"
        ))
    }
}

fn remove_dir(path: &Path) -> Result<(), String> {
    match std::fs::remove_dir_all(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(format!("cannot remove {}: {e}", path.display()))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_match_the_version_as_a_word_when_a_probe_prints_it() {
        // given
        let outputs = [
            ("cargo-nextest 0.9.152 (26ebeda79 2026-11-13)", true),
            ("1.7.12\ninstalled by downloading", true),
            ("pinact version v5.0.2", true),
            ("version: 0.11.0", true),
            ("ripgrep 15.2.01", false),
            ("ripgrep 15.2.0-rc1", false),
            ("x15.2.0", false),
        ];

        // when
        let matched: Vec<bool> = outputs
            .iter()
            .map(|(text, _)| {
                let version = if text.starts_with("cargo") {
                    "0.9.152"
                } else if text.starts_with("1.7") {
                    "1.7.12"
                } else if text.starts_with("pinact") {
                    "5.0.2"
                } else if text.starts_with("version") {
                    "0.11.0"
                } else {
                    "15.2.0"
                };
                names_version(text, version)
            })
            .collect();

        // then
        let expected: Vec<bool> = outputs.iter().map(|(_, ok)| *ok).collect();
        assert_eq!(matched, expected);
    }

    #[test]
    fn should_hash_a_file_when_its_digest_is_asked() {
        // given: the SHA-256 of "abc" (FIPS 180-2, appendix B.1)
        let dir = std::env::temp_dir().join(format!("xtask-sha-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("abc");
        let _ = std::fs::write(&file, b"abc");

        // when
        let digest = sha256_of(&file);
        let _ = std::fs::remove_dir_all(&dir);

        // then
        assert_eq!(
            digest.as_deref(),
            Ok("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
    }

    #[test]
    fn should_see_a_cache_inside_the_repository_when_it_is_below_the_root() {
        // given
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

        // when
        let below = inside(&root.join("target/not-yet/tools"), &root);
        let elsewhere = inside(Path::new("/nonexistent-xtask-cache/tools"), &root);

        // then
        assert!(below);
        assert!(!elsewhere);
    }
}
