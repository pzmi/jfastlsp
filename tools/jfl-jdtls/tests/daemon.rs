//! Integration tests: the built `jfl-jdtls lsp` proxies, the daemon they start, and the
//! scripted fake backend (`jfl-jdtls-fake-lsp`) as real processes over a real Unix socket.
//! They start a daemon, so they live in `daemon_or_jvm` (the nextest group of that name).

mod daemon_or_jvm {
    use std::error::Error;
    use std::io::{BufReader, Write};
    use std::path::{Path, PathBuf};
    use std::process::{Child, ChildStdin, Command, Stdio};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::mpsc::{Receiver, sync_channel};
    use std::thread;
    use std::time::Duration;

    use jfl_jdtls::framing::{MAX_FRAME_BYTES, read_frame, write_frame};
    use jfl_jdtls::json::Value;
    use jfl_jdtls::launch::{MAX_SOCKET_PATH, StateLayout};

    type Res<T> = Result<T, Box<dyn Error>>;

    const WAIT: Duration = Duration::from_secs(30);
    const EXE: &str = env!("CARGO_BIN_EXE_jfl-jdtls");
    const FAKE: &str = env!("CARGO_BIN_EXE_jfl-jdtls-fake-lsp");

    /// A scratch directory under the system temp directory (short, for the socket path),
    /// removed on drop; the daemon of its state directory is stopped first.
    struct Scratch {
        dir: PathBuf,
    }

    impl Scratch {
        fn new(name: &str) -> Res<Self> {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("jj-{}-{n}-{name}", std::process::id()));
            if dir.exists() {
                std::fs::remove_dir_all(&dir)?;
            }
            std::fs::create_dir_all(&dir)?;
            Ok(Self {
                dir: dir.canonicalize()?,
            })
        }

        fn state(&self) -> PathBuf {
            self.dir.join("s")
        }

