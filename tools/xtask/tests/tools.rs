//! `xtask doctor` and `xtask install-tools` (XT-02, fnd §8) over a scratch repository whose
//! `tools/versions.toml`, mise.toml and mise.lock pin one synthetic tool, `faketool`, whose
//! release archive the test builds (so nothing is downloaded: `install-tools --archives`). The
//! tool cache is a scratch `XDG_CACHE_HOME`; mise is a fake whose `bin-paths` names a scratch
//! install directory, so both install paths of FND-08's N6 are exercised: xtask's cache first,
//! then mise's install directories, never the shims while either holds the tool.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use sha2::{Digest as _, Sha256};

type TestResult = Result<(), Box<dyn Error>>;

fn printed(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// The platform `xtask` runs on, as mise.lock names it.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const HOST: &str = "linux-x64";
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const HOST: &str = "linux-arm64";
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const HOST: &str = "macos-arm64";

const URL: &str = "https://github.com/example/faketool/releases/download/v1.2.3/faketool.tar.gz";

fn chmod_x(file: &Path) -> TestResult {
    let chmod = Command::new("chmod").arg("+x").arg(file).output()?;
    assert!(chmod.status.success(), "{}", printed(&chmod));
    Ok(())
}

/// A program that prints `faketool <version>`.
fn program(file: &Path, version: &str) -> TestResult {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(file, format!("#!/bin/sh\necho \"faketool {version}\"\n"))?;
    chmod_x(file)
}

/// The scratch world of one case, removed when dropped.
struct World {
    dir: PathBuf,
    repo: PathBuf,
    archives: PathBuf,
    cache: PathBuf,
    mise_bins: PathBuf,
    bin: PathBuf,
}

impl World {
    /// A repository pinning `faketool` 1.2.3 at the SHA-256 of the archive built here, whose
    /// mise.lock carries `lock_sha` (`None`: the same digest).
    fn new(case: &str, lock_sha: Option<&str>) -> Result<Self, Box<dyn Error>> {
        let dir = std::env::temp_dir().join(format!("xtask-tools-{}-{case}", std::process::id()));
        if dir.try_exists()? {
            fs::remove_dir_all(&dir)?;
        }
        let world = Self {
            repo: dir.join("repo"),
            archives: dir.join("archives"),
            cache: dir.join("cache"),
            mise_bins: dir.join("mise-installs/faketool/1.2.3"),
            bin: dir.join("bin"),
            dir,
        };
        let tree = world.dir.join("tree");
        program(&tree.join("faketool-1.2.3/faketool"), "1.2.3")?;
        fs::create_dir_all(&world.archives)?;
        let archive = world.archives.join("faketool.tar.gz");
        let tar = Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&tree)
            .arg("faketool-1.2.3")
            .output()?;
        assert!(tar.status.success(), "{}", printed(&tar));
        let sha = Sha256::digest(fs::read(&archive)?)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let lock_sha = lock_sha.unwrap_or(&sha);
        fs::create_dir_all(world.repo.join("tools"))?;
        let mut versions = "[tools.faketool]\nversion = \"1.2.3\"\nmise = \"aqua:example/faketool\"\nprobe = [\"faketool\", \"--version\"]\n".to_owned();
        let mut lock = "lockfile_version = 3\n\n[[tools.\"aqua:example/faketool\"]]\nversion = \"1.2.3\"\nbackend = \"aqua:example/faketool\"\nspecifiers = [\"1.2.3\"]\n".to_owned();
        for platform in ["linux-x64", "linux-arm64", "macos-arm64"] {
            versions.push_str(&format!(
                "\n[tools.faketool.{platform}]\nurl = \"{URL}\"\nsha256 = \"{sha}\"\nbin = \"faketool-1.2.3/faketool\"\n"
            ));
            lock.push_str(&format!(
                "\n[tools.\"aqua:example/faketool\".\"platforms.{platform}\"]\nchecksum = \"sha256:{lock_sha}\"\nurl = \"{URL}\"\n"
            ));
        }
        fs::write(world.repo.join("tools/versions.toml"), versions)?;
        fs::write(
            world.repo.join("mise.toml"),
            "[tools]\n\"aqua:example/faketool\" = \"1.2.3\"\n",
        )?;
        fs::write(world.repo.join("mise.lock"), lock)?;
        fs::create_dir_all(&world.bin)?;
        fs::write(
            world.bin.join("mise"),
            format!(
                "#!/bin/sh\ncase \"$1\" in bin-paths) [ -d '{0}' ] && echo '{0}' ;; esac\nexit 0\n",
                world.mise_bins.display()
            ),
        )?;
        chmod_x(&world.bin.join("mise"))?;
        Ok(world)
    }

    /// `xtask --root <repo> <args>` with the scratch cache, the fake mise and the system's PATH.
    fn xtask(&self, args: &[&str]) -> Result<Output, Box<dyn Error>> {
        let mut path = std::ffi::OsString::from(&self.bin);
        path.push(":/usr/bin:/bin");
        Ok(Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("--root")
            .arg(&self.repo)
            .args(args)
            .env_clear()
            .env("PATH", path)
            .env("HOME", &self.dir)
            .env("XDG_CACHE_HOME", &self.cache)
            .env("MISE_DATA_DIR", self.dir.join("mise-data"))
            .output()?)
    }

    fn install(&self) -> Result<Output, Box<dyn Error>> {
        let archives = self.archives.display().to_string();
        self.xtask(&["install-tools", "--archives", &archives])
    }

    /// Where `install-tools` puts the program.
    fn installed(&self) -> PathBuf {
        self.cache
            .join("jfastlsp/tools/faketool/1.2.3")
            .join(HOST)
            .join("bin/faketool")
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn should_install_the_tool_and_doctor_find_it_when_the_archive_matches_its_pin() -> TestResult {
    // given
    let world = World::new("install", None)?;

    // when
    let installed = world.install()?;
    let again = world.install()?;
    let doctor = world.xtask(&["doctor"])?;

    // then
    assert!(installed.status.success(), "{}", printed(&installed));
    assert!(world.installed().is_file());
    assert!(
        printed(&again).contains("ok faketool 1.2.3 (already installed)"),
        "{}",
        printed(&again)
    );
    let text = printed(&doctor);
    assert!(doctor.status.success(), "{text}");
    assert!(
        text.contains(&format!(
            "ok      faketool 1.2.3 ({})",
            world.installed().display()
        )),
        "{text}"
    );
    Ok(())
}

#[test]
fn should_refuse_and_install_nothing_when_the_archive_is_tampered() -> TestResult {
    // given: the archive changed after its digest was pinned
    let world = World::new("tampered", None)?;
    let archive = world.archives.join("faketool.tar.gz");
    let mut bytes = fs::read(&archive)?;
    bytes.extend_from_slice(b"tampered");
    fs::write(&archive, bytes)?;

    // when
    let output = world.install()?;

    // then
    let text = printed(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("but tools/versions.toml pins"), "{text}");
    assert!(text.contains("refused, nothing installed"), "{text}");
    assert!(
        !world
            .cache
            .join("jfastlsp/tools/faketool/1.2.3")
            .join(HOST)
            .try_exists()?,
        "{text}"
    );
    Ok(())
}

#[test]
fn should_name_the_tool_and_the_fix_when_doctor_finds_it_missing() -> TestResult {
    // given: nothing installed by either path
    let world = World::new("missing", None)?;

    // when
    let output = world.xtask(&["doctor"])?;

    // then
    let text = printed(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("missing faketool 1.2.3"), "{text}");
    assert!(text.contains("install-tools"), "{text}");
    Ok(())
}

#[test]
fn should_name_the_version_found_when_doctor_finds_another() -> TestResult {
    // given: mise's install directory holds another version
    let world = World::new("wrong", None)?;
    program(&world.mise_bins.join("faketool"), "9.9.9")?;

    // when
    let output = world.xtask(&["doctor"])?;

    // then
    let text = printed(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("wrong   faketool 1.2.3"), "{text}");
    assert!(text.contains("says 'faketool 9.9.9'"), "{text}");
    assert!(text.contains("install-tools"), "{text}");
    Ok(())
}

#[test]
fn should_run_the_tools_from_their_install_directories_when_both_paths_hold_them() -> TestResult {
    // given: the tool in mise's install directory and in mise's shims, then also in xtask's cache
    let world = World::new("paths", None)?;
    program(&world.mise_bins.join("faketool"), "1.2.3")?;
    program(&world.dir.join("mise-data/shims/faketool"), "1.2.3")?;

    // when
    let mise_only = world.xtask(&["doctor"])?;
    let installed = world.install()?;
    let both = world.xtask(&["doctor"])?;

    // then: mise's install directory before its shims; xtask's cache before both
    let text = printed(&mise_only);
    assert!(mise_only.status.success(), "{text}");
    assert!(
        text.contains(&format!("({})", world.mise_bins.join("faketool").display())),
        "{text}"
    );
    assert!(installed.status.success(), "{}", printed(&installed));
    let text = printed(&both);
    assert!(both.status.success(), "{text}");
    assert!(
        text.contains(&format!("({})", world.installed().display())),
        "{text}"
    );
    Ok(())
}

#[test]
fn should_fail_doctor_when_versions_toml_and_mise_lock_disagree() -> TestResult {
    // given: mise.lock pins another digest than versions.toml, and the tool is installed
    let other = "0".repeat(64);
    let world = World::new("disagree", Some(&other))?;
    let installed = world.install()?;

    // when
    let output = world.xtask(&["doctor"])?;

    // then
    assert!(installed.status.success(), "{}", printed(&installed));
    let text = printed(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(
        text.contains(&format!(
            "tools/versions.toml and mise disagree: tools.faketool.{HOST}: sha256"
        )),
        "{text}"
    );
    Ok(())
}

#[test]
fn should_refuse_a_tool_cache_inside_the_repository_when_installing() -> TestResult {
    // given
    let world = World::new("inside", None)?;
    let archives = world.archives.display().to_string();

    // when
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("--root")
        .arg(&world.repo)
        .args(["install-tools", "--archives", &archives])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("XDG_CACHE_HOME", world.repo.join("cache"))
        .output()?;

    // then
    let text = printed(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("inside the repository"), "{text}");
    assert!(!world.repo.join("cache").try_exists()?, "{text}");
    Ok(())
}

#[test]
fn should_refuse_doctor_and_install_when_versions_toml_leaves_its_shape() -> TestResult {
    // given: a url on the GitHub API, which fnd §8 refuses
    let world = World::new("shape", None)?;
    let file = world.repo.join("tools/versions.toml");
    let text = fs::read_to_string(&file)?.replacen(
        URL,
        "https://api.github.com/repos/example/faketool/releases/assets/1",
        1,
    );
    fs::write(&file, text)?;

    // when
    let doctor = world.xtask(&["doctor"])?;
    let install = world.install()?;

    // then
    for output in [&doctor, &install] {
        let text = printed(output);
        assert_eq!(output.status.code(), Some(1), "{text}");
        assert!(text.contains("is not a GitHub release asset"), "{text}");
    }
    Ok(())
}

#[test]
fn should_install_the_whole_archive_when_the_tool_is_a_tree() -> TestResult {
    // given: `kind = "tree"` and a program that reads a file beside it, as Node, the JDK and
    // Apalache do (FORMAL-01), so the archive's one program alone would not run
    let world = World::new("tree", None)?;
    let top = world.dir.join("tree-archive/faketool-1.2.3");
    fs::create_dir_all(top.join("share"))?;
    fs::write(top.join("share/version"), "1.2.3\n")?;
    fs::create_dir_all(top.join("bin"))?;
    fs::write(
        top.join("bin/faketool"),
        "#!/bin/sh\necho \"faketool $(cat \"$(dirname \"$0\")/../share/version\")\"\n",
    )?;
    chmod_x(&top.join("bin/faketool"))?;
    let archive = world.archives.join("faketool.tar.gz");
    let tar = Command::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(world.dir.join("tree-archive"))
        .arg("faketool-1.2.3")
        .output()?;
    assert!(tar.status.success(), "{}", printed(&tar));
    let sha = Sha256::digest(fs::read(&archive)?)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let versions = world.repo.join("tools/versions.toml");
    let text = fs::read_to_string(&versions)?;
    let old = text
        .split("sha256 = \"")
        .nth(1)
        .and_then(|rest| rest.get(..64))
        .ok_or("no sha256 in versions.toml")?
        .to_owned();
    let text = text
        .replace(&old, &sha)
        .replace(
            "bin = \"faketool-1.2.3/faketool\"",
            "bin = \"faketool-1.2.3/bin/faketool\"",
        )
        .replace(
            "probe = [\"faketool\", \"--version\"]\n",
            "probe = [\"faketool\", \"--version\"]\nkind = \"tree\"\n",
        );
    fs::write(&versions, text)?;
    let lock = world.repo.join("mise.lock");
    fs::write(&lock, fs::read_to_string(&lock)?.replace(&old, &sha))?;

    // when
    let installed = world.install()?;
    let doctor = world.xtask(&["doctor"])?;

    // then
    assert!(installed.status.success(), "{}", printed(&installed));
    let program = world
        .cache
        .join("jfastlsp/tools/faketool/1.2.3")
        .join(HOST)
        .join("tree/faketool-1.2.3/bin/faketool");
    assert!(program.is_file());
    let text = printed(&doctor);
    assert!(doctor.status.success(), "{text}");
    assert!(
        text.contains(&format!("ok      faketool 1.2.3 ({})", program.display())),
        "{text}"
    );
    Ok(())
}

#[test]
fn should_report_a_tool_as_optional_and_pass_doctor_when_only_one_command_needs_it() -> TestResult {
    // given: `only = "formal-verify"`, as the JDK and Apalache carry (FORMAL-01), and the tool
    // absent from both install paths; then another version in mise's install directory
    let world = World::new("optional", None)?;
    let versions = world.repo.join("tools/versions.toml");
    let text = fs::read_to_string(&versions)?.replacen(
        "probe = [\"faketool\", \"--version\"]\n",
        "probe = [\"faketool\", \"--version\"]\nonly = \"formal-verify\"\n",
        1,
    );
    fs::write(&versions, text)?;

    // when
    let absent = world.xtask(&["doctor"])?;
    program(&world.mise_bins.join("faketool"), "9.9.9")?;
    let wrong = world.xtask(&["doctor"])?;

    // then
    for output in [&absent, &wrong] {
        let text = printed(output);
        assert!(output.status.success(), "{text}");
        assert!(
            text.contains("absent  faketool 1.2.3 (optional: only `formal-verify` needs it"),
            "{text}"
        );
        assert!(text.contains("--formal-verify"), "{text}");
    }
    assert!(printed(&wrong).contains("says 'faketool 9.9.9'"));
    Ok(())
}
