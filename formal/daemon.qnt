// The daemon model (FORMAL-04; plan §5.2, §5.3, §7.2; ADR 0016, ADR 0021): one state directory
// with its lock file (an flock), its socket file and the proxies' spawn lock; daemon processes
// that start, try the lock, unlink a stale socket, bind, serve, crash, exit when idle or on
// SIGTERM; proxies that connect, race to spawn, wait on the notify handshake, reconnect after a
// daemon dies and re-send their open documents and their request; clients that open and close
// documents, ask, and quit. It is the contract the daemon host and launcher of IDX-12 conform to;
// doc/formal-models.md states its stimuli, the observation the driver produces and the invariants.
//
// The design it fixes (doc/formal-models.md):
// - The owner is the daemon holding an flock on the inode at `<state>/daemon.lock`. The kernel
//   drops the flock when the process dies, so a crash never leaves the lock held; the lock file
//   is never removed (an unlinked lock file lets a second daemon lock a new inode).
// - Start: try the lock (fail: report the error on the notify socket and exit), then unlink any
//   socket file (stale: only the owner may bind), then bind, set 0600 and listen, then report
//   ready. Shutdown: stop accepting and close every connection, unlink the socket, then release
//   the lock and exit; never release before the unlink.
// - A socket file removed by someone else while its daemon serves (a tmp cleaner, a user's `rm`;
//   IDX-12's decision) is noticed by the daemon on its next tick, comparing the file at the path
//   with the one it bound, and the daemon shuts down: no proxy can reach it, and a spawned
//   daemon fails its lock, so serving on would starve every new client.
// - Overlays (open documents) are keyed by connection and die with it: a crash, a stop, or the
//   daemon reading a closed connection's EOF drops them; a new daemon starts with none.
// - A proxy that finds no daemon takes the spawn lock, checks again, spawns, and waits for the
//   notify handshake with a bounded budget; on any failure it retries from connect. On a new
//   connection it re-sends its open documents and its unanswered request.
module daemon {
  // ---- The world ----

  // Two proxies (each a client session: `jfl lsp` or a CLI call), three daemon process slots
  // (a slot is reused once nothing refers to its dead process), three connection slots, two
  // documents.
  pure val PROXIES: Set[str] = Set("p1", "p2")
  pure val DAEMONS: Set[str] = Set("d1", "d2", "d3")
  pure val CONNS: Set[str] = Set("c1", "c2", "c3")
  pure val DOCS: Set[str] = Set("a", "b")
  // `daemon.lock` inodes: 1 is the file the first lock creates; a control that unlinks and
  // recreates it takes the next number, up to this bound.
  pure val MAX_INODE = 3

  // ---- Types ----

  // A daemon process. `phase`:
  // - none: the slot was never used;
  // - started: running (it called setsid), before the lock;
  // - locked: holds the lock, nothing bound yet;
  // - serving: bound, 0600, listening, ready reported;
  // - stopping: stopped accepting, every connection closed (SIGTERM or idle exit);
  // - unlinked: its socket file removed, the lock still held;
  // - exited: released the lock and exited 0; failed: the lock was held, it reported the error
  //   and exited 1; crashed: killed (the kernel dropped its flock and closed its sockets).
  // `lock`: the inode of daemon.lock it holds an flock on, or 0.
  type Daemon = { phase: str, lock: int }
  // `<state>/sock`: whether a socket file is there and which daemon bound it ("" for one left
  // by a process before this run). With no file, `owner` names a live daemon whose socket file
  // was removed from under it by someone else (`sweep`) and that has not unlinked it yet; else "".
  type Sock = { present: bool, owner: str }
  // A connection. `state`:
  // - free: the slot is unused;
  // - backlog: connected in the kernel, not accepted yet;
  // - open: accepted, handshake done;
  // - peerClosed: the proxy closed it, the daemon has not read its EOF yet;
  // - closed: the daemon closed it (crash or stop), the proxy has not noticed yet.
  // `overlays`: the daemon's buffers for this connection's open documents; `inflight`: a request
  // sent on it and not answered.
  type Conn = { state: str, proxy: str, daemon: str, overlays: Set[str], inflight: bool }
  // A proxy. `phase`:
  // - disconnected: no connection; next it connects;
  // - noDaemon: connect failed (no socket file: ENOENT; a file nobody listens on: ECONNREFUSED);
  // - spawning: holds the spawn lock;
  // - awaiting: spawned `daemon` and waits on its notify socket;
  // - connecting: its connection is in the backlog;
  // - ready: connected, documents and request re-sent.
  // `docs`: the client's open documents; `want`: the client waits for an answer.
  type Proxy = { phase: str, conn: str, daemon: str, docs: Set[str], want: bool }

  pure val FREE_CONN: Conn = { state: "free", proxy: "", daemon: "", overlays: Set(), inflight: false }
  pure val NEW_PROXY: Proxy = { phase: "disconnected", conn: "", daemon: "", docs: Set(), want: false }
  pure val NO_SOCK: Sock = { present: false, owner: "" }

  // ---- State ----

  var daemons: str -> Daemon
  var lockFile: int          // the inode at <state>/daemon.lock, 0 while there is none
  var sock: Sock
  var spawnLock: str         // the proxy holding the spawn lock's flock, or ""
  var conns: str -> Conn
  var proxies: str -> Proxy
  var t: int                 // the steps taken since init (the driver's step index): the
                             // liveness simulation's clock, `Progress` below

  // ---- Helpers ----

  def phaseOf(d: str): str = daemons.get(d).phase
  def alive(d: str): bool = Set("started", "locked", "serving", "stopping", "unlinked", "released").contains(phaseOf(d))
  // The flock on inode `i` is held by some process.
  def held(i: int): bool = DAEMONS.exists(e => daemons.get(e).lock == i)
  // The owner: a live daemon holding the flock on the inode daemon.lock names.
  def isOwner(d: str): bool = and { alive(d), daemons.get(d).lock != 0, daemons.get(d).lock == lockFile }
  // A ready proxy whose connection is still open forwards what its client sends.
  def forwards(x: Proxy): bool = x.phase == "ready" and conns.get(x.conn).state == "open"
  // A daemon listening at the socket path: the file is there and its binder is serving.
  def listening: bool = sock.present and sock.owner != "" and phaseOf(sock.owner) == "serving"
  def live(state: str): bool = state == "open" or state == "peerClosed"
  // The daemon's socket file was removed from under it: it still listens on the unlinked inode.
  def swept(d: str): bool = not(sock.present) and sock.owner == d
  // A slot is free once its process is gone and nothing refers to it.
  def daemonFree(d: str): bool = and {
    Set("none", "exited", "failed", "crashed").contains(phaseOf(d)),
    not(sock.present and sock.owner == d),
    CONNS.forall(c => conns.get(c).state == "free" or conns.get(c).daemon != d),
    PROXIES.forall(p => proxies.get(p).daemon != d),
  }
  // The first free slot, in the order d1, d2, d3, or "" (spelled out: Apalache's temporal pass
  // cannot inline a fold here).
  def freeDaemon: str =
    if (daemonFree("d1")) "d1" else if (daemonFree("d2")) "d2" else if (daemonFree("d3")) "d3" else ""
  def connFree(c: str): bool = conns.get(c).state == "free"
  def freeConn: str = if (connFree("c1")) "c1" else if (connFree("c2")) "c2" else if (connFree("c3")) "c3" else ""
  // What a daemon's death or stop does to its connections: an accepted or queued one is closed
  // (its proxy will notice), one its proxy already closed is gone; their overlays die with them.
  def closedBy(d: str): str -> Conn = CONNS.mapBy(c => {
    val k = conns.get(c)
    if (k.state == "free" or k.daemon != d) k
    else if (k.state == "peerClosed") FREE_CONN
    else { ...k, state: "closed", overlays: Set(), inflight: false }
  })

  // ---- Initial state: a state directory, clean or as a crash left it ----

  // `stale`: a socket file left by a daemon that crashed before this run, nobody listening on it
  // (its flock died with it; the lock file stays). Every proxy is a client session with nothing
  // open yet.
  action initWith(stale: bool): bool = all {
    daemons' = DAEMONS.mapBy(_ => { phase: "none", lock: 0 }),
    lockFile' = if (stale) 1 else 0,
    sock' = { present: stale, owner: "" },
    spawnLock' = "",
    conns' = CONNS.mapBy(_ => FREE_CONN),
    proxies' = PROXIES.mapBy(_ => NEW_PROXY),
    t' = 0,
  }
  action init = { nondet stale = Set(false, true).oneOf() initWith(stale) }
  // The two initial states with the pick fixed, for the scenarios and the tests. `--mbt` records
  // the name of the init a trace was generated with; the driver treats all three as `init` with
  // the recorded `stale`.
  action initClean = { nondet stale = Set(false).oneOf() initWith(stale) }
  action initStale = { nondet stale = Set(true).oneOf() initWith(stale) }

  // ---- Clients ----

  // The client asks; a ready proxy sends the request at once, any other sends it once connected.
  def requestEnabled(proxy: str): bool = not(proxies.get(proxy).want)
  action request(proxy: str): bool = {
    val x = proxies.get(proxy)
    all {
      requestEnabled(proxy),
      proxies' = proxies.set(proxy, { ...x, want: true }),
      conns' = if (forwards(x)) conns.setBy(x.conn, k => { ...k, inflight: true }) else conns,
      daemons' = daemons, lockFile' = lockFile, sock' = sock, spawnLock' = spawnLock, t' = t + 1,
    }
  }

  // `didOpen`: the proxy records the document, and a ready proxy forwards it to the daemon.
  action didOpen(proxy: str, doc: str): bool = {
    val x = proxies.get(proxy)
    all {
      not(x.docs.contains(doc)),
      proxies' = proxies.set(proxy, { ...x, docs: x.docs.union(Set(doc)) }),
      conns' = if (forwards(x)) conns.setBy(x.conn, k => { ...k, overlays: k.overlays.union(Set(doc)) }) else conns,
      daemons' = daemons, lockFile' = lockFile, sock' = sock, spawnLock' = spawnLock, t' = t + 1,
    }
  }