        /// A worktree (a directory with a `.git` directory) at `rel`.
        fn worktree(&self, rel: &str) -> Res<PathBuf> {
            let root = self.dir.join(rel);
            std::fs::create_dir_all(root.join(".git"))?;
            Ok(root)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = ctl(&self.state(), "stop");
            let _ = std::fs::remove_dir_all(&self.dir);
            let _ = std::fs::remove_dir_all(short_base());
        }
    }

    /// Stops the daemon of a state directory the scratch does not know about, on drop (so a
    /// failing assertion still leaves nothing running).
    struct StopOnDrop(PathBuf);

    impl Drop for StopOnDrop {
        fn drop(&mut self) {
            let _ = ctl(&self.0, "stop");
        }
    }

    /// This test process's base for the short socket directory: short whatever `$TMPDIR` is,
    /// and removed with the scratch, so no test leaves `/tmp/jfl-jdtls-<uid>` behind.
    fn short_base() -> PathBuf {
        PathBuf::from(format!("/tmp/jjs-{}", std::process::id()))
    }

    /// The socket a state directory's daemon listens on, as the tool computes it.
    fn socket_of(state: &Path) -> Res<PathBuf> {
        Ok(StateLayout::new(state.canonicalize()?)
            .with_short_base(short_base())
            .socket())
    }

    /// `jfl-jdtls <op> --state-dir <state>`, its stdout as JSON.
    fn ctl(state: &Path, op: &str) -> Res<Value> {
        let out = Command::new(EXE)
            .arg(op)
            .arg("--state-dir")
            .arg(state)
            .arg("--short-socket-base")
            .arg(short_base())
            .output()?;
        if !out.status.success() {
            return Err(format!("{op} failed: {}", String::from_utf8_lossy(&out.stderr)).into());
        }
        Ok(serde_json::from_slice(out.stdout.trim_ascii())?)
    }

    fn alive(pid: &Value) -> Res<bool> {
        let Value::Number(n) = pid else {
            return Err("no pid".into());
        };
        Ok(Command::new("kill")
            .arg("-0")
            .arg(n.to_string())
            .stderr(Stdio::null())
            .status()?
            .success())
    }

    /// One `jfl-jdtls lsp` process, as an LSP client would run it.
    struct Proxy {
        child: Child,
        stdin: Option<ChildStdin>,
        inbox: Receiver<Value>,
    }

    impl Proxy {
        fn start(state: &Path, root: &Path) -> Res<Self> {
            let mut child = Command::new(EXE)
                .arg("lsp")
                .arg("--short-socket-base")
                .arg(short_base())
                .arg("--state-dir")
                .arg(state)
                .arg("--root")
                .arg(root)
                .args([
                    "--idle-secs",
                    "30",
                    "--daemon-idle-secs",
                    "30",
                    "--backend-command",
                    FAKE,
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()?;
            let stdin = child.stdin.take().ok_or("no stdin")?;
            let stdout = child.stdout.take().ok_or("no stdout")?;
            let (tx, inbox) = sync_channel(256);
            thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                while let Ok(Some(body)) = read_frame(&mut reader, MAX_FRAME_BYTES) {
                    let Ok(msg) = serde_json::from_slice::<Value>(&body) else {
                        return;
                    };
                    if tx.send(msg).is_err() {
                        return;
                    }
                }
            });
            Ok(Self {
                child,
                stdin: Some(stdin),
                inbox,
            })
        }

        fn send(&mut self, text: &str) -> Res<()> {
            let stdin = self.stdin.as_mut().ok_or("stdin closed")?;
            write_frame(stdin, text.as_bytes())?;
            Ok(stdin.flush()?)
        }

        fn next(&self) -> Res<Value> {
            Ok(self.inbox.recv_timeout(WAIT)?)
        }

        fn next_response(&self) -> Res<Value> {
            loop {
                let msg = self.next()?;
                if msg.get("id").is_some() {
                    return Ok(msg);
                }
            }
        }

        /// The next notification of `method`, skipping everything else.
        fn notification(&self, method: &str) -> Res<Value> {
            loop {
                let msg = self.next()?;
                if msg.get("id").is_none()
                    && msg.get("method").and_then(Value::as_str) == Some(method)
                {
                    return Ok(msg);
                }
            }
        }

        fn initialize(&mut self) -> Res<()> {
            self.send(
                r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"processId":null}}"#,
            )?;
            let reply = self.next_response()?;
            if reply.get("result").is_none() {
                return Err(format!("initialize failed: {reply:?}").into());
            }
            self.send(r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#)
        }

        fn result(&mut self, request: &str) -> Res<Value> {
            self.send(request)?;
            let reply = self.next_response()?;
            reply
                .get("result")
                .cloned()
                .ok_or_else(|| format!("no result: {reply:?}").into())
        }

        fn stats(&mut self) -> Res<Value> {
            self.result(r#"{"jsonrpc":"2.0","id":"stats","method":"fake/stats"}"#)
        }

        /// Ends the session the way an LSP client does, and waits for the process.
        fn close(mut self) -> Res<()> {
            self.send(r#"{"jsonrpc":"2.0","id":"bye","method":"shutdown"}"#)?;
            self.next_response()?;
            self.send(r#"{"jsonrpc":"2.0","method":"exit"}"#)?;
            self.stdin = None;
            let status = self.child.wait()?;
            if status.success() {
                Ok(())
            } else {
                Err(format!("proxy exited with {status}").into())
            }
        }
    }

    impl Drop for Proxy {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn sessions(state: &Path) -> Res<Vec<Value>> {
        match ctl(state, "status")?.get("sessions") {
            Some(Value::Array(s)) => Ok(s.clone()),
            other => Err(format!("no sessions in status: {other:?}").into()),
        }
    }

    #[test]
    fn should_share_one_backend_with_answers_under_each_clients_ids_when_two_proxies_share_a_worktree()
    -> Res<()> {
        // given
        let scratch = Scratch::new("share")?;
        let root = scratch.worktree("repo")?;
        let mut a = Proxy::start(&scratch.state(), &root)?;
        let mut b = Proxy::start(&scratch.state(), &root)?;
        a.initialize()?;
        b.initialize()?;

        // when: the same ids from both clients
        a.send(r#"{"jsonrpc":"2.0","id":1,"method":"fake/echo","params":{"from":"a"}}"#)?;
        b.send(r#"{"jsonrpc":"2.0","id":1,"method":"fake/echo","params":{"from":"b"}}"#)?;
        let (ra, rb) = (a.next_response()?, b.next_response()?);

        // then: one backend, initialized once, and each answer reached its own client
        assert_eq!(ra.get("id"), Some(&Value::from(1)));
        assert_eq!(
            ra.get("result")
                .unwrap()
                .get("from")
                .and_then(Value::as_str),
            Some("a")
        );
        assert_eq!(rb.get("id"), Some(&Value::from(1)));
        assert_eq!(
            rb.get("result")
                .unwrap()
                .get("from")
                .and_then(Value::as_str),
            Some("b")
        );
        let stats = a.stats()?;
        assert_eq!(stats.get("initializeCount"), Some(&Value::from(1)));
        let sessions = sessions(&scratch.state())?;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].get("clients"), Some(&Value::from(2)));
        let backend = sessions[0].get("backendPid").cloned().unwrap();
        assert_eq!(stats.get("pid"), Some(&backend));

        // and when both leave and the daemon is stopped, the backend is gone with it
        a.close()?;
        b.close()?;
        let socket = socket_of(&scratch.state())?;
        assert_eq!(
            ctl(&scratch.state(), "stop")?.get("ok"),
            Some(&Value::Bool(true))
        );
        assert!(!alive(&backend)?);
        assert!(!socket.exists());
        Ok(())
    }

    #[test]
    fn should_give_different_data_dirs_when_worktrees_share_a_basename() -> Res<()> {
        // given: two worktrees named alike
        let scratch = Scratch::new("basename")?;
        let one = scratch.worktree("one/repo")?;
        let two = scratch.worktree("two/repo")?;
        let mut a = Proxy::start(&scratch.state(), &one)?;
        let mut b = Proxy::start(&scratch.state(), &two)?;

        // when
        a.initialize()?;
        b.initialize()?;
        let (sa, sb) = (a.stats()?, b.stats()?);

        // then: two backends, each in its own worktree with its own -data
        let data = |s: &Value| -> Option<String> {
            let Value::Array(argv) = s.get("argv")? else {
                return None;
            };
            argv.iter()
                .skip_while(|a| **a != Value::String("-data".into()))
                .nth(1)
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        let (da, db) = (data(&sa).unwrap(), data(&sb).unwrap());
        assert_ne!(da, db);
        assert!(da.starts_with(&scratch.state().to_string_lossy().into_owned()));
        assert_eq!(
            sa.get("cwd").and_then(Value::as_str),
            Some(one.to_string_lossy().as_ref())
        );
        assert_eq!(
            sb.get("cwd").and_then(Value::as_str),
            Some(two.to_string_lossy().as_ref())
        );
        assert_ne!(sa.get("pid"), sb.get("pid"));
        let sessions = sessions(&scratch.state())?;
        assert_eq!(sessions.len(), 2);
        let dirs: Vec<_> = sessions
            .iter()
            .filter_map(|s| s.get("dataDir").and_then(Value::as_str))
            .collect();
        assert!(dirs.contains(&da.as_str()) && dirs.contains(&db.as_str()));
        Ok(())
    }

    #[test]
    fn should_keep_serving_the_others_when_a_proxy_dies_mid_request() -> Res<()> {
        // given: a holds a request the backend has not answered
        let scratch = Scratch::new("exit")?;
        let root = scratch.worktree("repo")?;
        let mut a = Proxy::start(&scratch.state(), &root)?;
        let mut b = Proxy::start(&scratch.state(), &root)?;
        a.initialize()?;
        b.initialize()?;
        a.send(r#"{"jsonrpc":"2.0","id":5,"method":"fake/slow"}"#)?;
        let started = a.notification("fake/slowStarted")?;
        let backend_id = started.get("params").unwrap().get("id").cloned().unwrap();

        // when: a's process is killed
        a.child.kill()?;
        a.child.wait()?;

        // then: the backend got the cancel and keeps serving b
        let cancelled = b.notification("fake/cancelled")?;
        assert_eq!(
            cancelled.get("params").unwrap().get("id"),
            Some(&backend_id)
        );
        let echo =
            b.result(r#"{"jsonrpc":"2.0","id":2,"method":"fake/echo","params":{"from":"b"}}"#)?;
        assert_eq!(echo.get("from").and_then(Value::as_str), Some("b"));
        let sessions = sessions(&scratch.state())?;
        assert_eq!(sessions[0].get("clients"), Some(&Value::from(1)));
        Ok(())
    }

    #[test]
    fn should_keep_the_state_directory_private_when_the_daemon_runs() -> Res<()> {
        // given
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new("perm")?;
        let root = scratch.worktree("repo")?;
        let mut a = Proxy::start(&scratch.state(), &root)?;

        // when
        a.initialize()?;

        // then
        let mode = |p: &Path| std::fs::metadata(p).map(|m| m.permissions().mode() & 0o777);
        assert_eq!(mode(&scratch.state())?, 0o700);
        assert_eq!(mode(&socket_of(&scratch.state())?)?, 0o600);
        assert_eq!(mode(&scratch.state().join("daemon.lock"))?, 0o600);
        assert_eq!(mode(&scratch.state().join("startup.lock"))?, 0o600);
        Ok(())
    }

    /// Runs `jfl-jdtls lsp` against `state` and returns its stderr; it must fail.
    fn refused(state: &Path, root: &Path) -> Res<String> {
        let out = Command::new(EXE)
            .arg("lsp")
            .arg("--short-socket-base")
            .arg(short_base())
            .arg("--state-dir")
            .arg(state)
            .arg("--root")
            .arg(root)
            .args(["--backend-command", FAKE])
            .stdin(Stdio::null())
            .output()?;
        if out.status.success() {
            return Err("lsp accepted the state directory".into());
        }
        Ok(String::from_utf8_lossy(&out.stderr).into_owned())
    }

    #[test]
    fn should_refuse_a_state_directory_when_it_is_not_private() -> Res<()> {
        use std::os::unix::fs::PermissionsExt;
        // given: a 0755 directory, and a symlink to a 0700 one
        let scratch = Scratch::new("refuse")?;
        let root = scratch.worktree("repo")?;
        let open = scratch.dir.join("open");
        std::fs::create_dir(&open)?;
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755))?;
        let private = scratch.dir.join("private");
        std::fs::create_dir(&private)?;
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700))?;
        let link = scratch.dir.join("link");
        std::os::unix::fs::symlink(&private, &link)?;

        // when
        let (open_err, link_err) = (refused(&open, &root)?, refused(&link, &root)?);

        // then: each refusal names the path, and nothing was started in it
        assert!(
            open_err.contains(&*open.to_string_lossy()) && open_err.contains("Mode(493)"),
            "{open_err}"
        );
        assert!(
            link_err.contains(&*link.to_string_lossy()) && link_err.contains("Symlink"),
            "{link_err}"
        );
        assert!(!socket_of(&open)?.exists() && !socket_of(&private)?.exists());
        Ok(())
    }

    #[test]
    fn should_refuse_a_state_directory_when_another_user_owns_it() -> Res<()> {
        use std::os::unix::fs::PermissionsExt;
        // given: a 0700 directory owned by someone else (made so as root; as any other user,
        // the root-owned filesystem root stands in)
        let scratch = Scratch::new("owner")?;
        let root = scratch.worktree("repo")?;
        let state = if rustix::process::geteuid().is_root() {
            let theirs = scratch.dir.join("theirs");
            std::fs::create_dir(&theirs)?;
            std::fs::set_permissions(&theirs, std::fs::Permissions::from_mode(0o700))?;
            std::os::unix::fs::chown(&theirs, Some(65534), Some(65534))?;
            theirs
        } else {
            PathBuf::from("/")
        };

        // when
        let err = refused(&state, &root)?;

        // then
        assert!(err.contains("Owner("), "{err}");
        Ok(())
    }

    #[test]
    fn should_serve_through_a_short_socket_when_the_state_path_is_long() -> Res<()> {
        use std::os::unix::fs::PermissionsExt;
        // given: a state directory whose `sock` would not fit a socket address on any platform
        let scratch = Scratch::new("long")?;
        let root = scratch.worktree("repo")?;
        let state = scratch.dir.join("l".repeat(120)).join("s");
        assert!(state.join("sock").as_os_str().len() > MAX_SOCKET_PATH);
        let _stop = StopOnDrop(state.clone());
        let mut a = Proxy::start(&state, &root)?;

        // when
        a.initialize()?;
        let socket = socket_of(&state)?;
        let echo =
            a.result(r#"{"jsonrpc":"2.0","id":1,"method":"fake/echo","params":{"ok":1}}"#)?;

        // then: served through a short, private socket that the stop removes
        assert_eq!(echo["ok"], 1);
        assert!(
            socket.as_os_str().len() <= MAX_SOCKET_PATH,
            "{}",
            socket.display()
        );
        let mode = |p: &Path| std::fs::metadata(p).map(|m| m.permissions().mode() & 0o777);
        assert_eq!(mode(&socket)?, 0o600);
        assert_eq!(mode(socket.parent().ok_or("no parent")?)?, 0o700);
        assert_eq!(ctl(&state, "stop")?.get("ok"), Some(&Value::Bool(true)));
        assert!(!socket.exists());
        Ok(())
    }

    #[test]
    fn should_find_the_same_daemon_when_a_long_state_directory_is_reached_through_an_alias()
    -> Res<()> {
        // given: a long state directory, and a short symlinked alias of its parent
        let scratch = Scratch::new("alias")?;
        let root = scratch.worktree("repo")?;
        let long_parent = scratch.dir.join("l".repeat(120));
        std::fs::create_dir_all(&long_parent)?;
        let alias = scratch.dir.join("a");
        std::os::unix::fs::symlink(&long_parent, &alias)?;
        let (state, aliased) = (long_parent.join("s"), alias.join("s"));
        let _stop = StopOnDrop(state.clone());
        let mut a = Proxy::start(&state, &root)?;
        a.initialize()?;

        // when: a second client and `status` name the state directory through the alias
        let mut b = Proxy::start(&aliased, &root)?;
        b.initialize()?;
        let sessions = sessions(&aliased)?;

        // then: one daemon, one backend, both clients on it
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].get("clients"), Some(&Value::from(2)));
        assert_eq!(socket_of(&state)?, socket_of(&aliased)?);
        Ok(())
    }

    #[test]
    fn should_refuse_a_short_socket_directory_when_it_is_not_private() -> Res<()> {
        use std::os::unix::fs::PermissionsExt;
        // given: a long state directory, so its socket goes to the short directory, which is
        // pre-created in turn as 0755, as a symlink to a private directory and (as root) as
        // another user's
        let scratch = Scratch::new("shortdir")?;
        let root = scratch.worktree("repo")?;
        let state = scratch.dir.join("l".repeat(120)).join("s");
        std::fs::create_dir_all(short_base())?;
        let short = StateLayout::new(state.clone())
            .with_short_base(short_base())
            .short_socket_dir();
        let private = scratch.dir.join("private");
        std::fs::create_dir(&private)?;
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700))?;
        let mut faults = vec!["Mode(493)", "Symlink"];
        if rustix::process::geteuid().is_root() {
            faults.push("Owner(65534)");
        }

        for fault in faults {
            if fault == "Symlink" {
                std::os::unix::fs::symlink(&private, &short)?;
            } else {
                std::fs::create_dir(&short)?;
                let mode = if fault == "Mode(493)" { 0o755 } else { 0o700 };
                std::fs::set_permissions(&short, std::fs::Permissions::from_mode(mode))?;
                if fault == "Owner(65534)" {
                    std::os::unix::fs::chown(&short, Some(65534), Some(65534))?;
                }
            }

            // when
            let err = refused(&state, &root)?;

            // then: refused, naming the directory and the limit, and no socket was bound in it
            assert!(
                err.contains(fault) && err.contains(&*short.to_string_lossy()),
                "{err}"
            );
            assert!(err.contains("socket path limit"), "{err}");
            assert!(!private.read_dir()?.any(|_| true));
            if std::fs::remove_dir(&short).is_err() {
                std::fs::remove_file(&short)?;
            }
        }
        Ok(())
    }
}
