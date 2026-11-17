//! Stock jdtls for the comparison arm (BENCH-03; ADR 0028, amended 2026-11-17): the pinned
//! Eclipse JDT Language Server, started as a separate process for one bench session and talked to
//! over its stdio. Nothing of ours wraps it: no proxy, no daemon, no shared workspace.
//!
//! - **Pinned, verified at fetch time.** [`JDTLS_VERSION`] and [`JDTLS_SHA256`] are the release
//!   `scripts/jdtls-fetch.sh` downloads; that script checks the archive's SHA-256 before it
//!   extracts anything and then records the digest in the install's `.verified`. [`launcher`]
//!   refuses an install whose `.verified` is not this digest (missing, partial or another
//!   release). That is trust on fetch, not a check of the files: the archive is not kept, so the
//!   extracted install is not re-hashed, and a later change to it under the same marker is not
//!   detected. `scripts/test-jdtls-fetch.sh` checks that the script pins the same version and
//!   digest as this module.
//! - **Fresh per run.** The `-data` workspace and the writable OSGi `-configuration` are made
//!   empty under the run's scratch directory, so no index or import state carries over from an
//!   earlier run ([`fresh_dir`]).
//! - **Argv only, bounded.** The JVM is started the way jdtls's own `bin/jdtls` launcher starts
//!   it (the equinox launcher jar and the install's read-only shared configuration), with a heap
//!   cap, under `timeout(1)` so a server that stops answering ends at the suite's deadline (the
//!   harness holds no thread or timed-wait grant, fnd §2.4), with the environment cleared to
//!   [`ENV_ALLOWLIST`]. jdtls (EPL-2.0) is never linked or bundled.
//!
//! Pinned by: this module's tests (the command line, the environment, the refusal of an
//! unverified install, the fresh directories).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{BenchError, Context, Result};

/// The pinned jdtls release: the newest that runs on Java 21 (from 1.62.0 the Eclipse platform
/// bundles require JavaSE-25).
pub const JDTLS_VERSION: &str = "1.61.0";

/// The SHA-256 of the release archive `scripts/jdtls-fetch.sh` verifies and records in
/// `.verified`.
pub const JDTLS_SHA256: &str = "338e7e73d61836651ba2453919a0d34fa763eb4e7c03342092309bffb8934c64";

/// The variables the JVM inherits; everything else is cleared, so a `JAVA_TOOL_OPTIONS` or a
/// `CLASSPATH` of the caller's shell cannot change what is measured.
pub const ENV_ALLOWLIST: &[&str] = &["PATH", "HOME", "JAVA_HOME", "LANG", "LC_ALL", "TMPDIR"];

/// Where `scripts/jdtls-fetch.sh` installs the release by default:
/// `${XDG_CACHE_HOME:-$HOME/.cache}/jfastlsp/arms/jdtls-<version>`.
pub fn default_home() -> Option<PathBuf> {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(cache.join(format!("jfastlsp/arms/jdtls-{JDTLS_VERSION}")))
}

/// The `java` to start: `$JAVA_HOME/bin/java` when it exists, else the first on `PATH`.
pub fn java() -> Option<PathBuf> {
    std::env::var_os("JAVA_HOME")
        .filter(|v| !v.is_empty())
        .map(|h| PathBuf::from(h).join("bin/java"))
        .filter(|p| p.is_file())
        .or_else(|| crate::machine::find_on_path("java"))
}

/// The equinox launcher jar of an install fetched and verified by `scripts/jdtls-fetch.sh`:
/// `home/.verified` must hold [`JDTLS_SHA256`] (written after the archive's checksum check; the
/// extracted files are not re-hashed here), and `home/plugins` must hold
/// one `org.eclipse.equinox.launcher_*.jar` (the newest by name when there are several).
///
/// # Errors
///
/// When the install is missing, unverified, of another release, or has no launcher jar.
pub fn launcher(home: &Path) -> Result<PathBuf> {
    let marker = home.join(".verified");
    let recorded = std::fs::read_to_string(&marker).context(format!(
        "no verified jdtls {JDTLS_VERSION} at {} (run `bash scripts/jdtls-fetch.sh`, or pass --jdtls-home DIR)",
        home.display()
    ))?;
    if recorded.trim() != JDTLS_SHA256 {
        return Err(BenchError::new(format!(
            "{} records {:?}, not the pinned jdtls {JDTLS_VERSION} ({JDTLS_SHA256}): run `bash scripts/jdtls-fetch.sh`",
            marker.display(),
            recorded.trim()
        )));
    }
    let plugins = home.join("plugins");
    let mut found: Option<PathBuf> = None;
    for entry in std::fs::read_dir(&plugins).context(format!("read {}", plugins.display()))? {
        let name = entry.context("read a directory entry")?.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("org.eclipse.equinox.launcher_") && name.ends_with(".jar") {
            let path = plugins.join(name.as_ref());
            if found.as_ref().is_none_or(|f| path > *f) {
                found = Some(path);
            }
        }
    }
    found.context(format!(
        "no equinox launcher jar under {}: run `bash scripts/jdtls-fetch.sh`",
        plugins.display()
    ))
}