  // `didClose`: the document leaves the proxy and, through a ready proxy, the daemon.
  action didClose(proxy: str, doc: str): bool = {
    val x = proxies.get(proxy)
    all {
      x.docs.contains(doc),
      proxies' = proxies.set(proxy, { ...x, docs: x.docs.exclude(Set(doc)) }),
      conns' = if (forwards(x)) conns.setBy(x.conn, k => { ...k, overlays: k.overlays.exclude(Set(doc)) }) else conns,
      daemons' = daemons, lockFile' = lockFile, sock' = sock, spawnLock' = spawnLock, t' = t + 1,
    }
  }

  // The client session ends (the editor closes, the CLI call returns) and a new one takes the
  // slot. Its connection is closed from the proxy's side (the daemon reads EOF later; one still
  // in the backlog is gone), its spawn lock released by the kernel. A client waiting for an
  // answer does not quit (the liveness claim is about one that keeps waiting).
  action quit(proxy: str): bool = {
    val x = proxies.get(proxy)
    all {
      not(x.want),
      proxies' = proxies.set(proxy, NEW_PROXY),
      conns' =
        if (x.conn == "") conns
        else if (conns.get(x.conn).state == "open") conns.setBy(x.conn, k => { ...k, state: "peerClosed" })
        else conns.set(x.conn, FREE_CONN),
      spawnLock' = if (spawnLock == proxy) "" else spawnLock,
      daemons' = daemons, lockFile' = lockFile, sock' = sock, t' = t + 1,
    }
  }

  // ---- Proxies ----

  // Connects to the socket path: queued in the daemon's backlog when one listens there,
  // otherwise refused (ENOENT, or ECONNREFUSED on a stale file or a stopped listener).
  def connectEnabled(proxy: str): bool = and {
    proxies.get(proxy).phase == "disconnected",
    not(listening) or freeConn != "",
  }
  action connect(proxy: str): bool = {
    val x = proxies.get(proxy)
    val c = freeConn
    all {
      connectEnabled(proxy),
      if (listening) all {
        conns' = conns.set(c, { state: "backlog", proxy: proxy, daemon: sock.owner, overlays: Set(), inflight: false }),
        proxies' = proxies.set(proxy, { ...x, phase: "connecting", conn: c }),
      } else all {
        conns' = conns,
        proxies' = proxies.set(proxy, { ...x, phase: "noDaemon" }),
      },
      daemons' = daemons, lockFile' = lockFile, sock' = sock, spawnLock' = spawnLock, t' = t + 1,
    }
  }

  // Tries the spawn lock; while another proxy holds it, waits and connects again.
  def spawnLockEnabled(proxy: str): bool = proxies.get(proxy).phase == "noDaemon"
  action takeSpawnLock(proxy: str): bool = {
    val x = proxies.get(proxy)
    all {
      spawnLockEnabled(proxy),
      if (spawnLock == "") all {
        spawnLock' = proxy,
        proxies' = proxies.set(proxy, { ...x, phase: "spawning" }),
      } else all {
        spawnLock' = spawnLock,
        proxies' = proxies.set(proxy, { ...x, phase: "disconnected" }),
      },
      daemons' = daemons, lockFile' = lockFile, sock' = sock, conns' = conns, t' = t + 1,
    }
  }

  // Under the spawn lock, checks again: a daemon a racing proxy started may listen by now, and
  // then it connects instead. Otherwise it binds its notify socket and starts `jfl daemon`
  // detached in the first free slot.
  def spawnEnabled(proxy: str): bool = and {
    proxies.get(proxy).phase == "spawning",
    listening or freeDaemon != "",
  }
  action spawn(proxy: str): bool = {
    val x = proxies.get(proxy)
    val d = freeDaemon
    all {
      spawnEnabled(proxy),
      if (listening) all {
        spawnLock' = "",
        proxies' = proxies.set(proxy, { ...x, phase: "disconnected" }),
        daemons' = daemons,
      } else all {
        spawnLock' = spawnLock,
        proxies' = proxies.set(proxy, { ...x, phase: "awaiting", daemon: d }),
        daemons' = daemons.set(d, { phase: "started", lock: 0 }),
      },
      lockFile' = lockFile, sock' = sock, conns' = conns, t' = t + 1,
    }
  }

  // The notify handshake answers: ready once the daemon listens, an error once it failed or
  // died. Either way the proxy releases the spawn lock and connects again.
  def handshakeEnabled(proxy: str): bool = {
    val x = proxies.get(proxy)
    x.phase == "awaiting" and not(Set("started", "locked").contains(phaseOf(x.daemon)))
  }
  action handshake(proxy: str): bool = all {
    handshakeEnabled(proxy),
    spawnLock' = if (spawnLock == proxy) "" else spawnLock,
    proxies' = proxies.setBy(proxy, x => { ...x, phase: "disconnected", daemon: "" }),
    daemons' = daemons, lockFile' = lockFile, sock' = sock, conns' = conns, t' = t + 1,
  }

  // The bounded wait expires while the daemon is still starting: as an error.
  action spawnTimeout(proxy: str): bool = all {
    proxies.get(proxy).phase == "awaiting",
    Set("started", "locked").contains(phaseOf(proxies.get(proxy).daemon)),
    spawnLock' = if (spawnLock == proxy) "" else spawnLock,
    proxies' = proxies.setBy(proxy, x => { ...x, phase: "disconnected", daemon: "" }),
    daemons' = daemons, lockFile' = lockFile, sock' = sock, conns' = conns, t' = t + 1,
  }

  // The proxy reads its connection's EOF: the daemon died or stopped. Its unanswered request
  // fails with an error it retries, so `want` stays; it connects again.
  def noticeEnabled(proxy: str): bool = {
    val x = proxies.get(proxy)
    Set("connecting", "ready").contains(x.phase) and conns.get(x.conn).state == "closed"
  }
  action notice(proxy: str): bool = {
    val x = proxies.get(proxy)
    all {
      noticeEnabled(proxy),
      conns' = conns.set(x.conn, FREE_CONN),
      proxies' = proxies.set(proxy, { ...x, phase: "disconnected", conn: "" }),
      daemons' = daemons, lockFile' = lockFile, sock' = sock, spawnLock' = spawnLock, t' = t + 1,
    }
  }

  // ---- Daemons ----

  // `File::try_lock` on daemon.lock (created if absent). Held by another process: the daemon
  // reports the error on its notify socket and exits 1.
  def lockEnabled(daemon: str): bool = phaseOf(daemon) == "started"
  action lock(daemon: str): bool = {
    val inode = if (lockFile == 0) 1 else lockFile
    all {
      lockEnabled(daemon),
      lockFile' = inode,
      daemons' = if (held(inode)) daemons.set(daemon, { phase: "failed", lock: 0 })
                 else daemons.set(daemon, { phase: "locked", lock: inode }),
      sock' = sock, spawnLock' = spawnLock, conns' = conns, proxies' = proxies, t' = t + 1,
    }
  }

  // The owner removes a socket file it finds: no live daemon can be listening on it (only the
  // owner binds, and an owner unlinks its socket before it releases the lock), so it is stale.
  def unlinkStaleEnabled(daemon: str): bool = phaseOf(daemon) == "locked" and sock.present
  action unlinkStale(daemon: str): bool = all {
    unlinkStaleEnabled(daemon),
    sock' = NO_SOCK,
    daemons' = daemons, lockFile' = lockFile, spawnLock' = spawnLock, conns' = conns, proxies' = proxies, t' = t + 1,
  }

  // Binds the socket, sets it 0600, listens, and reports ready on the notify socket.
  def listenEnabled(daemon: str): bool = phaseOf(daemon) == "locked" and not(sock.present)
  action listen(daemon: str): bool = all {
    listenEnabled(daemon),
    sock' = { present: true, owner: daemon },
    daemons' = daemons.setBy(daemon, x => { ...x, phase: "serving" }),
    lockFile' = lockFile, spawnLock' = spawnLock, conns' = conns, proxies' = proxies, t' = t + 1,
  }

  // Accepts a queued connection: the version handshake, then the proxy re-sends its open
  // documents and its unanswered request (plan §5.2).
  def acceptEnabled(daemon: str, conn: str): bool = and {
    phaseOf(daemon) == "serving",
    conns.get(conn).state == "backlog",
    conns.get(conn).daemon == daemon,
  }
  action accept(daemon: str, conn: str): bool = {
    val k = conns.get(conn)
    val x = proxies.get(k.proxy)
    all {
      acceptEnabled(daemon, conn),
      conns' = conns.set(conn, { ...k, state: "open", overlays: x.docs, inflight: x.want }),
      proxies' = proxies.set(k.proxy, { ...x, phase: "ready" }),
      daemons' = daemons, lockFile' = lockFile, sock' = sock, spawnLock' = spawnLock, t' = t + 1,
    }
  }

  // Answers the request in flight on an open connection.
  def answerEnabled(daemon: str, conn: str): bool = and {
    phaseOf(daemon) == "serving",
    conns.get(conn).state == "open",
    conns.get(conn).daemon == daemon,
    conns.get(conn).inflight,
  }
  action answer(daemon: str, conn: str): bool = {
    val k = conns.get(conn)
    all {
      answerEnabled(daemon, conn),
      conns' = conns.set(conn, { ...k, inflight: false }),
      proxies' = proxies.setBy(k.proxy, x => { ...x, want: false }),
      daemons' = daemons, lockFile' = lockFile, sock' = sock, spawnLock' = spawnLock, t' = t + 1,
    }
  }

  // Reads the EOF of a connection its proxy closed, and drops its state and overlays.
  def reapEnabled(daemon: str, conn: str): bool = and {
    phaseOf(daemon) == "serving",
    conns.get(conn).state == "peerClosed",
    conns.get(conn).daemon == daemon,
  }
  action reap(daemon: str, conn: str): bool = all {
    reapEnabled(daemon, conn),
    conns' = conns.set(conn, FREE_CONN),
    daemons' = daemons, lockFile' = lockFile, sock' = sock, spawnLock' = spawnLock, proxies' = proxies, t' = t + 1,
  }

  // The first step of a shutdown, on SIGTERM or idle exit: stop accepting (the listener is
  // closed, so a connection still in the backlog is reset and a new connect is refused), cancel
  // running queries, close every connection and drop its overlays, stop the workers. (`--mbt`
  // records the innermost action, so the two triggers repeat the effect rather than share an
  // action.)
  def stopped(daemon: str): str -> Daemon = daemons.setBy(daemon, x => { ...x, phase: "stopping" })

