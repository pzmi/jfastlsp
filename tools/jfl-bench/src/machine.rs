//! The machine facts every result carries: a number without its machine is not a measurement
//! (ver §10 rule 1). CPU, RAM, OS, disk, toolchain, tool versions and the commit measured.
//!
//! Each fact is read from `/proc` and `/etc` where Linux has them and from a short argv-only
//! command otherwise (`sysctl`, `sw_vers`, `df`); a fact that cannot be read is `"unknown"` or
//! `null`, never a guess. The host name is deliberately not recorded: results are committed and
//! published, and a name identifies a person's machine (the public-repository rule).
//!
//! Pinned by: this module's tests (the parsers, on captured real output).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

/// The disk the corpus lives on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Disk {
    /// The device or filesystem name `df` reports.
    pub filesystem: String,
    /// Size of the filesystem in GiB.
    pub size_gib: u64,
}

/// How the measuring binary itself was built. A debug build writes plausible-looking numbers that
/// are 10-50 times off, so every report carries this and `run` refuses a debug build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Build {
    /// The profile directory the executable lives in (`bench-tool`, `release`, `debug`).
    pub profile: String,
    /// Whether the binary was built with debug assertions (the `dev` profile's default).
    pub debug_assertions: bool,
    /// The toolchain channel pinned by `rust-toolchain.toml` when this binary was compiled
    /// (embedded at build time: the rustc that built it, not whatever `rustc` is on `PATH` now).
    pub toolchain: String,
}

impl Build {
    /// The build of the running binary.
    pub fn current() -> Self {
        let profile = std::env::current_exe()
            .ok()
            .and_then(|p| {
                p.parent()
                    .and_then(|d| d.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "unknown".to_string());
        Self {
            profile,
            debug_assertions: cfg!(debug_assertions),
            toolchain: parse_channel(PINNED_TOOLCHAIN).unwrap_or_else(|| "unknown".to_string()),
        }
    }
}

const PINNED_TOOLCHAIN: &str = include_str!("../../../rust-toolchain.toml");

/// The `channel` of a `rust-toolchain.toml`.
pub fn parse_channel(text: &str) -> Option<String> {
    text.lines()
        .find_map(|l| l.trim().strip_prefix("channel"))
        .and_then(|rest| rest.split_once('='))
        .map(|(_, v)| v.trim().trim_matches('"').to_string())
        .filter(|v| !v.is_empty())
}

/// What a run executed on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    /// CPU model string.
    pub cpu_model: String,
    /// Logical CPUs the process may use.
    pub logical_cpus: u64,
    /// Physical memory in MiB.
    pub ram_mib: Option<u64>,
    /// Operating system, as the distribution or `sw_vers` names it.
    pub os: String,
    /// Kernel name and release (`uname -sr`).
    pub kernel: String,
    /// CPU architecture (`x86_64`, `aarch64`).
    pub arch: String,
    /// The disk holding the corpus.
    pub disk: Option<Disk>,
    /// Versions of the tools the run drove (`hyperfine`, `rg`, `git`).
    pub tools: BTreeMap<String, String>,
    /// The commit of this repository that was measured.
    pub commit: String,
    /// Whether that checkout had uncommitted changes.
    pub dirty: bool,
}

/// The first `model name` of `/proc/cpuinfo`.
pub fn parse_cpuinfo(text: &str) -> Option<String> {
    text.lines()
        .find_map(|l| l.strip_prefix("model name"))
        .and_then(|rest| rest.split_once(':'))
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// `MemTotal` of `/proc/meminfo` in MiB.
pub fn parse_meminfo_mib(text: &str) -> Option<u64> {
    let kib: u64 = text
        .lines()
        .find_map(|l| l.strip_prefix("MemTotal:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    Some(kib / 1024)
}

/// The filesystem and size of the line `df -Pk <path>` prints for `path`.
pub fn parse_df(text: &str) -> Option<Disk> {
    let line = text.lines().nth(1)?;
    let mut cols = line.split_whitespace();
    let filesystem = cols.next()?.to_string();
    let kib: u64 = cols.next()?.parse().ok()?;
    Some(Disk {
        filesystem,
        size_gib: kib / (1024 * 1024),
    })
}

/// `PRETTY_NAME` of an `os-release` file.
pub fn parse_os_release(text: &str) -> Option<String> {
    text.lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))
        .map(|v| v.trim().trim_matches('"').to_string())
        .filter(|v| !v.is_empty())
}

/// The first executable file named `program` on `PATH`.
pub fn find_on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|p| p.is_file())
}

/// Whether `path` is a version manager's shim (mise, asdf, rbenv: a `shims` directory). A shim
/// starts a launcher before the real program, which adds milliseconds and a launcher's memory to
/// every timed run, so the runner says so when a measured tool resolves to one.
pub fn is_shim(path: &Path) -> bool {
    path.parent()
        .is_some_and(|dir| dir.file_name().is_some_and(|n| n == "shims"))
}