/// Make `dir` an empty directory: remove whatever is there, then create it.
///
/// # Errors
///
/// When it cannot be removed or created.
pub fn fresh_dir(dir: &Path) -> Result<()> {
    if dir
        .try_exists()
        .context(format!("stat {}", dir.display()))?
    {
        std::fs::remove_dir_all(dir).context(format!("remove {}", dir.display()))?;
    }
    std::fs::create_dir_all(dir).context(format!("create {}", dir.display()))
}

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const CONFIG_DIR: &str = "config_linux_arm";
#[cfg(all(target_os = "linux", not(target_arch = "aarch64")))]
const CONFIG_DIR: &str = "config_linux";
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const CONFIG_DIR: &str = "config_mac_arm";
#[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
const CONFIG_DIR: &str = "config_mac";

/// One session's launch settings.
#[derive(Debug, Clone)]
pub struct Launch<'a> {
    /// The `java` executable (Java 21 or newer).
    pub java: &'a Path,
    /// The verified install (holds `plugins/` and the `config_*` directories).
    pub home: &'a Path,
    /// Its equinox launcher jar ([`launcher`]).
    pub launcher: &'a Path,
    /// The `-data` workspace, fresh for this run.
    pub data: &'a Path,
    /// The writable OSGi configuration area, fresh for this run.
    pub configuration: &'a Path,
    /// The heap cap, `-Xmx`, in MiB; `-Xms` is the cap up to 1 GiB, as `bin/jdtls` starts at 1 GiB.
    pub heap_mib: u32,
    /// The whole session's deadline, seconds (`timeout`).
    pub deadline_secs: u64,
}

fn prefixed(prefix: &str, path: &Path) -> OsString {
    let mut out = OsString::from(prefix);
    out.push(path.as_os_str());
    out
}

/// The command line: `timeout`, then the JVM and jdtls's own arguments. The JVM options are the
/// ones the BENCH-03 baseline ran with (`bin/jdtls`'s, vscode-java's GC settings and the heap
/// cap), so the series stay comparable.
pub fn argv(l: &Launch<'_>) -> Vec<OsString> {
    let initial_heap = l.heap_mib.min(1024);
    vec![
        "timeout".into(),
        "--kill-after=10s".into(),
        format!("{}s", l.deadline_secs).into(),
        l.java.as_os_str().to_owned(),
        "-Declipse.application=org.eclipse.jdt.ls.core.id1".into(),
        "-Dosgi.bundles.defaultStartLevel=4".into(),
        "-Declipse.product=org.eclipse.jdt.ls.core.product".into(),
        "-Dosgi.checkConfiguration=true".into(),
        prefixed("-Dosgi.sharedConfiguration.area=", &l.home.join(CONFIG_DIR)),
        "-Dosgi.sharedConfiguration.area.readOnly=true".into(),
        "-Dosgi.configuration.cascaded=true".into(),
        "-Dsun.zip.disableMemoryMapping=true".into(),
        "-Djdk.xml.maxGeneralEntitySizeLimit=0".into(),
        "-Djdk.xml.totalEntitySizeLimit=0".into(),
        "-XX:+UseParallelGC".into(),
        "-XX:GCTimeRatio=4".into(),
        "-XX:AdaptiveSizePolicyWeight=90".into(),
        format!("-Xms{initial_heap}m").into(),
        format!("-Xmx{}m", l.heap_mib).into(),
        "--add-modules=ALL-SYSTEM".into(),
        "--add-opens".into(),
        "java.base/java.util=ALL-UNNAMED".into(),
        "--add-opens".into(),
        "java.base/java.lang=ALL-UNNAMED".into(),
        "-jar".into(),
        l.launcher.as_os_str().to_owned(),
        "-configuration".into(),
        l.configuration.as_os_str().to_owned(),
        "-data".into(),
        l.data.as_os_str().to_owned(),
    ]
}