  // SIGTERM: a shutdown, whatever the connections.
  action sigterm(daemon: str): bool = all {
    phaseOf(daemon) == "serving",
    daemons' = stopped(daemon),
    conns' = closedBy(daemon),
    lockFile' = lockFile, sock' = sock, spawnLock' = spawnLock, proxies' = proxies, t' = t + 1,
  }

  // The idle period passed with no connection accepted and open: a shutdown. A connection
  // still in the backlog is not seen, and is reset with the listener (the race with a new
  // connection; its proxy reconnects).
  def idle(daemon: str): bool = CONNS.forall(c => not(live(conns.get(c).state) and conns.get(c).daemon == daemon))
  action idleExit(daemon: str): bool = all {
    phaseOf(daemon) == "serving",
    idle(daemon),
    daemons' = stopped(daemon),
    conns' = closedBy(daemon),
    lockFile' = lockFile, sock' = sock, spawnLock' = spawnLock, proxies' = proxies, t' = t + 1,
  }

  // Removes the socket file (the path, as the code does), while it still holds the lock.
  def unlinkEnabled(daemon: str): bool = phaseOf(daemon) == "stopping"
  action unlink(daemon: str): bool = all {
    unlinkEnabled(daemon),
    sock' = NO_SOCK,
    daemons' = daemons.setBy(daemon, x => { ...x, phase: "unlinked" }),
    lockFile' = lockFile, spawnLock' = spawnLock, conns' = conns, proxies' = proxies, t' = t + 1,
  }

  // Releases the lock (the file stays) and exits 0.
  def exitEnabled(daemon: str): bool = phaseOf(daemon) == "unlinked"
  action exit(daemon: str): bool = all {
    exitEnabled(daemon),
    daemons' = daemons.set(daemon, { phase: "exited", lock: 0 }),
    lockFile' = lockFile, sock' = sock, spawnLock' = spawnLock, conns' = conns, proxies' = proxies, t' = t + 1,
  }

  // A crash (a panic, OOM kill, SIGKILL; also a signal before the handler is installed): the
  // kernel drops the flock and closes every socket; the socket file stays, stale; memory, and
  // with it every overlay, is gone.
  action crash(daemon: str): bool = all {
    alive(daemon),
    daemons' = daemons.set(daemon, { phase: "crashed", lock: 0 }),
    conns' = closedBy(daemon),
    // A swept socket's inode dies with its listener: nothing at the path, nobody bound.
    sock' = if (swept(daemon)) NO_SOCK else sock,
    lockFile' = lockFile, spawnLock' = spawnLock, proxies' = proxies, t' = t + 1,
  }

  // Someone else removes the socket file of the serving owner (a tmp cleaner on plan §5.3's
  // `/tmp` fallback, a user's `rm`): the daemon still listens on the unlinked inode, which no
  // proxy can reach any more (connect: ENOENT).
  action sweep: bool = all {
    listening,
    sock' = { ...sock, present: false },
    daemons' = daemons, lockFile' = lockFile, spawnLock' = spawnLock, conns' = conns, proxies' = proxies, t' = t + 1,
  }

  // On its tick the daemon finds that the path no longer names the socket it bound, and shuts
  // down as on SIGTERM; its `unlink` then finds nothing to remove.
  def noticeSweptEnabled(daemon: str): bool = phaseOf(daemon) == "serving" and swept(daemon)
  action noticeSwept(daemon: str): bool = all {
    noticeSweptEnabled(daemon),
    daemons' = stopped(daemon),
    conns' = closedBy(daemon),
    lockFile' = lockFile, sock' = sock, spawnLock' = spawnLock, proxies' = proxies, t' = t + 1,
  }

  // ---- Every stimulus, for simulation, Apalache and the generated conformance traces ----

  // Each parameter is a nondeterministic pick named as the parameter (`proxy`, `daemon`, `conn`,
  // `doc`), so `quint run --mbt` records it for the driver. The stimuli are grouped so each kind
  // is about equally likely, and faults are rarer than the steps that recover from them: a run
  // must get a daemon serving, crash it and reach an answer from the next one.
  action clientStep = any {
    nondet proxy = PROXIES.oneOf() request(proxy),
    nondet proxy = PROXIES.oneOf() nondet doc = DOCS.oneOf() didOpen(proxy, doc),
    nondet proxy = PROXIES.oneOf() nondet doc = DOCS.oneOf() didClose(proxy, doc),
    nondet proxy = PROXIES.oneOf() quit(proxy),
  }

  action proxyStep = any {
    nondet proxy = PROXIES.oneOf() connect(proxy),
    nondet proxy = PROXIES.oneOf() takeSpawnLock(proxy),
    nondet proxy = PROXIES.oneOf() spawn(proxy),
    nondet proxy = PROXIES.oneOf() handshake(proxy),
    nondet proxy = PROXIES.oneOf() notice(proxy),
  }

  action faultStep = any {
    nondet daemon = DAEMONS.oneOf() crash(daemon),
    nondet daemon = DAEMONS.oneOf() sigterm(daemon),
    nondet daemon = DAEMONS.oneOf() idleExit(daemon),
    nondet proxy = PROXIES.oneOf() spawnTimeout(proxy),
    sweep,
  }

  action daemonStep = any {
    nondet daemon = DAEMONS.oneOf() lock(daemon),
    nondet daemon = DAEMONS.oneOf() unlinkStale(daemon),
    nondet daemon = DAEMONS.oneOf() listen(daemon),
    nondet daemon = DAEMONS.oneOf() nondet conn = CONNS.oneOf() accept(daemon, conn),
    nondet daemon = DAEMONS.oneOf() nondet conn = CONNS.oneOf() answer(daemon, conn),
    nondet daemon = DAEMONS.oneOf() nondet conn = CONNS.oneOf() reap(daemon, conn),
    nondet daemon = DAEMONS.oneOf() unlink(daemon),
    nondet daemon = DAEMONS.oneOf() exit(daemon),
    nondet daemon = DAEMONS.oneOf() noticeSwept(daemon),
    faultStep,
  }

  action step = any { clientStep, proxyStep, daemonStep }

  // ---- Invariants (plan §7.2, ver §6.1) ----

  // At most one process holds an flock on daemon.lock, on any inode it ever named.
  val AtMostOneOwner = DAEMONS.filter(d => daemons.get(d).lock != 0).size() <= 1

  // Every connection a daemon has accepted and not yet closed is the owner's, and that owner is
  // serving: no other daemon answers anyone.
  val ServedOnlyByOwner = CONNS.forall(c => {
    val k = conns.get(c)
    live(k.state) implies (isOwner(k.daemon) and phaseOf(k.daemon) == "serving")
  })

  // Every overlay belongs to a live connection of the current daemon (open, or closed by its
  // proxy and not yet read to EOF), and an open connection holds only documents its proxy has
  // open: nothing of a dead connection or of an earlier daemon, nothing the client closed.
  val NoGhostBuffers = CONNS.forall(c => {
    val k = conns.get(c)
    k.overlays != Set() implies and {
      live(k.state),
      isOwner(k.daemon),
      k.state == "open" implies and {
        proxies.get(k.proxy).conn == c,
        k.overlays.subseteq(proxies.get(k.proxy).docs),
      },
    }
  })

  // After a stop, no socket and no lock remain: a daemon that exited holds no flock and left no
  // socket file of its own.
  val ShutdownReleases = DAEMONS.forall(d => {
    phaseOf(d) == "exited" implies and {
      daemons.get(d).lock == 0,
      not(sock.present and sock.owner == d),
    }
  })

  // A serving owner's socket is the file at the path, so a proxy that connects reaches it, or
  // that file was removed from under it by someone else (`sweep`, the one fault no code can
  // prevent), which the owner notices on its tick and stops (`noticeSwept`; the liveness check
  // fails without it). The plan's own negative control (release the lock before removing the
  // socket) breaks this: the next owner binds, then the old one's unlink removes the new socket,
  // leaving the path naming nothing of the new owner's.
  val OwnerReachable = DAEMONS.forall(d => {
    (phaseOf(d) == "serving" and isOwner(d)) implies sock.owner == d
  })

  // A ready proxy's connection holds every document it has open and its unanswered request: a
  // reconnect re-sends both (plan §5.2), and a request is never silently dropped.
  val ReconnectRestores = PROXIES.forall(p => {
    val x = proxies.get(p)
    x.phase == "ready" implies and {
      conns.get(x.conn).state == "open" or conns.get(x.conn).state == "closed",
      conns.get(x.conn).state == "open" implies and {
        x.docs.subseteq(conns.get(x.conn).overlays),
        x.want implies conns.get(x.conn).inflight,
      },
    }
  })

  val allInvariants = and {
    AtMostOneOwner,
    ServedOnlyByOwner,
    NoGhostBuffers,
    ShutdownReleases,
    OwnerReachable,
    ReconnectRestores,
  }

  // ---- Liveness (plan §7.2): a proxy that keeps retrying is eventually served ----
  //
  // Apalache has no ENABLED, so each weak fairness condition is written with the action's own
  // enabling condition, the `*Enabled` definition its guard uses: `[]<>(not enabled or action)`.
  // The system's actions are weakly fair: every proxy step that retries and every daemon step
  // that makes progress. Faults are not: crashes, SIGTERM, idle exits and expired waits happen
  // finitely often (`<>[]` none of them), since a daemon that crashes forever serves nobody.
  // The clients are free: they ask, open, close and quit as they like, but a client waiting for
  // an answer does not quit (`quit` requires `not(want)`).
  // Each proxy is in one phase, and one of its actions at most is enabled in it; so is each
  // daemon, and each connection (whose daemon is fixed). Weak fairness of a proxy's, a daemon's
  // or a connection's moves taken together is then the same as weak fairness of each move, and
  // costs Apalache one temporal formula instead of five or three.
  def proxyMoveEnabled(proxy: str): bool = or {
    connectEnabled(proxy), spawnLockEnabled(proxy), spawnEnabled(proxy), handshakeEnabled(proxy),
    noticeEnabled(proxy),
  }
  action proxyMove(proxy: str): bool = any {
    connect(proxy), takeSpawnLock(proxy), spawn(proxy), handshake(proxy), notice(proxy),
  }
  def daemonMoveEnabled(daemon: str): bool = or {
    lockEnabled(daemon), unlinkStaleEnabled(daemon), listenEnabled(daemon), unlinkEnabled(daemon),
    exitEnabled(daemon), noticeSweptEnabled(daemon),
  }
  action daemonMove(daemon: str): bool = any {
    lock(daemon), unlinkStale(daemon), listen(daemon), unlink(daemon), exit(daemon),
    noticeSwept(daemon),
  }
  def connMoveEnabled(conn: str): bool = {
    val d = conns.get(conn).daemon
    d != "" and or { acceptEnabled(d, conn), answerEnabled(d, conn), reapEnabled(d, conn) }
  }
  action connMove(conn: str): bool = {
    val d = conns.get(conn).daemon
    all { d != "", any { accept(d, conn), answer(d, conn), reap(d, conn) } }
  }