fn command_text(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

fn first_line(s: String) -> String {
    s.lines().next().unwrap_or("").trim().to_string()
}

/// Read the facts of this machine. `corpus_path` locates the disk; `repo_root` the commit.
pub fn collect(repo_root: &Path, corpus_path: &Path, tools: &[&str]) -> Machine {
    let cpu_model = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|t| parse_cpuinfo(&t))
        .or_else(|| command_text("sysctl", &["-n", "machdep.cpu.brand_string"]))
        .unwrap_or_else(|| "unknown".to_string());
    let ram_mib = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|t| parse_meminfo_mib(&t))
        .or_else(|| {
            command_text("sysctl", &["-n", "hw.memsize"])
                .and_then(|s| s.parse::<u64>().ok())
                .map(|b| b / (1024 * 1024))
        });
    let os = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|t| parse_os_release(&t))
        .or_else(|| {
            command_text("sw_vers", &["-productName"]).map(|n| {
                let v = command_text("sw_vers", &["-productVersion"]).unwrap_or_default();
                format!("{n} {v}").trim().to_string()
            })
        })
        .unwrap_or_else(|| std::env::consts::OS.to_string());
    let kernel = command_text("uname", &["-sr"]).unwrap_or_else(|| "unknown".to_string());
    let disk = Command::new("df")
        .args(["-Pk"])
        .arg(corpus_path)
        .output()
        .ok()
        .and_then(|o| parse_df(&String::from_utf8_lossy(&o.stdout)));
    let tools = tools
        .iter()
        .map(|t| {
            let v = command_text(t, &["--version"])
                .map(first_line)
                .unwrap_or_else(|| "not found".to_string());
            ((*t).to_string(), v)
        })
        .collect();
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
    };
    let commit = git(&["rev-parse", "HEAD"])
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let dirty = git(&["status", "--porcelain"]).is_some_and(|o| !o.stdout.is_empty());
    Machine {
        cpu_model,
        logical_cpus: std::thread::available_parallelism().map_or(1, |n| n.get() as u64),
        ram_mib,
        os,
        kernel,
        arch: std::env::consts::ARCH.to_string(),
        disk,
        tools,
        commit,
        dirty,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_read_the_cpu_model_when_cpuinfo_has_one() {
        // given
        let text = "processor\t: 0\nvendor_id\t: GenuineIntel\nmodel name\t: Intel(R) Xeon(R) Processor @ 2.30GHz\ncpu MHz\t\t: 2300.000\n";
        // when
        // then
        assert_eq!(
            parse_cpuinfo(text).as_deref(),
            Some("Intel(R) Xeon(R) Processor @ 2.30GHz")
        );
        assert_eq!(parse_cpuinfo("processor\t: 0\nCPU part\t: 0xd0c\n"), None);
    }

    #[test]
    fn should_convert_to_mib_when_meminfo_reports_kib() {
        // given
        let text = "MemTotal:       16384000 kB\nMemFree:         1000 kB\n";
        // when
        // then
        assert_eq!(parse_meminfo_mib(text), Some(16000));
    }

    #[test]
    fn should_read_the_second_line_when_df_prints_a_header() {
        // given
        let text = "Filesystem     1024-blocks      Used Available Capacity Mounted on\n/dev/vda1         62914560  20971520  41943040      34% /\n";
        // when
        let disk = parse_df(text).unwrap();
        // then
        assert_eq!(
            disk,
            Disk {
                filesystem: "/dev/vda1".to_string(),
                size_gib: 60
            }
        );
        assert_eq!(parse_df("Filesystem\n"), None);
    }

    #[test]
    fn should_recognise_a_shim_when_the_program_lives_in_a_shims_directory() {
        // given
        let shim = Path::new("/home/u/.local/share/mise/shims/rg");
        let real = Path::new("/home/u/.local/share/mise/installs/ripgrep/15.2.0/rg");
        // when
        let (shim_is, real_is) = (is_shim(shim), is_shim(real));
        // then
        assert!(shim_is);
        assert!(!real_is);
    }

    #[test]
    fn should_read_the_channel_when_the_toolchain_file_pins_one() {
        // given
        let text = "[toolchain]\nchannel = \"1.97.0\"\ncomponents = [\"clippy\"]\n";
        // when
        let channel = parse_channel(text);
        // then
        assert_eq!(channel.as_deref(), Some("1.97.0"));
        assert_eq!(parse_channel("[toolchain]\n"), None);
    }

    #[test]
    fn should_say_a_debug_build_is_one_when_tests_run_with_debug_assertions() {
        // given
        // when
        let build = Build::current();
        // then
        assert_eq!(build.debug_assertions, cfg!(debug_assertions));
        assert!(!build.toolchain.is_empty() && build.toolchain != "unknown");
    }

    #[test]
    fn should_strip_quotes_when_os_release_quotes_the_name() {
        // given
        let text = "NAME=\"Ubuntu\"\nPRETTY_NAME=\"Ubuntu 24.04.2 LTS\"\nID=ubuntu\n";
        // when
        // then
        assert_eq!(
            parse_os_release(text).as_deref(),
            Some("Ubuntu 24.04.2 LTS")
        );
    }
}
