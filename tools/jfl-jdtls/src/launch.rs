//! The state directory's layout and the command line of one backend per worktree root.
//!
//! jdtls is started the way its own `bin/jdtls` launcher does (the equinox launcher jar and the
//! read-only shared configuration of the install), with what that launcher leaves out:
//! `-data` keyed on the full worktree path ([`crate::worktree::data_key`]), one
//! `-Djdt.core.sharedIndexLocation` for every worktree's library indexes, a heap cap, a
//! writable `-configuration` in the state directory, and the syntax-server mode on request.
//! jdtls (EPL-2.0) is only ever a separate process, never linked or bundled (ADR 0028).

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use crate::worktree::data_key;

/// The pinned jdtls release; `scripts/jdtls-fetch.sh` pins the same version and its checksum,
/// and `scripts/test-jdtls-fetch.sh` checks that the two agree.
pub const JDTLS_VERSION: &str = "1.61.0";

/// The longest socket path `bind` accepts: `sun_path` is 108 bytes on Linux and 104 on macOS,
/// one of them the terminating NUL.
#[cfg(target_os = "linux")]
pub const MAX_SOCKET_PATH: usize = 107;
/// The longest socket path `bind` accepts: `sun_path` is 108 bytes on Linux and 104 on macOS,
/// one of them the terminating NUL.
#[cfg(not(target_os = "linux"))]
pub const MAX_SOCKET_PATH: usize = 103;

/// Environment variables a backend inherits; everything else is cleared.
pub const ENV_ALLOWLIST: &[&str] = &[
    "PATH",
    "HOME",
    "JAVA_HOME",
    "JAVA_TOOL_OPTIONS",
    "LANG",
    "LC_ALL",
    "TMPDIR",
    "XDG_CACHE_HOME",
    "XDG_STATE_HOME",
];

/// Paths inside the daemon's state directory (`0700`, the access control).
#[derive(Debug, Clone)]
pub struct StateLayout {
    root: PathBuf,
    short_base: PathBuf,
}

impl StateLayout {
    /// A layout rooted at `root`, with `/tmp` as the base of the short socket directory.
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            short_base: PathBuf::from("/tmp"),
        }
    }

    /// The same layout with another base for the short socket directory
    /// (`--short-socket-base`; the tests use one of their own).
    #[must_use]
    pub fn with_short_base(mut self, base: PathBuf) -> Self {
        self.short_base = base;
        self
    }

    /// The same layout rooted at `root` (the canonical state directory, once it exists).
    #[must_use]
    pub fn with_root(mut self, root: PathBuf) -> Self {
        self.root = root;
        self
    }

    /// The state directory itself.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The Unix socket the proxies connect to: `<state>/sock` when that fits a socket address
    /// ([`MAX_SOCKET_PATH`]), else `<short dir>/<key>.sock` in [`StateLayout::short_socket_dir`],
    /// keyed on the state directory (canonical once `daemon::prepare_state_dir` has returned the
    /// layout, so aliases of one directory find one daemon) so several state directories never
    /// share a socket. Pinned by
    /// `tests::should_move_the_socket_to_a_short_directory_when_the_state_path_is_long` and
    /// `tests/daemon.rs::daemon_or_jvm::should_serve_through_a_short_socket_when_the_state_path_is_long`.
    #[must_use]
    pub fn socket(&self) -> PathBuf {
        let direct = self.root.join("sock");
        if direct.as_os_str().len() <= MAX_SOCKET_PATH {
            return direct;
        }
        let key = data_key(&self.root);
        self.short_socket_dir()
            .join(format!("{}.sock", key.get(..16).unwrap_or(&key)))
    }

    /// Where a socket goes when `<state>/sock` is too long: `<base>/jfl-jdtls-<euid>`, with `/tmp`
    /// as the base by default, a fixed short path (not `$TMPDIR`, which may itself be long), held
    /// to the state directory's rules (`0700`, owned by the effective uid) by
    /// `daemon::prepare_state_dir`.
    #[must_use]
    pub fn short_socket_dir(&self) -> PathBuf {
        self.short_base
            .join(format!("jfl-jdtls-{}", rustix::process::geteuid().as_raw()))
    }

    /// The lock the one daemon of this state directory holds for its lifetime.
    #[must_use]
    pub fn daemon_lock(&self) -> PathBuf {
        self.root.join("daemon.lock")
    }

    /// The lock that serialises "connect, or start the daemon" and the daemon's idle exit.
    #[must_use]
    pub fn startup_lock(&self) -> PathBuf {
        self.root.join("startup.lock")
    }

    /// Log files (the daemon's and each backend's stderr).
    #[must_use]
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// The `-data` directory of a worktree root (`suffix` separates the syntax server's).
    #[must_use]
    pub fn workspace(&self, worktree_root: &Path, suffix: &str) -> PathBuf {
        self.root
            .join("workspaces")
            .join(format!("{}{suffix}", data_key(worktree_root)))
    }

    /// The library index directory every jdtls of this daemon shares.
    #[must_use]
    pub fn shared_index(&self) -> PathBuf {
        self.root.join("shared-index")
    }

    /// The writable OSGi configuration area for one jdtls version and mode.
    #[must_use]
    pub fn jdtls_configuration(&self, syntax_server: bool) -> PathBuf {
        let mode = if syntax_server { "-ss" } else { "" };
        self.root
            .join("jdtls-configuration")
            .join(format!("{JDTLS_VERSION}{mode}"))
    }
}

