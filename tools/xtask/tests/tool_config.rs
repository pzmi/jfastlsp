//! `xtask tool-config` and how `scripts/bootstrap-tools.sh` uses it (FND-09, XT-01): every case of
//! the removed `scripts/test-tool-config.sh`, ported one for one. Scratch trees in a temporary
//! directory outside the repository (inside it, an ignored target directory would hide their
//! files from `git ls-files`), offline. Each tree that holds a configuration a tool of the gate
//! would read by itself, or a mise.toml or mise.lock outside its shape, must be refused with the
//! file named; the repository's own tree must pass. The bootstrap runs against a fake `mise`,
//! `just` and `cargo` that log every call, with this test's xtask standing in for the one the
//! launcher would build: with the reviewer's payload in mise.toml it must not call mise beyond
//! `--version`, and it must never call `mise trust` other than to remove a trust.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

type TestResult = Result<(), Box<dyn Error>>;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn printed(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// A fresh directory for one case, outside the repository, removed again when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(case: &str) -> Result<Self, Box<dyn Error>> {
        let dir =
            std::env::temp_dir().join(format!("xtask-tool-config-{}-{case}", std::process::id()));
        if dir.try_exists()? {
            fs::remove_dir_all(&dir)?;
        }
        fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }

    /// A copy of the repository's tool configuration in `<scratch>/<name>`.
    fn tree(&self, name: &str) -> Result<PathBuf, Box<dyn Error>> {
        let tree = self.0.join(name);
        fs::create_dir_all(tree.join(".github"))?;
        fs::create_dir_all(tree.join(".config"))?;
        for file in [
            "mise.toml",
            "mise.lock",
            ".github/zizmor.yml",
            ".config/nextest.toml",
        ] {
            fs::copy(repo_root().join(file), tree.join(file))?;
        }
        Ok(tree)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn tool_config(tree: &Path) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("tool-config")
        .arg(tree)
        .output()?)
}

/// Fails unless the check refuses `tree` naming `pattern`.
fn expect_refused(tree: &Path, pattern: &str) -> TestResult {
    let output = tool_config(tree)?;
    let text = printed(&output);
    if output.status.success() {
        return Err(format!("{}: the check passed", tree.display()).into());
    }
    if !text.contains(pattern) {
        return Err(format!(
            "{}: output does not name `{pattern}`:\n{text}",
            tree.display()
        )
        .into());
    }
    Ok(())
}