  temporal wfProxy(proxy: str): bool = always(eventually(not(proxyMoveEnabled(proxy)) or proxyMove(proxy)))
  temporal wfDaemon(daemon: str): bool = always(eventually(not(daemonMoveEnabled(daemon)) or daemonMove(daemon)))
  temporal wfConn(conn: str): bool = always(eventually(not(connMoveEnabled(conn)) or connMove(conn)))

  // Apalache cannot quantify over a temporal formula (a bound variable inside `always` is left
  // unassigned), so the conditions are spelled out for every proxy, daemon and connection.
  temporal fairness = and {
    wfProxy("p1"), wfProxy("p2"),
    wfDaemon("d1"), wfDaemon("d2"), wfDaemon("d3"),
    wfConn("c1"), wfConn("c2"), wfConn("c3"),
  }

  // Finitely many faults: from some point on, none.
  temporal faultsStop = eventually(always(and {
    not(crash("d1")), not(sigterm("d1")), not(idleExit("d1")),
    not(crash("d2")), not(sigterm("d2")), not(idleExit("d2")),
    not(crash("d3")), not(sigterm("d3")), not(idleExit("d3")),
    not(spawnTimeout("p1")), not(spawnTimeout("p2")), not(sweep),
  }))

  temporal servedAfterWaiting(proxy: str): bool =
    always(proxies.get(proxy).want implies eventually(not(proxies.get(proxy).want)))

  // Checked through `Progress` below, not by Apalache: too costly at a bound that reaches its
  // counterexamples, and vacuous on this model, whose clock `t` never lets a state repeat.
  temporal EventuallyServed = (fairness and faultsStop) implies and {
    servedAfterWaiting("p1"), servedAfterWaiting("p2"),
  }

  // ---- Witnesses: states the random simulation must reach (doc/formal-models.md, Budget) ----
  //
  // `quint run --invariant 'not(<witness>)'` must find each one, or the simulation does not
  // explore what the invariants are about.

  // A connection to a daemon started after another one crashed holds overlays.
  val witnessRespawnHoldsOverlays = CONNS.exists(c => {
    val k = conns.get(c)
    k.state == "open" and k.overlays != Set() and DAEMONS.exists(d => d != k.daemon and phaseOf(d) == "crashed")
  })
  // A spawned daemon failed the lock while another one owns it.
  val witnessLockRefused = DAEMONS.exists(d => phaseOf(d) == "failed") and DAEMONS.exists(e => isOwner(e))
  // A connection in the backlog was reset by a stopping daemon (the idle exit's race).
  val witnessBacklogReset = PROXIES.exists(p => {
    val x = proxies.get(p)
    x.phase == "connecting" and conns.get(x.conn).state == "closed" and phaseOf(conns.get(x.conn).daemon) != "crashed"
  })
  // A proxy holding the spawn lock finds a daemon listening: its second check spawns nothing.
  val witnessSecondCheck = PROXIES.exists(p => proxies.get(p).phase == "spawning") and listening
  // An owner about to unlink a stale socket, from a crash in this run.
  val witnessStaleFromCrash = DAEMONS.exists(d => phaseOf(d) == "locked") and sock.present and sock.owner != "" and phaseOf(sock.owner) == "crashed"
  // A daemon serves after another one shut down cleanly.
  val witnessServingAfterShutdown = DAEMONS.exists(d => phaseOf(d) == "exited") and DAEMONS.exists(e => phaseOf(e) == "serving")
  // A daemon whose socket was swept noticed it and stops.
  val witnessSweptNoticed = DAEMONS.exists(d => phaseOf(d) == "stopping" and swept(d))
  // A closed connection's overlays await the daemon's read of its EOF.
  val witnessEofPending = CONNS.exists(c => conns.get(c).state == "peerClosed" and conns.get(c).overlays != Set())
  // A request answered by a daemon after a crash: a ready proxy no longer waiting, served by a
  // daemon other than one that crashed.
  val witnessAnsweredAfterCrash = PROXIES.exists(p => {
    val x = proxies.get(p)
    x.phase == "ready" and not(x.want) and conns.get(x.conn).state == "open" and x.docs != Set()
      and DAEMONS.exists(d => phaseOf(d) == "crashed")
  })

  // ---- Named scenarios (ver §6.1) ----
  //
  // Each scenario is a scripted step: guarded branches of which exactly one is enabled in each
  // state of the run, so `quint run --mbt --init <initClean|initStale> --step <scenario>` writes
  // its regression trace with the same action names and picks as `step`
  // (formal/traces/regression/manifest.toml); its test below takes the same steps as named
  // actions.

  def ph(p: str): str = proxies.get(p).phase
  def cs(c: str): str = conns.get(c).state

  // Two proxies find no daemon at once. One takes the spawn lock and spawns; the other is
  // refused the spawn lock, finds nothing listening yet, and when it takes the lock its second
  // check finds the daemon, so it spawns nothing. One daemon serves both.
  action twoProxiesSpawnAtOnce = any {
    all { phaseOf("d1") == "none", ph("p1") == "disconnected", ph("p2") == "disconnected",
          not(proxies.get("p1").want), nondet proxy = Set("p1").oneOf() request(proxy) },
    all { phaseOf("d1") == "none", proxies.get("p1").want, not(proxies.get("p2").want),
          nondet proxy = Set("p2").oneOf() request(proxy) },
    all { phaseOf("d1") == "none", proxies.get("p1").want, proxies.get("p2").want,
          ph("p1") == "disconnected", ph("p2") == "disconnected", nondet proxy = Set("p1").oneOf() connect(proxy) },
    all { phaseOf("d1") == "none", ph("p1") == "noDaemon", ph("p2") == "disconnected", spawnLock == "",
          nondet proxy = Set("p2").oneOf() connect(proxy) },
    all { ph("p1") == "noDaemon", ph("p2") == "noDaemon", spawnLock == "",
          nondet proxy = Set("p1").oneOf() takeSpawnLock(proxy) },
    all { phaseOf("d1") == "none", ph("p1") == "spawning", ph("p2") == "noDaemon",
          nondet proxy = Set("p2").oneOf() takeSpawnLock(proxy) },
    all { phaseOf("d1") == "none", ph("p1") == "spawning", ph("p2") == "disconnected",
          nondet proxy = Set("p1").oneOf() spawn(proxy) },
    all { phaseOf("d1") == "started", ph("p1") == "awaiting", ph("p2") == "disconnected",
          nondet proxy = Set("p2").oneOf() connect(proxy) },
    all { phaseOf("d1") == "started", ph("p2") == "noDaemon", nondet daemon = Set("d1").oneOf() lock(daemon) },
    all { phaseOf("d1") == "locked", ph("p2") == "noDaemon", nondet daemon = Set("d1").oneOf() listen(daemon) },
    all { phaseOf("d1") == "serving", ph("p1") == "awaiting", nondet proxy = Set("p1").oneOf() handshake(proxy) },
    all { phaseOf("d1") == "serving", ph("p1") == "disconnected", ph("p2") == "noDaemon", spawnLock == "",
          nondet proxy = Set("p2").oneOf() takeSpawnLock(proxy) },
    all { ph("p2") == "spawning", nondet proxy = Set("p2").oneOf() spawn(proxy) },
    all { phaseOf("d1") == "serving", ph("p1") == "disconnected", ph("p2") == "disconnected",
          nondet proxy = Set("p1").oneOf() connect(proxy) },
    all { phaseOf("d1") == "serving", ph("p1") == "connecting", ph("p2") == "disconnected",
          nondet proxy = Set("p2").oneOf() connect(proxy) },
    all { cs("c1") == "backlog", cs("c2") == "backlog", nondet daemon = Set("d1").oneOf() nondet conn = Set("c1").oneOf() accept(daemon, conn) },
    all { cs("c1") == "open", cs("c2") == "backlog", nondet daemon = Set("d1").oneOf() nondet conn = Set("c2").oneOf() accept(daemon, conn) },
    all { cs("c2") == "open", conns.get("c1").inflight, nondet daemon = Set("d1").oneOf() nondet conn = Set("c1").oneOf() answer(daemon, conn) },
    all { not(conns.get("c1").inflight), conns.get("c2").inflight, nondet daemon = Set("d1").oneOf() nondet conn = Set("c2").oneOf() answer(daemon, conn) },
  }