/// A backend's program, arguments and working directory; its environment is
/// [`ENV_ALLOWLIST`] only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendCommand {
    /// The executable.
    pub program: OsString,
    /// Its arguments.
    pub args: Vec<OsString>,
    /// Its working directory (the worktree root).
    pub cwd: PathBuf,
    /// The `-data` directory given to it.
    pub data_dir: PathBuf,
}

/// How backends are started.
#[derive(Debug, Clone)]
pub enum BackendSpec {
    /// The pinned jdtls.
    Jdtls(JdtlsLaunch),
    /// Any language server, given `-data <dir>` after its own arguments (the scripted fake in
    /// tests, or another server fronted the same way).
    Command {
        /// The executable.
        program: OsString,
        /// Its own arguments.
        args: Vec<OsString>,
    },
}

impl BackendSpec {
    /// The command for one worktree root.
    ///
    /// # Errors
    ///
    /// For jdtls, the error of finding its launcher jar.
    pub fn command(
        &self,
        layout: &StateLayout,
        worktree_root: &Path,
    ) -> io::Result<BackendCommand> {
        match self {
            Self::Jdtls(launch) => launch.command(layout, worktree_root),
            Self::Command { program, args } => {
                let data_dir = layout.workspace(worktree_root, "");
                let mut all = args.clone();
                all.push("-data".into());
                all.push(data_dir.clone().into_os_string());
                Ok(BackendCommand {
                    program: program.clone(),
                    args: all,
                    cwd: worktree_root.to_path_buf(),
                    data_dir,
                })
            }
        }
    }
}

/// The jdtls launch settings.
#[derive(Debug, Clone)]
pub struct JdtlsLaunch {
    /// The `java` executable (Java 21 or newer).
    pub java: PathBuf,
    /// The extracted jdtls release (holds `plugins/` and `config_*`).
    pub home: PathBuf,
    /// The heap cap, `-Xmx`, in MiB.
    pub heap_mib: u32,
    /// Run jdtls in syntax-server mode (no project import; syntax answers only).
    pub syntax_server: bool,
}

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const CONFIG_DIR: &str = "linux_arm";
#[cfg(all(target_os = "linux", not(target_arch = "aarch64")))]
const CONFIG_DIR: &str = "linux";
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const CONFIG_DIR: &str = "mac_arm";
#[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
const CONFIG_DIR: &str = "mac";