/// The repository's mise.lock with the first line starting `key` replaced by `to` (dropped when
/// `to` is empty).
fn lock_with(key: &str, to: &str) -> Result<String, Box<dyn Error>> {
    let text = fs::read_to_string(repo_root().join("mise.lock"))?;
    let mut done = false;
    let mut out = String::new();
    for line in text.lines() {
        if !done && line.starts_with(key) {
            done = true;
            if !to.is_empty() {
                out.push_str(to);
                out.push('\n');
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if !done {
        return Err(format!("mise.lock has no line starting `{key}`").into());
    }
    Ok(out)
}

#[test]
fn should_pass_when_the_tree_is_the_repository_or_a_copy_of_its_tool_configuration() -> TestResult {
    // given
    let scratch = Scratch::new("clean")?;
    let clean = scratch.tree("clean")?;

    // when
    let repo = tool_config(&repo_root())?;
    let copy = tool_config(&clean)?;

    // then
    assert!(repo.status.success(), "the repository: {}", printed(&repo));
    assert!(copy.status.success(), "clean: {}", printed(&copy));
    Ok(())
}

#[test]
fn should_refuse_naming_the_file_when_a_tool_would_read_an_implicit_configuration() -> TestResult {
    // given: the configuration files the gate's tools read without being told (the FND-08
    // re-verification's F1)
    let scratch = Scratch::new("implicit")?;
    let files = [
        ".shellcheckrc",
        ".yamllint",
        ".tool-versions",
        ".mise.toml",
        "mise.local.toml",
        "mise.ci.toml",
        ".pinact.yaml",
        ".envrc",
        ".github/actionlint.yaml",
        ".github/actionlint.yml",
        ".github/pinact.yaml",
        ".config/mise.toml",
    ];
    let mut failures = Vec::new();

    // when
    for file in files {
        let tree = scratch.tree(&format!("implicit-{}", file.replace(['/', '.'], "_")))?;
        fs::write(tree.join(file), "disable=all\n")?;
        if let Err(err) = expect_refused(&tree, file) {
            failures.push(err.to_string());
        }
    }

    // then
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

#[test]
fn should_refuse_when_a_mise_conf_d_directory_sits_under_config() -> TestResult {
    // given
    let scratch = Scratch::new("confdir")?;
    let tree = scratch.tree("confdir")?;
    fs::create_dir_all(tree.join(".config/mise/conf.d"))?;
    fs::write(tree.join(".config/mise/conf.d/x.toml"), "[env]\n")?;

    // when
    let refused = expect_refused(&tree, ".config/mise");

    // then
    refused
}

#[test]
fn should_refuse_any_cargo_or_nested_toolchain_file_when_it_could_steer_cargo() -> TestResult {
    // given: G2 (the FND-09 review), cargo and toolchain configuration, which any cargo command
    // obeys, at the root, nested and in another letter case (H6)
    let scratch = Scratch::new("g2-cargo")?;
    let files = [
        ".cargo/config.toml",
        ".cargo/config",
        "crates/x/.cargo/config.toml",
        "crates/x/rust-toolchain.toml",
        "rust-toolchain",
        ".Cargo/Config.toml",
        "crates/x/Rust-Toolchain.toml",
    ];
    let mut failures = Vec::new();

    // when
    for file in files {
        let tree = scratch.tree(&format!("g2-cargo-{}", file.replace(['/', '.'], "_")))?;
        let path = tree.join(file);
        fs::create_dir_all(path.parent().ok_or("no parent")?)?;
        fs::write(&path, "[build]\nrustc-wrapper = \"./evil.sh\"\n")?;
        if let Err(err) = expect_refused(&tree, file) {
            failures.push(err.to_string());
        }
    }

    // then
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

#[test]
fn should_refuse_a_toolchain_path_and_accept_the_pinned_shape_when_rust_toolchain_toml_is_read()
-> TestResult {
    // given
    let scratch = Scratch::new("g2-toolchain")?;
    let path = scratch.tree("g2-toolchain-path")?;
    let pinned = fs::read_to_string(repo_root().join("rust-toolchain.toml"))?;
    fs::write(
        path.join("rust-toolchain.toml"),
        format!("{pinned}path = \"/elsewhere/toolchain\"\n"),
    )?;
    let ok = scratch.tree("g2-toolchain-ok")?;
    fs::write(ok.join("rust-toolchain.toml"), &pinned)?;

    // when
    let refused = expect_refused(&path, "rust-toolchain.toml:");
    let accepted = tool_config(&ok)?;

    // then
    refused?;
    assert!(
        accepted.status.success(),
        "g2-toolchain-ok: {}",
        printed(&accepted)
    );
    Ok(())
}

#[test]
fn should_refuse_mise_toml_when_it_holds_a_payload_a_plugin_a_template_or_a_symlink() -> TestResult
{
    // given: the reviewer's payload, a plugin backend, a template version, a symlink
    let scratch = Scratch::new("mise-toml")?;
    let base = fs::read_to_string(repo_root().join("mise.toml"))?;
    let payload = scratch.tree("payload")?;
    fs::write(
        payload.join("mise.toml"),
        format!("{base}\n[env]\n_.source = \"{{{{config_root}}}}/payload.sh\"\n"),
    )?;
    let plugin = scratch.tree("plugin")?;
    fs::write(
        plugin.join("mise.toml"),
        format!("{base}\"asdf:someone/plugin\" = \"1.0.0\"\n"),
    )?;
    let template = scratch.tree("template")?;
    fs::write(
        template.join("mise.toml"),
        format!("{base}\"aqua:a/b\" = \"{{{{exec(command='true')}}}}\"\n"),
    )?;
    let symlink = scratch.tree("symlink")?;
    fs::remove_file(symlink.join("mise.toml"))?;
    std::os::unix::fs::symlink(repo_root().join("mise.toml"), symlink.join("mise.toml"))?;

    // when
    let found = [
        expect_refused(&payload, "mise.toml"),
        expect_refused(&plugin, "mise.toml"),
        expect_refused(&template, "mise.toml"),
        expect_refused(&symlink, "mise.toml: not a regular file"),
    ];

    // then
    let failures: Vec<String> = found
        .into_iter()
        .filter_map(Result::err)
        .map(|e| e.to_string())
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

#[test]
fn should_refuse_mise_lock_when_an_asset_comes_from_elsewhere_or_lacks_a_sha256() -> TestResult {
    // given: an asset from elsewhere, a weak checksum, a platform without one
    let scratch = Scratch::new("mise-lock")?;
    let url = scratch.tree("url")?;
    fs::write(
        url.join("mise.lock"),
        lock_with("url = ", "url = \"https://example.invalid/tool.tar.gz\"")?,
    )?;
    let md5 = scratch.tree("md5")?;
    fs::write(
        md5.join("mise.lock"),
        lock_with("checksum = ", "checksum = \"md5:0123\"")?,
    )?;
    let nosum = scratch.tree("nosum")?;
    fs::write(nosum.join("mise.lock"), lock_with("checksum = ", "")?)?;

    // when
    let found = [
        expect_refused(&url, "mise.lock"),
        expect_refused(&md5, "mise.lock"),
        expect_refused(&nosum, "without a sha256 checksum"),
    ];

    // then
    let failures: Vec<String> = found
        .into_iter()
        .filter_map(Result::err)
        .map(|e| e.to_string())
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

/// The pinned version of `tool` in scripts/bootstrap-tools.sh (`<TOOL>_VERSION="..."`).
fn pinned(tool: &str) -> Result<String, Box<dyn Error>> {
    let script = fs::read_to_string(repo_root().join("scripts/bootstrap-tools.sh"))?;
    let prefix = format!("{tool}_VERSION=\"");
    script
        .lines()
        .find_map(|l| l.strip_prefix(&prefix)?.strip_suffix('"'))
        .map(str::to_owned)
        .ok_or_else(|| format!("no pinned {tool} version").into())
}

/// Where scripts/xtask.sh builds the xtask of the checkout `repo` under `target`:
/// `xtask/<SHA-256 of the checkout's physical path>`.
fn launcher_dir(target: &Path, repo: &Path) -> Result<PathBuf, Box<dyn Error>> {
    use std::io::Write as _;
    let physical = fs::canonicalize(repo)?;
    // sha256sum where it exists (Linux), else shasum (macOS), as the launcher picks.
    let mut tool = Command::new("sha256sum");
    if Command::new("sha256sum").arg("--version").output().is_err() {
        tool = Command::new("shasum");
        tool.args(["-a", "256"]);
    }
    let mut hash = tool
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()?;
    hash.stdin
        .take()
        .ok_or("no stdin")?
        .write_all(physical.as_os_str().as_encoded_bytes())?;
    let output = hash.wait_with_output()?;
    let sum = String::from_utf8(output.stdout)?
        .split_whitespace()
        .next()
        .ok_or("the hash tool printed nothing")?
        .to_owned();
    Ok(target.join("xtask").join(sum))
}

/// A scratch repository `name` holding the bootstrap and the launcher, a fake `mise`, `just` and
/// `cargo` (each call logged to `calls`) and, where the launcher looks for the binary it built,
/// this test's xtask.
struct Bootstrap {
    repo: PathBuf,
    bin: PathBuf,
    target: PathBuf,
    calls: PathBuf,
}

impl Bootstrap {
    fn new(scratch: &Scratch, name: &str) -> Result<Self, Box<dyn Error>> {
        let repo = scratch.tree(name)?;
        fs::create_dir_all(repo.join("scripts"))?;
        for file in [
            "scripts/bootstrap-tools.sh",
            "scripts/xtask.sh",
            "rust-toolchain.toml",
        ] {
            fs::copy(repo_root().join(file), repo.join(file))?;
        }
        let bin = scratch.0.join(format!("{name}-bin"));
        let target = scratch.0.join(format!("{name}-target"));
        let calls = scratch.0.join(format!("{name}-calls"));
        fs::create_dir_all(&bin)?;
        let built = launcher_dir(&target, &repo)?.join("debug");
        fs::create_dir_all(&built)?;
        fs::copy(env!("CARGO_BIN_EXE_xtask"), built.join("xtask"))?;
        let log = calls.display();
        fs::write(
            bin.join("mise"),
            format!(
                "#!/usr/bin/env bash\necho \"mise $*\" >>'{log}'\ncase \"$1\" in --version) echo \"{} linux-x64\" ;; esac\n",
                pinned("MISE")?
            ),
        )?;
        fs::write(
            bin.join("just"),
            format!("#!/usr/bin/env bash\necho \"just {}\"\n", pinned("JUST")?),
        )?;
        fs::write(
            bin.join("cargo"),
            format!("#!/usr/bin/env bash\necho \"cargo in $PWD: $*\" >>'{log}'\n"),
        )?;
        let chmod = Command::new("chmod")
            .arg("+x")
            .args([bin.join("mise"), bin.join("just"), bin.join("cargo")])
            .output()?;
        assert!(chmod.status.success(), "{}", printed(&chmod));
        Ok(Self {
            repo,
            bin,
            target,
            calls,
        })
    }

    fn run(&self) -> Result<Output, Box<dyn Error>> {
        fs::write(&self.calls, "")?;
        let mut path = std::ffi::OsString::from(&self.bin);
        if let Some(rest) = std::env::var_os("PATH") {
            path.push(":");
            path.push(rest);
        }
        Ok(Command::new("bash")
            .arg(self.repo.join("scripts/bootstrap-tools.sh"))
            .arg(&self.bin)
            .env("PATH", path)
            .env("CARGO_TARGET_DIR", &self.target)
            .output()?)
    }

    fn calls(&self) -> Result<Vec<String>, Box<dyn Error>> {
        Ok(fs::read_to_string(&self.calls)?
            .lines()
            .map(str::to_owned)
            .collect())
    }
}

#[test]
fn should_install_from_mise_lock_and_never_trust_the_tree_when_the_bootstrap_meets_a_clean_tree()
-> TestResult {
    // given
    let scratch = Scratch::new("bootstrap-clean")?;
    let bootstrap = Bootstrap::new(&scratch, "clean")?;

    // when
    let output = bootstrap.run()?;
    let calls = bootstrap.calls()?;

    // then
    assert!(output.status.success(), "{}", printed(&output));
    assert!(
        calls.iter().any(|c| c == "mise install --locked"),
        "no mise install: {calls:?}"
    );
    assert!(
        !calls
            .iter()
            .any(|c| c.starts_with("mise trust") && !c.contains("--untrust")),
        "it trusted the repository for mise: {calls:?}"
    );
    let build = calls
        .iter()
        .find(|c| c.starts_with("cargo in "))
        .ok_or("the launcher did not build xtask")?;
    assert!(
        build.contains("/build: build --locked --quiet --manifest-path ")
            && build.ends_with("/Cargo.toml -p xtask"),
        "{build}"
    );
    Ok(())
}

#[test]
fn should_run_no_mise_and_no_payload_when_the_bootstrap_meets_the_reviewer_s_payload() -> TestResult
{
    // given
    let scratch = Scratch::new("bootstrap-payload")?;
    let bootstrap = Bootstrap::new(&scratch, "payload")?;
    let ran = scratch.0.join("ran");
    let base = fs::read_to_string(repo_root().join("mise.toml"))?;
    fs::write(
        bootstrap.repo.join("mise.toml"),
        format!("{base}\n[env]\n_.source = \"{{{{config_root}}}}/payload.sh\"\n"),
    )?;
    fs::write(
        bootstrap.repo.join("payload.sh"),
        format!("touch '{}'\n", ran.display()),
    )?;

    // when
    let output = bootstrap.run()?;
    let calls = bootstrap.calls()?;

    // then
    let text = printed(&output);
    assert!(
        !output.status.success(),
        "bootstrap payload: passed\n{text}"
    );
    assert!(text.contains("mise.toml"), "mise.toml not named:\n{text}");
    let mise: Vec<&String> = calls
        .iter()
        .filter(|c| c.starts_with("mise ") && c.as_str() != "mise --version")
        .collect();
    assert!(mise.is_empty(), "mise ran on a refused tree: {mise:?}");
    assert!(!ran.try_exists()?, "the payload ran");
    assert!(
        launcher_dir(&bootstrap.target, &bootstrap.repo)?
            .join("debug/xtask")
            .is_file()
    );
    Ok(())
}