/// The command for [`argv`], run in `cwd` with only [`ENV_ALLOWLIST`] inherited. `timeout`
/// starts its own process group and signals it at the deadline, so the JVM and anything it
/// starts are ended with it.
///
/// # Errors
///
/// Never in practice: [`argv`] is never empty.
pub fn command(l: &Launch<'_>, cwd: &Path) -> Result<Command> {
    let argv = argv(l);
    let (program, rest) = argv.split_first().context("empty jdtls command")?;
    let mut cmd = Command::new(program);
    cmd.args(rest).current_dir(cwd).env_clear();
    for name in ENV_ALLOWLIST {
        if let Some(value) = std::env::var_os(name) {
            cmd.env(name, value);
        }
    }
    Ok(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory, removed when the guard drops, so a failing assertion leaves nothing
    /// behind either.
    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn scratch(case: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("jfl-bench-jdtls-{case}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn launch<'a>(paths: &'a [PathBuf; 5]) -> Launch<'a> {
        Launch {
            java: &paths[0],
            home: &paths[1],
            launcher: &paths[2],
            data: &paths[3],
            configuration: &paths[4],
            heap_mib: 2048,
            deadline_secs: 600,
        }
    }

    fn after<'a>(argv: &'a [OsString], flag: &str) -> &'a OsString {
        argv.iter().skip_while(|a| *a != flag).nth(1).unwrap()
    }

    #[test]
    fn should_run_the_jvm_under_the_deadline_with_the_run_s_own_data_when_building_the_command() {
        // given
        let paths = [
            PathBuf::from("/jdk/bin/java"),
            PathBuf::from("/opt/jdtls"),
            PathBuf::from("/opt/jdtls/plugins/org.eclipse.equinox.launcher_1.jar"),
            PathBuf::from("/s/jdtls-data"),
            PathBuf::from("/s/jdtls-configuration"),
        ];
        // when
        let argv = argv(&launch(&paths));
        // then
        assert_eq!(
            &argv[..4],
            ["timeout", "--kill-after=10s", "600s", "/jdk/bin/java"]
        );
        assert_eq!(after(&argv, "-data"), "/s/jdtls-data");
        assert_eq!(after(&argv, "-configuration"), "/s/jdtls-configuration");
        assert_eq!(after(&argv, "-jar"), paths[2].as_os_str());
        assert!(argv.contains(&OsString::from("-Xmx2048m")));
        assert!(argv.contains(&OsString::from("-Xms1024m")));
        let shared = format!("-Dosgi.sharedConfiguration.area=/opt/jdtls/{CONFIG_DIR}");
        assert!(argv.contains(&OsString::from(shared)));
        assert!(
            !argv
                .iter()
                .any(|a| a.to_string_lossy().contains("sharedIndexLocation"))
        );
    }

    #[test]
    fn should_pass_only_the_allowlisted_environment_when_starting_jdtls() {
        // given
        let paths = [
            PathBuf::from("java"),
            PathBuf::from("/h"),
            PathBuf::from("/h/l.jar"),
            PathBuf::from("/d"),
            PathBuf::from("/c"),
        ];
        // when
        let cmd = command(&launch(&paths), Path::new("/w")).unwrap();
        // then: cleared, then only allowlisted names set
        let names: Vec<String> = cmd
            .get_envs()
            .filter(|(_, v)| v.is_some())
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().all(|n| ENV_ALLOWLIST.contains(&n.as_str())),
            "{names:?}"
        );
        assert_eq!(cmd.get_program(), "timeout");
        assert_eq!(cmd.get_current_dir(), Some(Path::new("/w")));
    }

    #[test]
    fn should_refuse_an_install_when_it_is_not_the_verified_pinned_release() {
        // given: no marker, another digest, and the pinned digest without a launcher jar
        let guard = scratch("refuse");
        let dir = &guard.0;
        let missing = dir.join("missing");
        let other = dir.join("other");
        let empty = dir.join("empty");
        for (home, digest) in [(&other, "0".repeat(64)), (&empty, JDTLS_SHA256.to_string())] {
            std::fs::create_dir_all(home.join("plugins")).unwrap();
            std::fs::write(home.join(".verified"), format!("{digest}\n")).unwrap();
        }
        // when
        let errors = [launcher(&missing), launcher(&other), launcher(&empty)];
        // then
        let text: Vec<String> = errors
            .iter()
            .map(|e| e.as_ref().unwrap_err().to_string())
            .collect();
        assert!(text[0].contains("no verified jdtls"), "{}", text[0]);
        assert!(text[1].contains("not the pinned jdtls"), "{}", text[1]);
        assert!(text[2].contains("no equinox launcher jar"), "{}", text[2]);
    }

    #[test]
    fn should_find_the_newest_launcher_jar_when_the_install_is_verified() {
        // given
        let guard = scratch("verified");
        let home = &guard.0;
        std::fs::create_dir_all(home.join("plugins")).unwrap();
        std::fs::write(home.join(".verified"), format!("{JDTLS_SHA256}\n")).unwrap();
        for name in [
            "org.eclipse.equinox.launcher_1.6.0.jar",
            "org.eclipse.equinox.launcher_1.7.0.jar",
            "org.eclipse.equinox.launcher.gtk_1.9.0.jar.txt",
        ] {
            std::fs::write(home.join("plugins").join(name), "jar").unwrap();
        }
        // when
        let jar = launcher(home).unwrap();
        // then
        assert!(
            jar.ends_with("plugins/org.eclipse.equinox.launcher_1.7.0.jar"),
            "{}",
            jar.display()
        );
    }

    #[test]
    fn should_empty_the_directory_when_a_run_makes_it_fresh() {
        // given: a workspace left by an earlier run
        let guard = scratch("fresh");
        let dir = &guard.0;
        let data = dir.join("jdtls-data");
        std::fs::create_dir_all(data.join(".metadata")).unwrap();
        std::fs::write(data.join(".metadata/old.index"), "stale").unwrap();
        // when
        fresh_dir(&data).unwrap();
        // then
        assert!(data.is_dir());
        assert_eq!(std::fs::read_dir(&data).unwrap().count(), 0);
    }
}