impl JdtlsLaunch {
    /// The command for one worktree root, after finding the equinox launcher jar.
    ///
    /// # Errors
    ///
    /// When `home/plugins` cannot be read or holds no launcher jar.
    pub fn command(
        &self,
        layout: &StateLayout,
        worktree_root: &Path,
    ) -> io::Result<BackendCommand> {
        let launcher = find_launcher(&self.home)?;
        Ok(self.command_with_launcher(&launcher, layout, worktree_root))
    }

    /// The command line, given the launcher jar. Pinned by
    /// `tests::should_key_data_on_the_full_path_and_share_the_index_when_built`.
    #[must_use]
    pub fn command_with_launcher(
        &self,
        launcher: &Path,
        layout: &StateLayout,
        worktree_root: &Path,
    ) -> BackendCommand {
        let (config, suffix) = if self.syntax_server {
            (format!("config_ss_{CONFIG_DIR}"), "-ss")
        } else {
            (format!("config_{CONFIG_DIR}"), "")
        };
        let data_dir = layout.workspace(worktree_root, suffix);
        let shared_config = self.home.join(config);
        let initial_heap = self.heap_mib.min(1024);
        let mut args: Vec<OsString> = vec![
            "-Declipse.application=org.eclipse.jdt.ls.core.id1".into(),
            "-Dosgi.bundles.defaultStartLevel=4".into(),
            "-Declipse.product=org.eclipse.jdt.ls.core.product".into(),
            "-Dosgi.checkConfiguration=true".into(),
            prefixed("-Dosgi.sharedConfiguration.area=", &shared_config),
            "-Dosgi.sharedConfiguration.area.readOnly=true".into(),
            "-Dosgi.configuration.cascaded=true".into(),
            prefixed("-Djdt.core.sharedIndexLocation=", &layout.shared_index()),
            // vscode-java's defaults; harmless properties on JVMs that do not read them.
            "-Dsun.zip.disableMemoryMapping=true".into(),
            "-Djdk.xml.maxGeneralEntitySizeLimit=0".into(),
            "-Djdk.xml.totalEntitySizeLimit=0".into(),
            "-XX:+UseParallelGC".into(),
            "-XX:GCTimeRatio=4".into(),
            "-XX:AdaptiveSizePolicyWeight=90".into(),
            format!("-Xms{initial_heap}m").into(),
            format!("-Xmx{}m", self.heap_mib).into(),
        ];
        if self.syntax_server {
            args.push("-Dsyntaxserver=true".into());
        }
        args.extend([
            "--add-modules=ALL-SYSTEM".into(),
            "--add-opens".into(),
            "java.base/java.util=ALL-UNNAMED".into(),
            "--add-opens".into(),
            "java.base/java.lang=ALL-UNNAMED".into(),
            "-jar".into(),
            launcher.as_os_str().to_owned(),
            "-configuration".into(),
            layout
                .jdtls_configuration(self.syntax_server)
                .into_os_string(),
            "-data".into(),
            data_dir.clone().into_os_string(),
        ]);
        BackendCommand {
            program: self.java.clone().into_os_string(),
            args,
            cwd: worktree_root.to_path_buf(),
            data_dir,
        }
    }
}

fn prefixed(prefix: &str, path: &Path) -> OsString {
    let mut out = OsString::from(prefix);
    out.push(path.as_os_str());
    out
}