  // A daemon crashes while p1 has a document open and a request in flight, and p2's closed
  // connection still awaits its EOF with an overlay. The stale socket refuses p1's connect; p1
  // spawns a new daemon, which unlinks the stale file, binds, and gets p1's document and request
  // re-sent; it holds no overlay of either dead connection, and it answers.
  action crashWithOpenDocuments = any {
    all { phaseOf("d1") == "none", ph("p1") == "disconnected", proxies.get("p1").docs == Set(),
          nondet proxy = Set("p1").oneOf() nondet doc = Set("a").oneOf() didOpen(proxy, doc) },
    all { phaseOf("d1") == "none", ph("p1") == "disconnected", proxies.get("p1").docs == Set("a"),
          nondet proxy = Set("p1").oneOf() connect(proxy) },
    all { phaseOf("d1") == "none", ph("p1") == "noDaemon", nondet proxy = Set("p1").oneOf() takeSpawnLock(proxy) },
    all { phaseOf("d1") == "none", ph("p1") == "spawning", nondet proxy = Set("p1").oneOf() spawn(proxy) },
    all { phaseOf("d1") == "started", nondet daemon = Set("d1").oneOf() lock(daemon) },
    all { phaseOf("d1") == "locked", nondet daemon = Set("d1").oneOf() listen(daemon) },
    all { phaseOf("d1") == "serving", ph("p1") == "awaiting", nondet proxy = Set("p1").oneOf() handshake(proxy) },
    all { phaseOf("d1") == "serving", ph("p1") == "disconnected", ph("p2") == "disconnected",
          nondet proxy = Set("p1").oneOf() connect(proxy) },
    all { phaseOf("d1") == "serving", ph("p1") == "connecting", nondet proxy = Set("p2").oneOf() connect(proxy) },
    all { phaseOf("d1") == "serving", cs("c1") == "backlog", cs("c2") == "backlog", nondet daemon = Set("d1").oneOf() nondet conn = Set("c1").oneOf() accept(daemon, conn) },
    all { phaseOf("d1") == "serving", cs("c1") == "open", cs("c2") == "backlog", nondet daemon = Set("d1").oneOf() nondet conn = Set("c2").oneOf() accept(daemon, conn) },
    all { phaseOf("d1") == "serving", ph("p2") == "ready", proxies.get("p2").docs == Set(),
          nondet proxy = Set("p2").oneOf() nondet doc = Set("b").oneOf() didOpen(proxy, doc) },
    all { phaseOf("d1") == "serving", ph("p2") == "ready", proxies.get("p2").docs == Set("b"),
          nondet proxy = Set("p2").oneOf() quit(proxy) },
    all { phaseOf("d1") == "serving", cs("c2") == "peerClosed", not(proxies.get("p1").want),
          nondet proxy = Set("p1").oneOf() request(proxy) },
    all { phaseOf("d1") == "serving", cs("c2") == "peerClosed", proxies.get("p1").want,
          nondet daemon = Set("d1").oneOf() crash(daemon) },
    all { phaseOf("d1") == "crashed", ph("p1") == "ready", nondet proxy = Set("p1").oneOf() notice(proxy) },
    all { phaseOf("d1") == "crashed", phaseOf("d2") == "none", ph("p1") == "disconnected",
          nondet proxy = Set("p1").oneOf() connect(proxy) },
    all { phaseOf("d2") == "none", ph("p1") == "noDaemon", nondet proxy = Set("p1").oneOf() takeSpawnLock(proxy) },
    all { phaseOf("d2") == "none", ph("p1") == "spawning", nondet proxy = Set("p1").oneOf() spawn(proxy) },
    all { phaseOf("d2") == "started", nondet daemon = Set("d2").oneOf() lock(daemon) },
    all { phaseOf("d2") == "locked", sock.present, nondet daemon = Set("d2").oneOf() unlinkStale(daemon) },
    all { phaseOf("d2") == "locked", not(sock.present), nondet daemon = Set("d2").oneOf() listen(daemon) },
    all { phaseOf("d2") == "serving", ph("p1") == "awaiting", nondet proxy = Set("p1").oneOf() handshake(proxy) },
    all { phaseOf("d2") == "serving", ph("p1") == "disconnected", nondet proxy = Set("p1").oneOf() connect(proxy) },
    all { phaseOf("d2") == "serving", cs("c1") == "backlog", nondet daemon = Set("d2").oneOf() nondet conn = Set("c1").oneOf() accept(daemon, conn) },
    all { phaseOf("d2") == "serving", cs("c1") == "open", conns.get("c1").inflight,
          nondet daemon = Set("d2").oneOf() nondet conn = Set("c1").oneOf() answer(daemon, conn) },
  }

  // The daemon's last connection closes and its idle period ends just as p2 connects: p2's
  // connection is still in the backlog, so the idle exit resets it. p2 is refused while the old
  // daemon stops, and its spawn fails the lock the old daemon still holds; once the old daemon
  // has unlinked its socket and released the lock, p2's next spawn serves it.
  action idleExitRacesConnect = any {
    all { phaseOf("d1") == "none", ph("p1") == "disconnected", not(proxies.get("p1").want),
          nondet proxy = Set("p1").oneOf() request(proxy) },
    all { phaseOf("d1") == "none", ph("p1") == "disconnected", proxies.get("p1").want,
          nondet proxy = Set("p1").oneOf() connect(proxy) },
    all { phaseOf("d1") == "none", ph("p1") == "noDaemon", nondet proxy = Set("p1").oneOf() takeSpawnLock(proxy) },
    all { phaseOf("d1") == "none", ph("p1") == "spawning", nondet proxy = Set("p1").oneOf() spawn(proxy) },
    all { phaseOf("d1") == "started", phaseOf("d2") == "none", nondet daemon = Set("d1").oneOf() lock(daemon) },
    all { phaseOf("d1") == "locked", phaseOf("d2") == "none", nondet daemon = Set("d1").oneOf() listen(daemon) },
    all { phaseOf("d1") == "serving", phaseOf("d2") == "none", ph("p1") == "awaiting",
          nondet proxy = Set("p1").oneOf() handshake(proxy) },
    all { phaseOf("d1") == "serving", phaseOf("d2") == "none", ph("p1") == "disconnected", proxies.get("p1").want,
          nondet proxy = Set("p1").oneOf() connect(proxy) },
    all { phaseOf("d2") == "none", ph("p1") == "connecting", cs("c1") == "backlog",
          nondet daemon = Set("d1").oneOf() nondet conn = Set("c1").oneOf() accept(daemon, conn) },
    all { phaseOf("d2") == "none", cs("c1") == "open", conns.get("c1").inflight,
          nondet daemon = Set("d1").oneOf() nondet conn = Set("c1").oneOf() answer(daemon, conn) },
    all { phaseOf("d2") == "none", cs("c1") == "open", ph("p1") == "ready", not(proxies.get("p1").want),
          nondet proxy = Set("p1").oneOf() quit(proxy) },
    all { phaseOf("d2") == "none", cs("c1") == "peerClosed",
          nondet daemon = Set("d1").oneOf() nondet conn = Set("c1").oneOf() reap(daemon, conn) },
    all { phaseOf("d1") == "serving", phaseOf("d2") == "none", cs("c1") == "free", ph("p1") == "disconnected",
          not(proxies.get("p1").want), not(proxies.get("p2").want),
          nondet proxy = Set("p2").oneOf() request(proxy) },
    all { phaseOf("d1") == "serving", phaseOf("d2") == "none", cs("c1") == "free", ph("p2") == "disconnected",
          proxies.get("p2").want, nondet proxy = Set("p2").oneOf() connect(proxy) },
    all { phaseOf("d1") == "serving", phaseOf("d2") == "none", cs("c1") == "backlog", ph("p2") == "connecting",
          nondet daemon = Set("d1").oneOf() idleExit(daemon) },
    all { phaseOf("d1") == "stopping", ph("p2") == "connecting", nondet proxy = Set("p2").oneOf() notice(proxy) },
    all { phaseOf("d1") == "stopping", phaseOf("d2") == "none", ph("p2") == "disconnected",
          nondet proxy = Set("p2").oneOf() connect(proxy) },
    all { phaseOf("d1") == "stopping", phaseOf("d2") == "none", ph("p2") == "noDaemon",
          nondet proxy = Set("p2").oneOf() takeSpawnLock(proxy) },
    all { phaseOf("d1") == "stopping", phaseOf("d2") == "none", ph("p2") == "spawning",
          nondet proxy = Set("p2").oneOf() spawn(proxy) },
    all { phaseOf("d2") == "started", nondet daemon = Set("d2").oneOf() lock(daemon) },
    all { phaseOf("d2") == "failed", ph("p2") == "awaiting", nondet proxy = Set("p2").oneOf() handshake(proxy) },
    all { phaseOf("d1") == "stopping", phaseOf("d2") == "failed", ph("p2") == "disconnected",
          nondet daemon = Set("d1").oneOf() unlink(daemon) },
    all { phaseOf("d1") == "unlinked", nondet daemon = Set("d1").oneOf() exit(daemon) },
    all { phaseOf("d1") == "exited", ph("p2") == "disconnected", nondet proxy = Set("p2").oneOf() connect(proxy) },
    all { phaseOf("d1") == "exited", ph("p2") == "noDaemon", nondet proxy = Set("p2").oneOf() takeSpawnLock(proxy) },
    all { phaseOf("d1") == "exited", ph("p2") == "spawning", nondet proxy = Set("p2").oneOf() spawn(proxy) },
    all { phaseOf("d1") == "started", phaseOf("d2") == "failed", nondet daemon = Set("d1").oneOf() lock(daemon) },
    all { phaseOf("d1") == "locked", phaseOf("d2") == "failed", nondet daemon = Set("d1").oneOf() listen(daemon) },
    all { phaseOf("d1") == "serving", phaseOf("d2") == "failed", ph("p2") == "awaiting",
          nondet proxy = Set("p2").oneOf() handshake(proxy) },
    all { phaseOf("d1") == "serving", phaseOf("d2") == "failed", ph("p2") == "disconnected", proxies.get("p2").want,
          nondet proxy = Set("p2").oneOf() connect(proxy) },
    all { phaseOf("d2") == "failed", cs("c1") == "backlog",
          nondet daemon = Set("d1").oneOf() nondet conn = Set("c1").oneOf() accept(daemon, conn) },
    all { phaseOf("d2") == "failed", cs("c1") == "open", conns.get("c1").inflight,
          nondet daemon = Set("d1").oneOf() nondet conn = Set("c1").oneOf() answer(daemon, conn) },
  }

