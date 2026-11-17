//! The real pinned jdtls behind the proxy, on the synthetic single-module fixture with the
//! Gradle import off (never a Gradle import in the sandbox). Built only with the `real-jdtls`
//! feature; needs `bash scripts/jdtls-fetch.sh` first and a Java 21+ `java` on PATH. A missing
//! jdtls fails the test, never skips it.

#![cfg(feature = "real-jdtls")]

mod daemon_or_jvm {
    use std::error::Error;
    use std::io::{BufReader, Write};
    use std::path::{Path, PathBuf};
    use std::process::{Child, ChildStdin, Command, Stdio};
    use std::sync::mpsc::{Receiver, sync_channel};
    use std::thread;
    use std::time::{Duration, Instant};

    use jfl_jdtls::daemon::file_uri;
    use jfl_jdtls::framing::{MAX_FRAME_BYTES, read_frame, write_frame};
    use jfl_jdtls::json::Value;
    use jfl_jdtls::proxy::default_jdtls_home;

    type Res<T> = Result<T, Box<dyn Error>>;

    const EXE: &str = env!("CARGO_BIN_EXE_jfl-jdtls");
    const READY: Duration = Duration::from_secs(100);

    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = Command::new(EXE)
                .arg("stop")
                .arg("--state-dir")
                .arg(self.0.join("s"))
                .output();
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn copy_dir(from: &Path, to: &Path) -> Res<()> {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            let target = to.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                copy_dir(&entry.path(), &target)?;
            } else {
                std::fs::copy(entry.path(), target)?;
            }
        }
        Ok(())
    }

    struct Proxy {
        child: Child,
        stdin: Option<ChildStdin>,
        inbox: Receiver<Value>,
    }

    impl Proxy {
        fn start(state: &Path, root: &Path, home: &Path) -> Res<Self> {
            let mut child = Command::new(EXE)
                .arg("lsp")
                .arg("--state-dir")
                .arg(state)
                .arg("--root")
                .arg(root)
                .arg("--jdtls-home")
                .arg(home)
                .args([
                    "--no-gradle-import",
                    "--heap-mib",
                    "1024",
                    "--idle-secs",
                    "120",
                ])
                .args(["--daemon-idle-secs", "120"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()?;
            let stdin = child.stdin.take();
            let stdout = child.stdout.take().ok_or("no stdout")?;
            let (tx, inbox) = sync_channel(4096);
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
                stdin,
                inbox,
            })
        }

        fn send(&mut self, msg: &str) -> Res<()> {
            let stdin = self.stdin.as_mut().ok_or("stdin closed")?;
            write_frame(stdin, msg.as_bytes())?;
            Ok(stdin.flush()?)
        }

        /// The next message matching `want`, answering server requests with `null` meanwhile.
        fn wait_for(&mut self, deadline: Instant, want: impl Fn(&Value) -> bool) -> Res<Value> {
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                let msg = self.inbox.recv_timeout(left)?;
                if want(&msg) {
                    return Ok(msg);
                }
                if let (Some(id), Some(_)) =
                    (msg.get("id"), msg.get("method").and_then(Value::as_str))
                {
                    let id = serde_json::to_string(id)?;
                    self.send(&format!(r#"{{"jsonrpc":"2.0","id":{id},"result":null}}"#))?;
                }
            }
        }

        fn response(&mut self, id: u64, deadline: Instant) -> Res<Value> {
            self.wait_for(deadline, |m| {
                m.get("id") == Some(&Value::from(id)) && m.get("method").is_none()
            })
        }

        fn initialize(&mut self, deadline: Instant) -> Res<Value> {
            self.send(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":null,"capabilities":{}}}"#)?;
            let reply = self.response(1, deadline)?;
            self.send(r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#)?;
            reply
                .get("result")
                .cloned()
                .ok_or_else(|| format!("initialize: {reply:?}").into())
        }

        fn definition(&mut self, id: u64, uri: &str, deadline: Instant) -> Res<Value> {
            let params = format!(
                r#"{{"textDocument":{{"uri":"{uri}"}},"position":{{"line":4,"character":9}}}}"#
            );
            self.send(&format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"textDocument/definition","params":{params}}}"#))?;
            let reply = self.response(id, deadline)?;
            reply
                .get("result")
                .cloned()
                .ok_or_else(|| format!("definition: {reply:?}").into())
        }

        fn close(mut self, deadline: Instant) -> Res<()> {
            self.send(r#"{"jsonrpc":"2.0","id":99,"method":"shutdown"}"#)?;
            self.response(99, deadline)?;
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

    fn first_uri(result: &Value) -> Option<String> {
        let location = match result {
            Value::Array(items) => items.first()?,
            other => other,
        };
        location
            .get("uri")
            .and_then(Value::as_str)
            .or_else(|| location.get("targetUri").and_then(Value::as_str))
            .map(str::to_owned)
    }

    fn ctl(state: &Path, op: &str) -> Res<Value> {
        let out = Command::new(EXE)
            .arg(op)
            .arg("--state-dir")
            .arg(state)
            .output()?;
        Ok(serde_json::from_slice(out.stdout.trim_ascii())?)
    }

    #[test]
    fn should_answer_definitions_from_one_shared_jdtls_when_two_clients_use_one_worktree() -> Res<()>
    {
        // given: the fixture in a fresh worktree, and the pinned jdtls
        let home = default_jdtls_home().ok_or("no cache directory")?;
        if !home.join("plugins").is_dir() {
            return Err(format!(
                "no jdtls at {}: run bash scripts/jdtls-fetch.sh",
                home.display()
            )
            .into());
        }
        let dir = std::env::temp_dir().join(format!("jj-real-{}", std::process::id()));
        let scratch = Scratch(dir.clone());
        let root = dir.join("repo");
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/single-module"),
            &root,
        )?;
        std::fs::create_dir_all(root.join(".git"))?;
        let root = root.canonicalize()?;
        let state = scratch.0.join("s");
        let app = root.join("src/main/java/demo/App.java");
        let app_uri = file_uri(&app);
        let deadline = Instant::now() + READY;
        let mut a = Proxy::start(&state, &root, &home)?;

        // when: a opens App.java and asks where `Greeter` is defined, once jdtls is ready
        let caps = a.initialize(deadline)?;
        let text = serde_json::to_string(&std::fs::read_to_string(&app)?)?;
        a.send(&format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{app_uri}","languageId":"java","version":1,"text":{text}}}}}}}"#
        ))?;
        a.wait_for(deadline, |m| {
            m.get("method").and_then(Value::as_str) == Some("language/status")
                && m.get("params")
                    .and_then(|p| p.get("type").and_then(Value::as_str))
                    == Some("ServiceReady")
        })?;
        let from_a = a.definition(10, &app_uri, deadline)?;
        let mut b = Proxy::start(&state, &root, &home)?;
        b.initialize(deadline)?;
        let from_b = b.definition(10, &app_uri, deadline)?;

        // then: both get the definition, from one jdtls with its -data under the state directory
        assert!(
            caps.get("capabilities")
                .ok_or("no capabilities")?
                .get("definitionProvider")
                .is_some()
        );
        for result in [&from_a, &from_b] {
            let uri = first_uri(result).ok_or_else(|| format!("no location: {result:?}"))?;
            assert!(uri.ends_with("/demo/Greeter.java"), "{uri}");
        }
        let status = ctl(&state, "status")?;
        let Some(Value::Array(sessions)) = status.get("sessions") else {
            return Err("no sessions".into());
        };
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].get("clients"), Some(&Value::from(2)));
        let data_dir = sessions[0]
            .get("dataDir")
            .and_then(Value::as_str)
            .ok_or("no dataDir")?;
        assert!(Path::new(data_dir).starts_with(state.join("workspaces")));
        assert!(state.join("shared-index").is_dir());
        let backend = sessions[0]
            .get("backendPid")
            .cloned()
            .ok_or("no backendPid")?;

        // and when both leave and the daemon stops, no JVM is left
        a.close(deadline)?;
        b.close(deadline)?;
        ctl(&state, "stop")?;
        let Value::Number(pid) = backend else {
            return Err("pid".into());
        };
        let alive = Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stderr(Stdio::null())
            .status()?;
        assert!(!alive.success(), "jdtls {pid} still running");
        Ok(())
    }
}