/// The `org.eclipse.equinox.launcher_*.jar` of a jdtls install (the platform-neutral one).
///
/// # Errors
///
/// When `plugins/` cannot be read or holds no such jar.
pub fn find_launcher(home: &Path) -> io::Result<PathBuf> {
    let plugins = home.join("plugins");
    let mut found: Option<PathBuf> = None;
    for entry in std::fs::read_dir(&plugins)? {
        let name = entry?.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("org.eclipse.equinox.launcher_") && name.ends_with(".jar") {
            let path = plugins.join(name.as_ref());
            if found.as_ref().is_none_or(|f| path > *f) {
                found = Some(path);
            }
        }
    }
    found.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "no equinox launcher jar under {}: run scripts/jdtls-fetch.sh",
                plugins.display()
            ),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch(syntax_server: bool) -> JdtlsLaunch {
        JdtlsLaunch {
            java: "java".into(),
            home: "/opt/jdtls".into(),
            heap_mib: 2048,
            syntax_server,
        }
    }

    fn arg_after<'a>(cmd: &'a BackendCommand, flag: &str) -> &'a OsString {
        cmd.args.iter().skip_while(|a| *a != flag).nth(1).unwrap()
    }

    #[test]
    fn should_key_data_on_the_full_path_and_share_the_index_when_built() {
        // given: two worktrees with the same basename
        let layout = StateLayout::new("/state".into());
        let jar = Path::new("/opt/jdtls/plugins/org.eclipse.equinox.launcher_1.jar");

        // when
        let a = launch(false).command_with_launcher(jar, &layout, Path::new("/w/one/repo"));
        let b = launch(false).command_with_launcher(jar, &layout, Path::new("/w/two/repo"));

        // then
        assert_ne!(arg_after(&a, "-data"), arg_after(&b, "-data"));
        assert_eq!(a.data_dir.as_os_str(), arg_after(&a, "-data"));
        assert!(a.data_dir.starts_with("/state/workspaces"));
        let index = OsString::from("-Djdt.core.sharedIndexLocation=/state/shared-index");
        assert!(a.args.contains(&index) && b.args.contains(&index));
        assert!(a.args.contains(&OsString::from("-Xmx2048m")));
        assert_eq!(arg_after(&a, "-jar"), jar.as_os_str());
        assert_eq!(a.cwd, Path::new("/w/one/repo"));
    }

    #[test]
    fn should_use_the_syntax_configuration_and_own_data_when_syntax_server() {
        // given
        let layout = StateLayout::new("/state".into());
        let jar = Path::new("/j.jar");
        let root = Path::new("/w/repo");

        // when
        let full = launch(false).command_with_launcher(jar, &layout, root);
        let syntax = launch(true).command_with_launcher(jar, &layout, root);

        // then
        assert!(syntax.args.contains(&OsString::from("-Dsyntaxserver=true")));
        assert!(!full.args.contains(&OsString::from("-Dsyntaxserver=true")));
        assert_ne!(full.data_dir, syntax.data_dir);
        let shared = |c: &BackendCommand| {
            c.args
                .iter()
                .any(|a| a.to_string_lossy().contains("/opt/jdtls/config_ss_"))
        };
        assert!(shared(&syntax) && !shared(&full));
    }

    #[test]
    fn should_move_the_socket_to_a_short_directory_when_the_state_path_is_long() {
        // given
        let short = StateLayout::new("/home/u/.local/state/jfl-jdtls".into());
        let long_root = PathBuf::from(format!("/home/u/{}/jfl-jdtls", "d".repeat(120)));
        let long = StateLayout::new(long_root.clone());
        let other = StateLayout::new(long_root.join("other"));

        // when
        let (s, l, o) = (short.socket(), long.socket(), other.socket());

        // then
        assert_eq!(s, Path::new("/home/u/.local/state/jfl-jdtls/sock"));
        assert!(l.starts_with(long.short_socket_dir()), "{}", l.display());
        assert!(l.as_os_str().len() <= MAX_SOCKET_PATH);
        assert!(l.as_os_str().len() <= 103, "fits macOS too");
        assert_ne!(l, o);
    }

    #[test]
    fn should_append_data_when_backend_is_a_plain_command() {
        // given
        let layout = StateLayout::new("/state".into());
        let spec = BackendSpec::Command {
            program: "fake".into(),
            args: vec!["--x".into()],
        };

        // when
        let cmd = spec.command(&layout, Path::new("/w/repo")).unwrap();

        // then
        assert_eq!(cmd.args[0], "--x");
        assert_eq!(arg_after(&cmd, "-data"), cmd.data_dir.as_os_str());
    }
}