  // A crash left a stale socket file before this run: p1 is refused, spawns, and the daemon
  // takes the existing lock file, unlinks the stale socket and binds. SIGTERM then stops it with
  // p1 connected: p1 reads EOF; the daemon unlinks its socket and releases the lock, leaving
  // nothing behind; p1 finds no socket and spawns again.
  action sigtermAfterStaleStart = any {
    all { phaseOf("d1") == "none", ph("p1") == "disconnected", sock.present, nondet proxy = Set("p1").oneOf() connect(proxy) },
    all { phaseOf("d1") == "none", ph("p1") == "noDaemon", nondet proxy = Set("p1").oneOf() takeSpawnLock(proxy) },
    all { phaseOf("d1") == "none", ph("p1") == "spawning", nondet proxy = Set("p1").oneOf() spawn(proxy) },
    all { phaseOf("d1") == "started", nondet daemon = Set("d1").oneOf() lock(daemon) },
    all { phaseOf("d1") == "locked", sock.present, nondet daemon = Set("d1").oneOf() unlinkStale(daemon) },
    all { phaseOf("d1") == "locked", not(sock.present), nondet daemon = Set("d1").oneOf() listen(daemon) },
    all { phaseOf("d1") == "serving", ph("p1") == "awaiting", nondet proxy = Set("p1").oneOf() handshake(proxy) },
    all { phaseOf("d1") == "serving", ph("p1") == "disconnected", nondet proxy = Set("p1").oneOf() connect(proxy) },
    all { phaseOf("d1") == "serving", cs("c1") == "backlog", nondet daemon = Set("d1").oneOf() nondet conn = Set("c1").oneOf() accept(daemon, conn) },
    all { phaseOf("d1") == "serving", cs("c1") == "open", nondet daemon = Set("d1").oneOf() sigterm(daemon) },
    all { phaseOf("d1") == "stopping", ph("p1") == "ready", nondet proxy = Set("p1").oneOf() notice(proxy) },
    all { phaseOf("d1") == "stopping", ph("p1") == "disconnected", nondet daemon = Set("d1").oneOf() unlink(daemon) },
    all { phaseOf("d1") == "unlinked", nondet daemon = Set("d1").oneOf() exit(daemon) },
    all { phaseOf("d1") == "exited", ph("p1") == "disconnected", nondet proxy = Set("p1").oneOf() connect(proxy) },
  }

  // The serving daemon's socket file is removed by someone else while p1 is connected (a tmp
  // cleaner on plan §5.3's `/tmp` fallback, a user's `rm`). p2 finds no socket and spawns a
  // daemon, which fails the lock the first one still holds. On its tick the first daemon notices
  // that the path no longer names its socket and shuts down: p1 reads EOF, the daemon unlinks
  // (nothing is left to remove) and releases, and p2's next spawn serves it. Each branch is
  // guarded by the step count `t`, so exactly one is enabled in each state.
  action sweptSocketStops = any {
    all { t == 0, nondet proxy = Set("p1").oneOf() connect(proxy) },
    all { t == 1, nondet proxy = Set("p1").oneOf() takeSpawnLock(proxy) },
    all { t == 2, nondet proxy = Set("p1").oneOf() spawn(proxy) },
    all { t == 3, nondet daemon = Set("d1").oneOf() lock(daemon) },
    all { t == 4, nondet daemon = Set("d1").oneOf() listen(daemon) },
    all { t == 5, nondet proxy = Set("p1").oneOf() handshake(proxy) },
    all { t == 6, nondet proxy = Set("p1").oneOf() connect(proxy) },
    all { t == 7, nondet daemon = Set("d1").oneOf() nondet conn = Set("c1").oneOf() accept(daemon, conn) },
    all { t == 8, sweep },
    all { t == 9, nondet proxy = Set("p2").oneOf() request(proxy) },
    all { t == 10, nondet proxy = Set("p2").oneOf() connect(proxy) },
    all { t == 11, nondet proxy = Set("p2").oneOf() takeSpawnLock(proxy) },
    all { t == 12, nondet proxy = Set("p2").oneOf() spawn(proxy) },
    all { t == 13, nondet daemon = Set("d2").oneOf() lock(daemon) },
    all { t == 14, nondet proxy = Set("p2").oneOf() handshake(proxy) },
    all { t == 15, nondet daemon = Set("d1").oneOf() noticeSwept(daemon) },
    all { t == 16, nondet proxy = Set("p1").oneOf() notice(proxy) },
    all { t == 17, nondet daemon = Set("d1").oneOf() unlink(daemon) },
    all { t == 18, nondet daemon = Set("d1").oneOf() exit(daemon) },
    all { t == 19, nondet proxy = Set("p2").oneOf() connect(proxy) },
    all { t == 20, nondet proxy = Set("p2").oneOf() takeSpawnLock(proxy) },
    all { t == 21, nondet proxy = Set("p2").oneOf() spawn(proxy) },
    all { t == 22, nondet daemon = Set("d1").oneOf() lock(daemon) },
    all { t == 23, nondet daemon = Set("d1").oneOf() listen(daemon) },
    all { t == 24, nondet proxy = Set("p2").oneOf() handshake(proxy) },
    all { t == 25, nondet proxy = Set("p2").oneOf() connect(proxy) },
    all { t == 26, nondet daemon = Set("d1").oneOf() nondet conn = Set("c1").oneOf() accept(daemon, conn) },
    all { t == 27, nondet daemon = Set("d1").oneOf() nondet conn = Set("c1").oneOf() answer(daemon, conn) },
  }

  // The scenario tests replay each script as named actions, step for step (a test that draws a
  // nondeterministic value runs 10000 times, so the scripts themselves serve only the traces).

  // Two connected proxies, served: from a clean start to both proxies ready on daemon `d`.
  run twoProxiesSpawnAtOnceTest =
    initWith(false)
      .then(request("p1"))
      .then(request("p2"))
      .then(connect("p1"))
      .then(connect("p2"))
      .then(takeSpawnLock("p1"))
      .then(takeSpawnLock("p2"))
      .then(spawn("p1"))
      .then(connect("p2"))
      .then(lock("d1"))
      .then(listen("d1"))
      .then(handshake("p1"))
      .then(takeSpawnLock("p2"))
      .then(all { spawn("p2"), assert(listening) })
      .then(connect("p1"))
      .then(connect("p2"))
      .then(accept("d1", "c1"))
      .then(accept("d1", "c2"))
      .then(answer("d1", "c1"))
      .then(answer("d1", "c2"))
      .expect(and {
        // one daemon was ever started; the second spawn found it listening
        phaseOf("d1") == "serving", phaseOf("d2") == "none", phaseOf("d3") == "none",
        spawnLock == "",
        PROXIES.forall(p => ph(p) == "ready" and not(proxies.get(p).want)),
        allInvariants,
      })

  // While p1 holds the spawn lock, p2 is refused it and waits (connects again).
  run spawnLockRefusedTest =
    initWith(false)
      .then(connect("p1"))
      .then(connect("p2"))
      .then(takeSpawnLock("p1"))
      .then(takeSpawnLock("p2"))
      .expect(and { spawnLock == "p1", ph("p2") == "disconnected", allInvariants })

  run crashWithOpenDocumentsTest =
    initWith(false)
      .then(didOpen("p1", "a"))
      .then(connect("p1"))
      .then(takeSpawnLock("p1"))
      .then(spawn("p1"))
      .then(lock("d1"))
      .then(listen("d1"))
      .then(handshake("p1"))
      .then(connect("p1"))
      .then(connect("p2"))
      .then(accept("d1", "c1"))
      .then(accept("d1", "c2"))
      .then(didOpen("p2", "b"))
      .then(quit("p2"))
      .then(request("p1"))
      .then(all {
        // before the crash: p1's overlay and request, p2's overlay awaiting its EOF
        assert(conns.get("c1").overlays == Set("a") and conns.get("c1").inflight),
        assert(cs("c2") == "peerClosed" and conns.get("c2").overlays == Set("b")),
        crash("d1"),
      })
      .then(notice("p1"))
      .then(all { assert(sock.present and sock.owner == "d1"), connect("p1") })
      .then(all { assert(ph("p1") == "noDaemon"), takeSpawnLock("p1") })
      .then(spawn("p1"))
      .then(lock("d2"))
      .then(unlinkStale("d2"))
      .then(listen("d2"))
      .then(handshake("p1"))
      .then(connect("p1"))
      .then(all { assert(CONNS.forall(c => conns.get(c).overlays == Set())), accept("d2", "c1") })
      .then(answer("d2", "c1"))
      .expect(and {
        conns.get("c1").daemon == "d2", conns.get("c1").overlays == Set("a"),
        cs("c2") == "free", conns.get("c2").overlays == Set(),
        not(proxies.get("p1").want), phaseOf("d1") == "crashed", sock.owner == "d2",
        allInvariants,
      })

  run idleExitRacesConnectTest =
    initWith(false)
      .then(request("p1"))
      .then(connect("p1"))
      .then(takeSpawnLock("p1"))
      .then(spawn("p1"))
      .then(lock("d1"))
      .then(listen("d1"))
      .then(handshake("p1"))
      .then(connect("p1"))
      .then(accept("d1", "c1"))
      .then(answer("d1", "c1"))
      .then(quit("p1"))
      .then(reap("d1", "c1"))
      .then(request("p2"))
      .then(connect("p2"))
      .then(all { assert(cs("c1") == "backlog" and idle("d1")), idleExit("d1") })
      .then(all { assert(cs("c1") == "closed"), notice("p2") })
      .then(all { assert(sock.present), connect("p2") })
      .then(all { assert(ph("p2") == "noDaemon"), takeSpawnLock("p2") })
      .then(spawn("p2"))
      .then(lock("d2"))
      .then(all { assert(phaseOf("d2") == "failed" and isOwner("d1")), handshake("p2") })
      .then(unlink("d1"))
      .then(exit("d1"))
      .then(connect("p2"))
      .then(all { assert(not(sock.present) and ph("p2") == "noDaemon"), takeSpawnLock("p2") })
      .then(spawn("p2"))
      .then(lock("d1"))
      .then(listen("d1"))
      .then(handshake("p2"))
      .then(connect("p2"))
      .then(accept("d1", "c1"))
      .then(answer("d1", "c1"))
      .expect(and { not(proxies.get("p2").want), isOwner("d1"), phaseOf("d2") == "failed", allInvariants })

  // The idle exit is refused while a connection is open (accepted).
  run idleExitWaitsForOpenConnectionsTest =
    initWith(false)
      .then(connect("p1"))
      .then(takeSpawnLock("p1"))
      .then(spawn("p1"))
      .then(lock("d1"))
      .then(listen("d1"))
      .then(handshake("p1"))
      .then(connect("p1"))
      .then(accept("d1", "c1"))
      .then(quit("p1"))
      .then(idleExit("d1"))
      .fail()

  run sigtermAfterStaleStartTest =
    initWith(true)
      .then(connect("p1"))
      .then(all { assert(ph("p1") == "noDaemon"), takeSpawnLock("p1") })
      .then(spawn("p1"))
      .then(lock("d1"))
      .then(all { assert(lockFile == 1 and daemons.get("d1").lock == 1), unlinkStale("d1") })
      .then(listen("d1"))
      .then(handshake("p1"))
      .then(connect("p1"))
      .then(accept("d1", "c1"))
      .then(sigterm("d1"))
      .then(notice("p1"))
      .then(unlink("d1"))
      .then(exit("d1"))
      .then(connect("p1"))
      .expect(and {
        // nothing left: no socket, no flock, the lock file kept
        not(sock.present), not(held(1)), lockFile == 1,
        ph("p1") == "noDaemon", allInvariants,
      })

  // A daemon that finds the lock held fails, and its spawner's handshake reports it.
  run secondDaemonFailsTheLockTest =
    initWith(false)
      .then(connect("p1"))
      .then(takeSpawnLock("p1"))
      .then(spawn("p1"))
      .then(spawnTimeout("p1"))
      .then(connect("p1"))
      .then(takeSpawnLock("p1"))
      .then(spawn("p1"))
      .then(lock("d1"))
      .then(lock("d2"))
      .then(handshake("p1"))
      .expect(and { phaseOf("d2") == "failed", isOwner("d1"), spawnLock == "", allInvariants })

  // A document closed through a ready proxy leaves the daemon too; nothing closed is kept.
  run didCloseDropsTheOverlayTest =
    initWith(false)
      .then(didOpen("p1", "a"))
      .then(didOpen("p1", "b"))
      .then(connect("p1"))
      .then(takeSpawnLock("p1"))
      .then(spawn("p1"))
      .then(lock("d1"))
      .then(listen("d1"))
      .then(handshake("p1"))
      .then(connect("p1"))
      .then(accept("d1", "c1"))
      .then(didClose("p1", "a"))
      .expect(and { conns.get("c1").overlays == Set("b"), allInvariants })

  // A swept socket: the daemon notices it on its tick and stops; the next spawn serves.
  run sweptSocketStopsTest =
    initWith(false)
      .then(connect("p1"))
      .then(takeSpawnLock("p1"))
      .then(spawn("p1"))
      .then(lock("d1"))
      .then(listen("d1"))
      .then(handshake("p1"))
      .then(connect("p1"))
      .then(accept("d1", "c1"))
      .then(sweep)
      .then(request("p2"))
      .then(connect("p2"))
      .then(takeSpawnLock("p2"))
      .then(spawn("p2"))
      .then(all { assert(swept("d1") and not(listening)), lock("d2") })
      .then(all { assert(phaseOf("d2") == "failed" and isOwner("d1")), handshake("p2") })
      .then(noticeSwept("d1"))
      .then(notice("p1"))
      .then(all { assert(cs("c1") == "free" and swept("d1")), unlink("d1") })
      .then(exit("d1"))
      .then(connect("p2"))
      .then(takeSpawnLock("p2"))
      .then(spawn("p2"))
      .then(lock("d1"))
      .then(listen("d1"))
      .then(handshake("p2"))
      .then(connect("p2"))
      .then(accept("d1", "c1"))
      .then(answer("d1", "c1"))
      .expect(and {
        not(proxies.get("p2").want), isOwner("d1"), sock == { present: true, owner: "d1" },
        phaseOf("d2") == "failed", allInvariants,
      })

  // ---- Broken-model controls (ver §6.1): each must violate its named invariant ----

  // On a held lock, takes it for stale (a pid file says the holder is gone, the file is old):
  // unlinks daemon.lock and locks a new file. The old owner still holds its flock on the old
  // inode, so two daemons hold "the" lock.
  action lockBreakingStale(daemon: str): bool = {
    val inode = if (lockFile == 0) 1 else lockFile
    all {
      lockEnabled(daemon),
      if (held(inode)) all {
        inode < MAX_INODE,
        lockFile' = inode + 1,
        daemons' = daemons.set(daemon, { phase: "locked", lock: inode + 1 }),
      } else all {
        lockFile' = inode,
        daemons' = daemons.set(daemon, { phase: "locked", lock: inode }),
      },
      sock' = sock, spawnLock' = spawnLock, conns' = conns, proxies' = proxies, t' = t + 1,
    }
  }

  // Binds before the lock: takes whatever socket file is there for stale, unlinks it, binds and
  // serves; the lock comes later (and may fail).
  action listenBeforeLock(daemon: str): bool = all {
    phaseOf(daemon) == "started",
    sock' = { present: true, owner: daemon },
    daemons' = daemons.setBy(daemon, x => { ...x, phase: "serving" }),
    lockFile' = lockFile, spawnLock' = spawnLock, conns' = conns, proxies' = proxies, t' = t + 1,
  }

  // Keeps overlays across a restart: the dead daemon's overlays survive it (as if persisted for
  // the next daemon to restore), keyed by connections that no longer exist.
  action crashKeepingOverlays(daemon: str): bool = all {
    alive(daemon),
    daemons' = daemons.set(daemon, { phase: "crashed", lock: 0 }),
    conns' = CONNS.mapBy(c => {
      val k = conns.get(c)
      if (k.state == "free" or k.daemon != daemon) k
      else { ...k, state: "closed", inflight: false }
    }),
    lockFile' = lockFile, sock' = sock, spawnLock' = spawnLock, proxies' = proxies, t' = t + 1,
  }

  // Reads a closed connection's EOF but keeps its overlays (state keyed by document, not by
  // connection).
  action reapKeepingOverlays(daemon: str, conn: str): bool = all {
    reapEnabled(daemon, conn),
    conns' = conns.set(conn, { ...FREE_CONN, overlays: conns.get(conn).overlays }),
    daemons' = daemons, lockFile' = lockFile, sock' = sock, spawnLock' = spawnLock, proxies' = proxies, t' = t + 1,
  }

  // Exits from the shutdown without removing the socket (a `process::exit` in the signal path).
  action exitWithoutUnlink(daemon: str): bool = all {
    phaseOf(daemon) == "stopping",
    daemons' = daemons.set(daemon, { phase: "exited", lock: 0 }),
    lockFile' = lockFile, sock' = sock, spawnLock' = spawnLock, conns' = conns, proxies' = proxies, t' = t + 1,
  }

  // Exits while the lock's open file description lives on in a child (a worker that inherited
  // the fd without close-on-exec and outlives the daemon): the flock stays held.
  action exitLeakingLock(daemon: str): bool = all {
    exitEnabled(daemon),
    daemons' = daemons.setBy(daemon, x => { ...x, phase: "exited" }),
    lockFile' = lockFile, sock' = sock, spawnLock' = spawnLock, conns' = conns, proxies' = proxies, t' = t + 1,
  }

  // Plan §7.2's negative control: releases the lock before removing the socket. A new owner
  // can lock, unlink the old file and bind; the old daemon's unlink then removes the new socket.
  action releaseBeforeUnlink(daemon: str): bool = all {
    phaseOf(daemon) == "stopping",
    daemons' = daemons.set(daemon, { phase: "released", lock: 0 }),
    lockFile' = lockFile, sock' = sock, spawnLock' = spawnLock, conns' = conns, proxies' = proxies, t' = t + 1,
  }
  action unlinkAfterRelease(daemon: str): bool = all {
    phaseOf(daemon) == "released",
    sock' = NO_SOCK,
    daemons' = daemons.setBy(daemon, x => { ...x, phase: "exited" }),
    lockFile' = lockFile, spawnLock' = spawnLock, conns' = conns, proxies' = proxies, t' = t + 1,
  }

  // Accepts a reconnecting proxy without its documents and its request re-sent.
  action acceptWithoutResend(daemon: str, conn: str): bool = {
    val k = conns.get(conn)
    all {
      acceptEnabled(daemon, conn),
      conns' = conns.set(conn, { ...k, state: "open" }),
      proxies' = proxies.setBy(k.proxy, x => { ...x, phase: "ready" }),
      daemons' = daemons, lockFile' = lockFile, sock' = sock, spawnLock' = spawnLock, t' = t + 1,
    }
  }

  // The liveness control: binds without unlinking a socket file it finds. After any crash the
  // stale file makes every later bind fail (EADDRINUSE): each new daemon reports the error and
  // exits, and no proxy is served again. No safety invariant notices: EventuallyServed does.
  action listenWithoutUnlinkingStale(daemon: str): bool = all {
    phaseOf(daemon) == "locked",
    if (sock.present) all {
      sock' = sock,
      daemons' = daemons.set(daemon, { phase: "failed", lock: 0 }),
    } else all {
      sock' = { present: true, owner: daemon },
      daemons' = daemons.setBy(daemon, x => { ...x, phase: "serving" }),
    },
    lockFile' = lockFile, spawnLock' = spawnLock, conns' = conns, proxies' = proxies, t' = t + 1,
  }

  run lockBreakingStaleViolatesAtMostOneOwnerTest =
    initWith(false)
      .then(connect("p1"))
      .then(takeSpawnLock("p1"))
      .then(spawn("p1"))
      .then(spawnTimeout("p1"))
      .then(connect("p1"))
      .then(takeSpawnLock("p1"))
      .then(spawn("p1"))
      .then(lock("d1"))
      .then(lockBreakingStale("d2"))
      .expect(not(AtMostOneOwner))

  run listenBeforeLockViolatesServedOnlyByOwnerTest =
    initWith(false)
      .then(connect("p1"))
      .then(takeSpawnLock("p1"))
      .then(spawn("p1"))
      .then(listenBeforeLock("d1"))
      .then(handshake("p1"))
      .then(connect("p1"))
      .then(accept("d1", "c1"))
      .expect(not(ServedOnlyByOwner))

  run crashKeepingOverlaysViolatesNoGhostBuffersTest =
    initWith(false)
      .then(didOpen("p1", "a"))
      .then(connect("p1"))
      .then(takeSpawnLock("p1"))
      .then(spawn("p1"))
      .then(lock("d1"))
      .then(listen("d1"))
      .then(handshake("p1"))
      .then(connect("p1"))
      .then(accept("d1", "c1"))
      .then(all { assert(NoGhostBuffers), crashKeepingOverlays("d1") })
      .expect(not(NoGhostBuffers))

  run reapKeepingOverlaysViolatesNoGhostBuffersTest =
    initWith(false)
      .then(didOpen("p1", "a"))
      .then(connect("p1"))
      .then(takeSpawnLock("p1"))
      .then(spawn("p1"))
      .then(lock("d1"))
      .then(listen("d1"))
      .then(handshake("p1"))
      .then(connect("p1"))
      .then(accept("d1", "c1"))
      .then(quit("p1"))
      .then(all { assert(NoGhostBuffers), reapKeepingOverlays("d1", "c1") })
      .expect(not(NoGhostBuffers))

  run exitWithoutUnlinkViolatesShutdownReleasesTest =
    initWith(false)
      .then(connect("p1"))
      .then(takeSpawnLock("p1"))
      .then(spawn("p1"))
      .then(lock("d1"))
      .then(listen("d1"))
      .then(sigterm("d1"))
      .then(exitWithoutUnlink("d1"))
      .expect(not(ShutdownReleases))

  run exitLeakingLockViolatesShutdownReleasesTest =
    initWith(false)
      .then(connect("p1"))
      .then(takeSpawnLock("p1"))
      .then(spawn("p1"))
      .then(lock("d1"))
      .then(listen("d1"))
      .then(sigterm("d1"))
      .then(unlink("d1"))
      .then(exitLeakingLock("d1"))
      .expect(not(ShutdownReleases))

  run releaseBeforeUnlinkViolatesOwnerReachableTest =
    initWith(false)
      .then(connect("p1"))
      .then(takeSpawnLock("p1"))
      .then(spawn("p1"))
      .then(lock("d1"))
      .then(listen("d1"))
      .then(handshake("p1"))
      .then(sigterm("d1"))
      .then(releaseBeforeUnlink("d1"))
      .then(connect("p2"))
      .then(takeSpawnLock("p2"))
      .then(spawn("p2"))
      .then(lock("d2"))
      .then(unlinkStale("d2"))
      .then(listen("d2"))
      .then(all { assert(allInvariants), unlinkAfterRelease("d1") })
      .expect(not(OwnerReachable))

  // The same interleaving in the real order: the new daemon cannot lock until the old one has
  // removed its socket, so the old one never removes the new socket.
  run unlinkBeforeReleaseKeepsOwnerReachableTest =
    initWith(false)
      .then(connect("p1"))
      .then(takeSpawnLock("p1"))
      .then(spawn("p1"))
      .then(lock("d1"))
      .then(listen("d1"))
      .then(handshake("p1"))
      .then(sigterm("d1"))
      .then(connect("p2"))
      .then(takeSpawnLock("p2"))
      .then(spawn("p2"))
      .then(lock("d2"))
      .then(all { assert(phaseOf("d2") == "failed"), unlink("d1") })
      .then(exit("d1"))
      .then(handshake("p2"))
      .then(connect("p2"))
      .then(takeSpawnLock("p2"))
      .then(spawn("p2"))
      .then(lock("d1"))
      .then(listen("d1"))
      .expect(and { OwnerReachable, allInvariants })

  run acceptWithoutResendViolatesReconnectRestoresTest =
    initWith(false)
      .then(didOpen("p1", "a"))
      .then(request("p1"))
      .then(connect("p1"))
      .then(takeSpawnLock("p1"))
      .then(spawn("p1"))
      .then(lock("d1"))
      .then(listen("d1"))
      .then(handshake("p1"))
      .then(connect("p1"))
      .then(acceptWithoutResend("d1", "c1"))
      .expect(not(ReconnectRestores))

  // The liveness control's cycle: from a stale socket, each spawn's daemon fails its bind and
  // the proxy, still waiting, is back where it started (EventuallyServed's counterexample).
  run listenWithoutUnlinkingStaleNeverServesTest =
    initWith(true)
      .then(request("p1"))
      .then(connect("p1"))
      .then(takeSpawnLock("p1"))
      .then(spawn("p1"))
      .then(lock("d1"))
      .then(listenWithoutUnlinkingStale("d1"))
      .then(all { assert(phaseOf("d1") == "failed"), handshake("p1") })
      .then(connect("p1"))
      .then(takeSpawnLock("p1"))
      .then(spawn("p1"))
      .then(lock("d1"))
      .then(listenWithoutUnlinkingStale("d1"))
      .then(handshake("p1"))
      .expect(and {
        ph("p1") == "disconnected", proxies.get("p1").want, sock == { present: true, owner: "" },
        phaseOf("d1") == "failed", allInvariants,
      })

  // The swept socket's liveness control as a cycle: a daemon that never notices the sweep keeps
  // the lock, so every spawn fails it and a waiting proxy is back where it started (the control
  // `progressStepIgnoringSweep` below; EventuallyServed's counterexample).
  run sweptUnnoticedNeverServesTest =
    initWith(false)
      .then(connect("p1"))
      .then(takeSpawnLock("p1"))
      .then(spawn("p1"))
      .then(lock("d1"))
      .then(listen("d1"))
      .then(handshake("p1"))
      .then(connect("p1"))
      .then(accept("d1", "c1"))
      .then(sweep)
      .then(request("p2"))
      .then(connect("p2"))
      .then(takeSpawnLock("p2"))
      .then(spawn("p2"))
      .then(lock("d2"))
      .then(handshake("p2"))
      .then(connect("p2"))
      .then(takeSpawnLock("p2"))
      .then(spawn("p2"))
      .then(lock("d2"))
      .then(handshake("p2"))
      .expect(and {
        ph("p2") == "disconnected", proxies.get("p2").want, swept("d1"), phaseOf("d1") == "serving",
        phaseOf("d2") == "failed", phaseOf("d3") == "none", allInvariants,
      })

  // ---- EventuallyServed as a simulation (doc/formal-models.md, Liveness) ----
  //
  // Apalache cannot check the temporal property at a bound that reaches its counterexamples, so
  // this checks what it reduces to. Under weak fairness of every proxy, daemon and connection
  // move, once faults and the clients stop, a proxy waiting for an answer is served unless the
  // system can stop with it still waiting (a deadlock) or move forever without serving it (a
  // livelock). A run (`--step progressStep`) takes `PREFIX` random steps of `step`, faults and
  // clients included, then only system moves, each picked at random among the enabled ones,
  // until nothing is enabled; `Progress` fails if that takes more than `RECOVERY` moves or ends
  // with a proxy still waiting. (It lives in this module, with the clock `t`, because Apalache
  // cannot read a file whose IR holds a module that imports another.)
  pure val PREFIX = 30
  pure val RECOVERY = 60

  def quiescent: bool = and {
    PROXIES.forall(p => not(proxyMoveEnabled(p))),
    DAEMONS.forall(d => not(daemonMoveEnabled(d))),
    CONNS.forall(c => not(connMoveEnabled(c))),
  }

  action systemMove = any {
    nondet proxy = PROXIES.oneOf() proxyMove(proxy),
    nondet daemon = DAEMONS.oneOf() daemonMove(daemon),
    nondet conn = CONNS.oneOf() connMove(conn),
  }

  // Nothing is enabled: the run stays put, so `Progress` is checked at its end.
  action rest = all {
    quiescent,
    daemons' = daemons, lockFile' = lockFile, sock' = sock, spawnLock' = spawnLock,
    conns' = conns, proxies' = proxies, t' = t + 1,
  }

  action progressStep = if (t < PREFIX) step else any { systemMove, rest }

  val Progress = t >= PREFIX + RECOVERY implies and {
    quiescent,
    PROXIES.forall(p => not(proxies.get(p).want)),
  }

  val allProgress = and { allInvariants, Progress }

  // The liveness control: the same runs with a daemon that binds without unlinking a stale
  // socket (`listenWithoutUnlinkingStale`). `Progress` must fail: after a crash, or from a
  // stale start, every spawn fails its bind and the waiting proxy cycles forever.
  action daemonMoveStaleBlocks(daemon: str): bool = any {
    lock(daemon), listenWithoutUnlinkingStale(daemon), unlink(daemon), exit(daemon),
  }
  action stepStaleBlocks = any {
    clientStep,
    proxyStep,
    any {
      nondet daemon = DAEMONS.oneOf() daemonMoveStaleBlocks(daemon),
      nondet daemon = DAEMONS.oneOf() nondet conn = CONNS.oneOf() accept(daemon, conn),
      nondet daemon = DAEMONS.oneOf() nondet conn = CONNS.oneOf() answer(daemon, conn),
      nondet daemon = DAEMONS.oneOf() nondet conn = CONNS.oneOf() reap(daemon, conn),
      faultStep,
    },
  }
  action progressStepStaleBlocks =
    if (t < PREFIX) stepStaleBlocks else any {
      nondet proxy = PROXIES.oneOf() proxyMove(proxy),
      nondet daemon = DAEMONS.oneOf() daemonMoveStaleBlocks(daemon),
      nondet conn = CONNS.oneOf() connMove(conn),
      rest,
    }

  // The swept socket's liveness control, run by hand (doc/formal-models.md, Liveness): the same
  // runs with a daemon that never notices its socket removed. `Progress` must fail: after a
  // sweep, every spawn fails the lock the unnoticing daemon keeps, and a proxy cycles forever.
  action daemonMoveIgnoringSweep(daemon: str): bool = any {
    lock(daemon), unlinkStale(daemon), listen(daemon), unlink(daemon), exit(daemon),
  }
  action stepIgnoringSweep = any {
    clientStep,
    proxyStep,
    any {
      nondet daemon = DAEMONS.oneOf() daemonMoveIgnoringSweep(daemon),
      nondet daemon = DAEMONS.oneOf() nondet conn = CONNS.oneOf() accept(daemon, conn),
      nondet daemon = DAEMONS.oneOf() nondet conn = CONNS.oneOf() answer(daemon, conn),
      nondet daemon = DAEMONS.oneOf() nondet conn = CONNS.oneOf() reap(daemon, conn),
      faultStep,
    },
  }
  action progressStepIgnoringSweep =
    if (t < PREFIX) stepIgnoringSweep else any {
      nondet proxy = PROXIES.oneOf() proxyMove(proxy),
      nondet daemon = DAEMONS.oneOf() daemonMoveIgnoringSweep(daemon),
      nondet conn = CONNS.oneOf() connMove(conn),
      rest,
    }
}
