// The diagnostics model (FORMAL-05; plan §3.1, §3.2, §3.4, §7.3; ADR 0007, ADR 0036): documents
// an LSP client opens, changes and closes, and an agent rewrites on disk; a server that sees disk
// writes late (its watcher or the freshness stat), keeps the newer of an overlay and the disk,
// counts each document's inputs in epochs, and starts a compile job whenever a document's inputs
// change; a javac worker that runs several jobs at once, finishes them in any order, crashes,
// times out and restarts; and the `textDocument/publishDiagnostics` notifications the client
// receives. It is the contract the diagnostics coordinator of LSP-04 conforms to;
// doc/formal-models.md states its stimuli, the observation the driver produces and the invariants.
//
// The design it fixes (doc/formal-models.md):
// - A document's diagnostics depend on its own text and on its dependencies' (`a` imports `b`,
//   and reads `c` only while `c` exists: javac's negative lookup of a name it cannot find): a
//   change to `b`, or `c` created or deleted, makes `a`'s diagnostics stale though `a`'s LSP
//   version did not move. So the
//   coordinator orders results by an input epoch per document, bumped on every change of any of
//   its inputs, never by the LSP version alone.
// - LSP versions restart when a document is reopened (a client may start again at 1), so a result
//   computed in an earlier open session is never published in a later one.
// - Every change of a document's inputs makes a compile pending at once, so plan §3.2's rule
//   (drop a result older than the newest published or pending one) publishes a result only when
//   it was computed for the document's current inputs.
// - A publication carries the LSP version whose text it was computed from; diagnostics of disk
//   content newer than the client's overlay (disk wins, plan §2.5) carry no version.
// - Only open documents get diagnostics. Closing publishes an empty set, and nothing follows it
//   until the document is opened again.
module diagnostics {
  // ---- The world ----

  // Three files of one worktree and one `EnvironmentKey` (one worker, ADR 0036). `a` imports
  // `b`, so `a`'s compile reads `b`'s text; `b` depends on nothing. `a` also names a class `C`
  // that javac looks up: `a`'s compile reads `c` while `c` exists, and fails "cannot find symbol"
  // while it does not (a negative lookup, an input too). The client opens `a` and `b` (`DOCS`);
  // `c` is only ever created, written and deleted on disk, by an agent (`FILES`). The server's
  // view of `c` is one number, `seenC`, not a third document: a third entry in every map made
  // Apalache several times slower per step. Three job slots.
  pure val DOCS: Set[str] = Set("a", "b")
  pure val FILES: Set[str] = Set("a", "b", "c")
  // The versions a `didOpen` may carry: 1 for a fresh open, more when a proxy re-sends a document
  // the client already holds at a later version (a daemon restart, plan §5.2).
  pure val OPEN_VERSIONS: Set[int] = Set(1, 2, 3)
  pure val JOBS: Set[str] = Set("j1", "j2", "j3")
  // Every text (a disk write, an editor change) takes the next number, so a larger text is a
  // newer one; the texts on disk at the start are 1 (`a`) and 2 (`b`), and `c` is absent (text
  // 0). An action that would pass a bound is disabled.
  pure val MAX_TEXT = 14
  pure val MAX_VERSION = 6
  pure val MAX_SESSION = 3
  // Worker incarnations: the first spawn is 1. A crash is enabled only below the bound, so a
  // restart is always possible after one.
  pure val MAX_INC = 3
  // `publishDiagnostics` without a `version` (LSP's field is optional).
  pure val NO_VERSION = -1

  // ---- Types ----

  // The texts a compile reads, 0 for a file it does not read or that is absent: `a`'s compile
  // reads `a`, `b` and `c` (0 while `c` is absent), `b`'s reads only `b`.
  type Inputs = { a: int, b: int, c: int }
  // The server's state of a document:
  // - open, session: whether the client has it open, and how many times it was opened;
  // - version: the LSP version of the client's latest text in this session (the didOpen's, then
  //   one more per change);
  // - overlay: the text of that version (0 when closed);
  // - seenDisk: the disk text the server has seen (watcher or freshness stat), which can lag the
  //   disk;
  // - epoch: bumped on every change of the document's inputs, and at every didOpen;
  // - dirty: a compile of the current epoch is due and not started.
  type Doc = { open: bool, session: int, version: int, overlay: int, seenDisk: int, epoch: int, dirty: bool }
  // A compile job. `state`: free, running (sent to the worker), done (its reply is in the
  // worker's pipe, not read yet). `session`, `epoch`, `inputs` and `version` (the version tag it
  // will be published with) are what the coordinator recorded when it started the job; `inc` is
  // the worker incarnation that runs it; `late`: it timed out and was compiled again.
  type Job = { state: str, doc: str, session: int, version: int, epoch: int, inputs: Inputs, inc: int, late: bool }
  // The javac worker of the environment: up or not, its incarnation, and how many running jobs
  // its crashes lost.
  type Worker = { up: bool, inc: int, lost: int }
  // The client's diagnostics of a document: the last `publishDiagnostics` it received. `kind`:
  // none (nothing yet), cleared (the empty set a close publishes), diags (a compile's result).
  // `version` is the notification's version field; `session`, `epoch`, `inputs` and `inc` are
  // what the diagnostics were computed for (the driver's scripted compiler writes them into the
  // diagnostics it returns).
  type Pub = { kind: str, version: int, session: int, epoch: int, inputs: Inputs, inc: int }
  // Ghost: the highest epoch and version published before the client's latest diagnostics in the
  // document's current session (-1: none).
  type High = { epoch: int, version: int }
  // Ghost: a text the client sent as a version of a document in a session.
  type Sent = { doc: str, session: int, version: int, text: int }

  pure val NO_INPUTS: Inputs = { a: 0, b: 0, c: 0 }
  pure val FREE_JOB: Job = { state: "free", doc: "", session: 0, version: 0, epoch: 0, inputs: NO_INPUTS, inc: 0, late: false }
  pure val NO_PUB: Pub = { kind: "none", version: NO_VERSION, session: 0, epoch: 0, inputs: NO_INPUTS, inc: 0 }
  pure val CLEARED: Pub = { ...NO_PUB, kind: "cleared" }
  pure val NO_HIGH: High = { epoch: -1, version: -1 }

  // ---- State ----

  var disk: str -> int       // the text on disk, 0 for an absent file
  var seenC: int             // the text of `c` the server has seen, 0 while it sees none
  var docs: str -> Doc       // the coordinator, per document
  var worker: Worker
  var jobs: str -> Job
  var client: str -> Pub     // what the client shows, per document
  var high: str -> High      // ghost, for NeverOlderOverNewer
  var sinceEdit: str -> bool // ghost: a publication arrived after the latest change of the inputs
  var sent: Set[Sent]        // ghost: every text the client sent, for PublishedMatchesVersion
  var nextText: int          // the next text's number
  var t: int                 // the steps taken since init (the driver's step index): the
                             // liveness simulation's clock, `Progress` below

  // ---- Helpers ----

  pure def maxOf(x: int, y: int): int = if (x >= y) x else y

  // The text the server compiles for a document: the overlay while it is open and not older than
  // the disk the server has seen (plan §2.5: disk wins over an older overlay), else that disk.
  pure def effIn(ds: str -> Doc, d: str): int = {
    val x = ds.get(d)
    if (x.open and x.overlay >= x.seenDisk) x.overlay else x.seenDisk
  }
  // `sc`: the text of `c` the server has seen, 0 while it sees no `c`.
  pure def inputsIn(ds: str -> Doc, sc: int, d: str): Inputs =
    { a: if (d == "a") effIn(ds, "a") else 0, b: effIn(ds, "b"), c: if (d == "a") sc else 0 }
  pure def own(i: Inputs, d: str): int = if (d == "a") i.a else if (d == "b") i.b else i.c
  // The version a compile of `d` is published with: the client's version when the overlay is what
  // it compiles, none when the disk has overtaken the overlay.
  pure def tagIn(ds: str -> Doc, d: str): int = {
    val x = ds.get(d)
    if (x.overlay >= x.seenDisk) x.version else NO_VERSION
  }
  // The documents whose inputs changed from `ds0` to `ds1`, and those forced (a didOpen).
  pure def changedWith(ds0: str -> Doc, sc0: int, ds1: str -> Doc, sc1: int, forced: Set[str]): Set[str] =
    DOCS.filter(d => forced.contains(d) or inputsIn(ds1, sc1, d) != inputsIn(ds0, sc0, d))
  def changed(ds0: str -> Doc, ds1: str -> Doc, forced: Set[str]): Set[str] =
    changedWith(ds0, seenC, ds1, seenC, forced)
  // Every changed document takes a new epoch, and an open one a pending compile.
  pure def bump(ds: str -> Doc, ch: Set[str]): str -> Doc =
    DOCS.mapBy(d => {
      val x = ds.get(d)
      if (ch.contains(d)) { ...x, epoch: x.epoch + 1, dirty: x.open } else x
    })
  def staleAfter(ch: Set[str]): str -> bool = DOCS.mapBy(d => if (ch.contains(d)) false else sinceEdit.get(d))

  // A free job slot names no document; these read it as a closed one with no diagnostics.
  pure val NO_DOC: Doc = { open: false, session: 0, version: 0, overlay: 0, seenDisk: 0, epoch: 0, dirty: false }
  def docOf(d: str): Doc = if (DOCS.contains(d)) docs.get(d) else NO_DOC
  def pubOf(d: str): Pub = if (DOCS.contains(d)) client.get(d) else NO_PUB
  def jobFree(j: str): bool = jobs.get(j).state == "free"
  def freeJob: str = if (jobFree("j1")) "j1" else if (jobFree("j2")) "j2" else if (jobFree("j3")) "j3" else ""
  // A job computed for its document's current inputs, in its current open session.
  def current(k: Job): bool = {
    val x = docOf(k.doc)
    and { x.open, k.session == x.session, k.epoch == x.epoch }
  }
  // The client's diagnostics of `d` are for its current inputs: its current epoch in its current
  // session, and the texts every input has now (so an input the epoch failed to track is caught).
  def upToDate(d: str): bool = {
    val p = client.get(d)
    val x = docs.get(d)
    and { p.kind == "diags", p.session == x.session, p.epoch == x.epoch, p.inputs == inputsIn(docs, seenC, d) }
  }

  // ---- Initial state ----

  // Both documents on disk and closed; the worker not spawned yet (plan §3.4: lazily, on the
  // first edit).
  action init = all {
    disk' = Map("a" -> 1, "b" -> 2, "c" -> 0),
    docs' = Map(
      "a" -> { open: false, session: 0, version: 0, overlay: 0, seenDisk: 1, epoch: 0, dirty: false },
      "b" -> { open: false, session: 0, version: 0, overlay: 0, seenDisk: 2, epoch: 0, dirty: false },
    ),
    worker' = { up: false, inc: 0, lost: 0 },
    jobs' = JOBS.mapBy(_ => FREE_JOB),
    client' = DOCS.mapBy(_ => NO_PUB),
    high' = DOCS.mapBy(_ => NO_HIGH),
    sinceEdit' = DOCS.mapBy(_ => false),
    sent' = Set(),
    nextText' = 3,
    seenC' = 0,
    t' = 0,
  }

  // ---- The client and the agent ----

  // `didOpen`: the client opens the document with the text it read from disk, at `version`: 1 for
  // a fresh open (a client may start every open session at 1 again), or the client's current
  // version when a proxy re-sends the document to a restarted daemon, which never renumbers it.
  // The server sees that text as the overlay, so a disk write the server has not seen yet is
  // compiled at once.
  action didOpen(doc: str, version: int): bool = {
    val x = docs.get(doc)
    val ds1 = docs.set(doc, { ...x, open: true, session: x.session + 1, version: version, overlay: disk.get(doc) })
    val ch = changed(docs, ds1, Set(doc))
    all {
      not(x.open),
      x.session < MAX_SESSION,
      disk.get(doc) != 0,
      docs' = bump(ds1, ch),
      sinceEdit' = staleAfter(ch),
      high' = high.set(doc, NO_HIGH),
      sent' = sent.union(Set({ doc: doc, session: x.session + 1, version: version, text: disk.get(doc) })),
      disk' = disk, worker' = worker, jobs' = jobs, client' = client, nextText' = nextText, seenC' = seenC, t' = t + 1,
    }
  }

  // `didChange`: an editor change the disk does not have (an unsaved buffer).
  action didChange(doc: str): bool = {
    val x = docs.get(doc)
    val ds1 = docs.set(doc, { ...x, version: x.version + 1, overlay: nextText })
    val ch = changed(docs, ds1, Set())
    all {
      x.open,
      x.version < MAX_VERSION,
      nextText < MAX_TEXT,
      docs' = bump(ds1, ch),
      sinceEdit' = staleAfter(ch),
      sent' = sent.union(Set({ doc: doc, session: x.session, version: x.version + 1, text: nextText })),
      nextText' = nextText + 1,
      disk' = disk, worker' = worker, jobs' = jobs, client' = client, high' = high, seenC' = seenC, t' = t + 1,
    }
  }

  // Claude Code's Edit tool on an open document: it writes the file, then sends the same text in
  // a `didChange`. The server sees the disk write later (`noticeDisk`), as the same text.
  action toolEdit(doc: str): bool = {
    val x = docs.get(doc)
    val ds1 = docs.set(doc, { ...x, version: x.version + 1, overlay: nextText })
    val ch = changed(docs, ds1, Set())
    all {
      x.open,
      x.version < MAX_VERSION,
      nextText < MAX_TEXT,
      disk' = disk.set(doc, nextText),
      docs' = bump(ds1, ch),
      sinceEdit' = staleAfter(ch),
      sent' = sent.union(Set({ doc: doc, session: x.session, version: x.version + 1, text: nextText })),
      nextText' = nextText + 1,
      worker' = worker, jobs' = jobs, client' = client, high' = high, seenC' = seenC, t' = t + 1,
    }
  }

  // A disk write the client does not send: an agent's shell edit (`sed -i`, a formatter, a
  // `git checkout`), open or closed. The server learns of it only through `noticeDisk`.
  action diskWrite(doc: str): bool = all {
    nextText < MAX_TEXT,
    disk' = disk.set(doc, nextText),
    nextText' = nextText + 1,
    docs' = docs, worker' = worker, jobs' = jobs, client' = client, high' = high,
    sinceEdit' = sinceEdit, sent' = sent, seenC' = seenC, t' = t + 1,
  }

  // An agent deletes the file `c` (`rm`, a `git checkout` that removes it). Only `c` comes and
  // goes: deleting an open document is not modelled.
  action diskDelete(doc: str): bool = all {
    doc == "c",
    disk.get(doc) != 0,
    disk' = disk.set(doc, 0),
    docs' = docs, worker' = worker, jobs' = jobs, client' = client, high' = high,
    sinceEdit' = sinceEdit, sent' = sent, nextText' = nextText, seenC' = seenC, t' = t + 1,
  }

  // `didClose`: the overlay is dropped and the server publishes an empty set at once. Jobs of
  // the document keep running; their results are dropped.
  action didClose(doc: str): bool = {
    val x = docs.get(doc)
    val ds1 = docs.set(doc, { ...x, open: false, overlay: 0, dirty: false })
    val ch = changed(docs, ds1, Set())
    all {
      x.open,
      docs' = bump(ds1, ch),
      sinceEdit' = staleAfter(ch),
      client' = client.set(doc, CLEARED),
      disk' = disk, worker' = worker, jobs' = jobs, high' = high, sent' = sent, nextText' = nextText, seenC' = seenC, t' = t + 1,
    }
  }

  // ---- The coordinator ----

  // The watcher or the freshness stat sees the disk's text (plan §3.2: a disk change seen by the
  // freshness stat triggers a compile): the document and every document that reads it take a new
  // epoch when their inputs change.
  def seenAfter(doc: str): (str -> Doc, int) =
    if (doc == "c") (docs, disk.get("c"))
    else (docs.set(doc, { ...docs.get(doc), seenDisk: disk.get(doc) }), seenC)
  def unseen(doc: str): bool =
    if (doc == "c") disk.get("c") != seenC else disk.get(doc) != docs.get(doc).seenDisk
  action noticeDisk(doc: str): bool = {
    val after = seenAfter(doc)
    val ch = changedWith(docs, seenC, after._1, after._2, Set())
    all {
      unseen(doc),
      docs' = bump(after._1, ch),
      seenC' = after._2,
      sinceEdit' = staleAfter(ch),
      disk' = disk, worker' = worker, jobs' = jobs, client' = client, high' = high, sent' = sent,
      nextText' = nextText, t' = t + 1,
    }
  }

  // Sends a compile of the document's current inputs to the worker. A pending compile is never
  // skipped because an older one still runs (results are ordered by epoch, not by arrival).
  def startJobEnabled(doc: str): bool = and { docs.get(doc).open, docs.get(doc).dirty, worker.up, freeJob != "" }
  def jobFor(doc: str, tag: int): Job = {
    val x = docs.get(doc)
    { state: "running", doc: doc, session: x.session, version: tag, epoch: x.epoch,
      inputs: inputsIn(docs, seenC, doc), inc: worker.inc, late: false }
  }
  action startJobWith(doc: str, tag: int): bool = all {
    startJobEnabled(doc),
    jobs' = jobs.set(freeJob, jobFor(doc, tag)),
    docs' = docs.setBy(doc, x => { ...x, dirty: false }),
    disk' = disk, worker' = worker, client' = client, high' = high, sinceEdit' = sinceEdit,
    sent' = sent, nextText' = nextText, seenC' = seenC, t' = t + 1,
  }
  action startJob(doc: str): bool = startJobWith(doc, tagIn(docs, doc))

  // The coordinator reads a reply from the worker's pipe and publishes it only when it was
  // computed for the document's current inputs in its open session; otherwise it drops it.
  def publishOf(k: Job): Pub =
    { kind: "diags", version: k.version, session: k.session, epoch: k.epoch, inputs: k.inputs, inc: k.inc }
  // The ghost `high` before publishing `n` over the client's diagnostics of `d`.
  def highAfter(d: str): High = {
    val p = client.get(d)
    val h = high.get(d)
    if (p.kind == "diags") { epoch: maxOf(h.epoch, p.epoch), version: maxOf(h.version, p.version) } else h
  }
  action deliverWith(job: str, accept: bool): bool = {
    val k = jobs.get(job)
    all {
      k.state == "done",
      jobs' = jobs.set(job, FREE_JOB),
      if (accept) all {
        client' = client.set(k.doc, publishOf(k)),
        high' = high.set(k.doc, highAfter(k.doc)),
        sinceEdit' = sinceEdit.set(k.doc, true),
      } else all {
        client' = client, high' = high, sinceEdit' = sinceEdit,
      },
      disk' = disk, docs' = docs, worker' = worker, sent' = sent, nextText' = nextText, seenC' = seenC, t' = t + 1,
    }
  }
  action deliver(job: str): bool = deliverWith(job, current(jobs.get(job)))

  // ---- The javac worker (plan §3.1, §3.4) ----

  // The worker finishes a job and writes its reply; jobs finish in any order (a worker per
  // environment, a syntax pass beside javac, a reply of an incarnation that died after writing).
  action complete(job: str): bool = {
    val k = jobs.get(job)
    all {
      k.state == "running", worker.up, k.inc == worker.inc,
      jobs' = jobs.set(job, { ...k, state: "done" }),
      disk' = disk, docs' = docs, worker' = worker, client' = client, high' = high,
      sinceEdit' = sinceEdit, sent' = sent, nextText' = nextText, seenC' = seenC, t' = t + 1,
    }
  }

  // The worker is spawned (plan §3.4: lazily, on the first edit; after a crash, once the backoff
  // has passed) while a compile is pending.
  action restart = all {
    not(worker.up),
    worker.inc < MAX_INC,
    DOCS.exists(d => docs.get(d).open and docs.get(d).dirty),
    worker' = { ...worker, up: true, inc: worker.inc + 1 },
    disk' = disk, docs' = docs, jobs' = jobs, client' = client, high' = high,
    sinceEdit' = sinceEdit, sent' = sent, nextText' = nextText, seenC' = seenC, t' = t + 1,
  }

  // The worker dies (or is killed after a hung compile). Its running jobs are lost; a reply it
  // wrote before it died is still read. The coordinator compiles again every document whose
  // current compile was lost.
  def lostBy: Set[str] = JOBS.filter(j => jobs.get(j).state == "running" and jobs.get(j).inc == worker.inc)
  action crashWith(requeue: bool): bool = {
    val lost = lostBy
    all {
      worker.up,
      worker.inc < MAX_INC,
      worker' = { ...worker, up: false, lost: worker.lost + lost.size() },
      jobs' = JOBS.mapBy(j => if (lost.contains(j)) FREE_JOB else jobs.get(j)),
      docs' = DOCS.mapBy(d => {
        val x = docs.get(d)
        if (requeue and lost.exists(j => jobs.get(j).doc == d and current(jobs.get(j)))) { ...x, dirty: true } else x
      }),
      disk' = disk, client' = client, high' = high, sinceEdit' = sinceEdit, sent' = sent,
      nextText' = nextText, seenC' = seenC, t' = t + 1,
    }
  }
  action crash = crashWith(true)

  // A compile passes its deadline: the coordinator compiles the document again and leaves the
  // late job running; its reply may still come, and is published or dropped by the same rule.
  action timeout(job: str): bool = {
    val k = jobs.get(job)
    all {
      k.state == "running", not(k.late), worker.up, k.inc == worker.inc,
      jobs' = jobs.set(job, { ...k, late: true }),
      docs' = if (current(k)) docs.setBy(k.doc, x => { ...x, dirty: true }) else docs,
      disk' = disk, worker' = worker, client' = client, high' = high, sinceEdit' = sinceEdit,
      sent' = sent, nextText' = nextText, seenC' = seenC, t' = t + 1,
    }
  }

  // ---- Every stimulus, for simulation, Apalache and the generated conformance traces ----

  // Each parameter is a nondeterministic pick named as the parameter (`doc`, `job`), so `quint
  // run --mbt` records it for the driver. The stimuli are grouped so the client's changes, the
  // coordinator and the worker are about equally likely.
  action clientStep = any {
    nondet doc = DOCS.oneOf() nondet version = OPEN_VERSIONS.oneOf() didOpen(doc, version),
    nondet doc = DOCS.oneOf() didChange(doc),
    nondet doc = DOCS.oneOf() toolEdit(doc),
    nondet doc = FILES.oneOf() diskWrite(doc),
    nondet doc = Set("c").oneOf() diskDelete(doc),
    nondet doc = DOCS.oneOf() didClose(doc),
  }

  action coordinatorStep = any {
    nondet doc = FILES.oneOf() noticeDisk(doc),
    nondet doc = DOCS.oneOf() startJob(doc),
    nondet job = JOBS.oneOf() deliver(job),
  }

  action faultStep = any {
    crash,
    nondet job = JOBS.oneOf() timeout(job),
  }

  action workerStep = any {
    nondet job = JOBS.oneOf() complete(job),
    restart,
    faultStep,
  }

  action step = any { clientStep, coordinatorStep, workerStep }

  // ---- Invariants (plan §3.2, §7.3, ver §6.1) ----

  // Within one open session, the client's diagnostics of a document never go back: each
  // publication's epoch (what it was computed for, a dependency's text included) and version are
  // at least those of every earlier one.
  val NeverOlderOverNewer = DOCS.forall(d => {
    val p = client.get(d)
    val h = high.get(d)
    p.kind == "diags" implies and {
      p.epoch >= h.epoch,
      p.version != NO_VERSION implies p.version >= h.version,
    }
  })

  // A publication that carries a version was computed from exactly the text the client sent as
  // that version in the document's current open session.
  val PublishedMatchesVersion = DOCS.forall(d => {
    val p = client.get(d)
    val x = docs.get(d)
    (x.open and p.kind == "diags" and p.version != NO_VERSION) implies
      sent.contains({ doc: d, session: x.session, version: p.version, text: own(p.inputs, d) })
  })

  // A closed document shows the empty set its close published (or nothing, never opened), so
  // nothing was published after the close.
  val ClosedPublishesNothing = DOCS.forall(d => {
    val x = docs.get(d)
    not(x.open) implies client.get(d).kind == (if (x.session == 0) "none" else "cleared")
  })

  // Plan §3.2's "dropped when older than the newest pending one", with every change pending at
  // once: diagnostics the client receives after a change of the inputs were computed for that
  // change or a newer one. A client that reads them after its edit (Claude Code reads pushed
  // diagnostics after every edit, plan §1.2 and §15) never sees those of a text it already
  // replaced, whether or not it checks `version`.
  val FreshAfterEdit = DOCS.forall(d =>
    (docs.get(d).open and sinceEdit.get(d)) implies upToDate(d)
  )

  val allInvariants = and {
    NeverOlderOverNewer,
    PublishedMatchesVersion,
    ClosedPublishesNothing,
    FreshAfterEdit,
  }

  // ---- Witnesses: states the random simulation must reach (doc/formal-models.md, Budget) ----

  // A reply older than the published diagnostics is still to be read: results in reverse order.
  val witnessReverseOrder = JOBS.exists(j => {
    val k = jobs.get(j)
    val p = pubOf(k.doc)
    and { k.state == "done", p.kind == "diags", k.session == p.session, k.epoch < p.epoch }
  })
  // A document's diagnostics replaced earlier ones of the same open session, so
  // NeverOlderOverNewer compares two publications.
  val witnessRepublished = DOCS.exists(d => client.get(d).kind == "diags" and high.get(d).epoch >= 0)
  // A crash lost a running compile.
  val witnessCrashMidCompile = worker.lost > 0
  // After a crash lost a compile, a later incarnation's diagnostics are up to date.
  val witnessCurrentAfterCrash = worker.lost > 0 and DOCS.exists(d => upToDate(d) and client.get(d).inc > 1)
  // A document was closed while its compile runs or its reply waits.
  val witnessCloseDuringCompile = JOBS.exists(j => {
    val k = jobs.get(j)
    and { k.state != "free", not(docOf(k.doc).open), k.session == docOf(k.doc).session }
  })
  // A reply for a version the client has already superseded.
  val witnessSupersededVersion = JOBS.exists(j => {
    val k = jobs.get(j)
    and { k.state == "done", k.session == docOf(k.doc).session, k.version != NO_VERSION,
          k.version < docOf(k.doc).version }
  })
  // A reply for `a` at its current version, stale only because `b` changed.
  val witnessDependencyStale = JOBS.exists(j => {
    val k = jobs.get(j)
    and { k.state == "done", k.doc == "a", docs.get("a").open, k.session == docs.get("a").session,
          k.version == tagIn(docs, "a"), k.inputs.a == effIn(docs, "a"), k.inputs.b != effIn(docs, "b") }
  })
  // A reply computed in an earlier open session of a reopened document.
  val witnessReopenStale = JOBS.exists(j => {
    val k = jobs.get(j)
    and { k.state == "done", docOf(k.doc).open, k.session < docOf(k.doc).session }
  })
  // An open document's diagnostics are of disk content its overlay has not caught up with.
  val witnessDiskWins = DOCS.exists(d => upToDate(d) and docs.get(d).open and client.get(d).version == NO_VERSION)
  // A reply for `a` that failed to find `C`, read after the server saw `c` created.
  val witnessCreatedStale = JOBS.exists(j => {
    val k = jobs.get(j)
    and { k.state == "done", k.doc == "a", docs.get("a").open, k.session == docs.get("a").session,
          k.inputs.c == 0, seenC != 0 }
  })
  // `a`'s up-to-date diagnostics read `c`, created after `a` was opened.
  val witnessReadsCreated = upToDate("a") and client.get("a").inputs.c != 0 and high.get("a").epoch >= 0
  // A timed-out compile's reply arrives.
  val witnessLateReply = JOBS.exists(j => jobs.get(j).state == "done" and jobs.get(j).late)
  // Both documents open and up to date, `a`'s computed from `b`'s overlay.
  val witnessBothUpToDate = and {
    upToDate("a"), upToDate("b"), docs.get("b").open,
    client.get("a").inputs.b == docs.get("b").overlay, docs.get("b").overlay > docs.get("b").seenDisk,
  }

  // ---- Named scenarios (ver §6.1) ----
  //
  // Each scenario is a scripted step: guarded branches of which exactly one is enabled in each
  // state of the run, so `quint run --mbt --step <scenario>` writes its regression trace with the
  // same action names and picks as `step` (formal/traces/regression/manifest.toml); its test below
  // takes the same steps as named actions. Steps are counted by `t`.

  def js(j: str): str = jobs.get(j).state

  // Two quick edits; the newer compile's reply arrives first and is published; the older one's
  // arrives last and is dropped.
  action twoQuickEditsReverseOrder = any {
    all { t == 0, nondet doc = Set("a").oneOf() nondet version = Set(1).oneOf() didOpen(doc, version) },
    all { t == 1, restart },
    all { t == 2, nondet doc = Set("a").oneOf() startJob(doc) },
    all { t == 3, nondet doc = Set("a").oneOf() didChange(doc) },
    all { t == 4, nondet doc = Set("a").oneOf() startJob(doc) },
    all { t == 5, nondet doc = Set("a").oneOf() didChange(doc) },
    all { t == 6, nondet doc = Set("a").oneOf() startJob(doc) },
    all { t == 7, nondet job = Set("j3").oneOf() complete(job) },
    all { t == 8, nondet job = Set("j1").oneOf() complete(job) },
    all { t == 9, nondet job = Set("j3").oneOf() deliver(job) },
    all { t == 10, nondet job = Set("j2").oneOf() complete(job) },
    all { t == 11, nondet job = Set("j2").oneOf() deliver(job) },
    all { t == 12, nondet job = Set("j1").oneOf() deliver(job) },
  }

  // The worker crashes mid-compile; the restarted worker compiles the document again. A reply
  // the old worker wrote before it died is still read, and dropped as stale.
  action crashMidCompile = any {
    all { t == 0, nondet doc = Set("a").oneOf() nondet version = Set(1).oneOf() didOpen(doc, version) },
    all { t == 1, restart },
    all { t == 2, nondet doc = Set("a").oneOf() startJob(doc) },
    all { t == 3, nondet job = Set("j1").oneOf() complete(job) },
    all { t == 4, nondet doc = Set("a").oneOf() toolEdit(doc) },
    all { t == 5, nondet doc = Set("a").oneOf() startJob(doc) },
    all { t == 6, crash },
    all { t == 7, restart },
    all { t == 8, nondet doc = Set("a").oneOf() startJob(doc) },
    all { t == 9, nondet job = Set("j2").oneOf() complete(job) },
    all { t == 10, nondet job = Set("j1").oneOf() deliver(job) },
    all { t == 11, nondet job = Set("j2").oneOf() deliver(job) },
    all { t == 12, nondet doc = Set("a").oneOf() noticeDisk(doc) },
  }

  // The client closes the document while its compile runs: the close publishes an empty set and
  // the reply is dropped. Reopened at version 1 again, an older session's reply stays dropped.
  action closeDuringCompile = any {
    all { t == 0, nondet doc = Set("a").oneOf() nondet version = Set(1).oneOf() didOpen(doc, version) },
    all { t == 1, restart },
    all { t == 2, nondet doc = Set("a").oneOf() startJob(doc) },
    all { t == 3, nondet doc = Set("a").oneOf() didClose(doc) },
    all { t == 4, nondet job = Set("j1").oneOf() complete(job) },
    all { t == 5, nondet job = Set("j1").oneOf() deliver(job) },
    all { t == 6, nondet doc = Set("a").oneOf() nondet version = Set(1).oneOf() didOpen(doc, version) },
    all { t == 7, nondet doc = Set("a").oneOf() startJob(doc) },
    all { t == 8, nondet doc = Set("a").oneOf() didClose(doc) },
    all { t == 9, nondet doc = Set("a").oneOf() nondet version = Set(1).oneOf() didOpen(doc, version) },
    all { t == 10, nondet doc = Set("a").oneOf() startJob(doc) },
    all { t == 11, nondet job = Set("j1").oneOf() complete(job) },
    all { t == 12, nondet job = Set("j2").oneOf() complete(job) },
    all { t == 13, nondet job = Set("j1").oneOf() deliver(job) },
    all { t == 14, nondet job = Set("j2").oneOf() deliver(job) },
  }

  // An agent rewrites `b` on disk while `a` is open: `a`'s compile at the same version is stale.
  // The two replies arrive in reverse order; only the one that read the new `b` is published.
  action dependencyEditMakesStale = any {
    all { t == 0, nondet doc = Set("a").oneOf() nondet version = Set(1).oneOf() didOpen(doc, version) },
    all { t == 1, restart },
    all { t == 2, nondet doc = Set("a").oneOf() startJob(doc) },
    all { t == 3, nondet doc = Set("b").oneOf() diskWrite(doc) },
    all { t == 4, nondet doc = Set("b").oneOf() noticeDisk(doc) },
    all { t == 5, nondet doc = Set("a").oneOf() startJob(doc) },
    all { t == 6, nondet job = Set("j2").oneOf() complete(job) },
    all { t == 7, nondet job = Set("j2").oneOf() deliver(job) },
    all { t == 8, nondet job = Set("j1").oneOf() complete(job) },
    all { t == 9, nondet job = Set("j1").oneOf() deliver(job) },
  }

  // An agent rewrites the open document on disk; once the server sees it, the disk wins over
  // the older overlay and its diagnostics carry no version. Claude Code's Edit then sends a new
  // version whose text is also on disk.
  action diskWinsOverOverlay = any {
    all { t == 0, nondet doc = Set("a").oneOf() nondet version = Set(1).oneOf() didOpen(doc, version) },
    all { t == 1, nondet doc = Set("a").oneOf() didChange(doc) },
    all { t == 2, nondet doc = Set("a").oneOf() diskWrite(doc) },
    all { t == 3, restart },
    all { t == 4, nondet doc = Set("a").oneOf() startJob(doc) },
    all { t == 5, nondet doc = Set("a").oneOf() noticeDisk(doc) },
    all { t == 6, nondet doc = Set("a").oneOf() startJob(doc) },
    all { t == 7, nondet job = Set("j2").oneOf() complete(job) },
    all { t == 8, nondet job = Set("j1").oneOf() complete(job) },
    all { t == 9, nondet job = Set("j2").oneOf() deliver(job) },
    all { t == 10, nondet job = Set("j1").oneOf() deliver(job) },
    all { t == 11, nondet doc = Set("a").oneOf() toolEdit(doc) },
    all { t == 12, nondet doc = Set("a").oneOf() startJob(doc) },
    all { t == 13, nondet doc = Set("a").oneOf() noticeDisk(doc) },
    all { t == 14, nondet job = Set("j1").oneOf() complete(job) },
    all { t == 15, nondet job = Set("j1").oneOf() deliver(job) },
  }

  // A compile passes its deadline and is compiled again; both replies come, the late one last,
  // and both are for the current inputs.
  action timeoutLateReply = any {
    all { t == 0, nondet doc = Set("b").oneOf() nondet version = Set(1).oneOf() didOpen(doc, version) },
    all { t == 1, restart },
    all { t == 2, nondet doc = Set("b").oneOf() startJob(doc) },
    all { t == 3, nondet job = Set("j1").oneOf() timeout(job) },
    all { t == 4, nondet doc = Set("b").oneOf() startJob(doc) },
    all { t == 5, nondet job = Set("j2").oneOf() complete(job) },
    all { t == 6, nondet job = Set("j2").oneOf() deliver(job) },
    all { t == 7, nondet job = Set("j1").oneOf() complete(job) },
    all { t == 8, nondet job = Set("j1").oneOf() deliver(job) },
  }

  // `a` fails to find `C`; an agent then creates `c` on disk. Once the server sees it, `a` is
  // pending though nothing it read changed, and its next diagnostics read `c`.
  action createdFileMakesStale = any {
    all { t == 0, nondet doc = Set("a").oneOf() nondet version = Set(1).oneOf() didOpen(doc, version) },
    all { t == 1, restart },
    all { t == 2, nondet doc = Set("a").oneOf() startJob(doc) },
    all { t == 3, nondet job = Set("j1").oneOf() complete(job) },
    all { t == 4, nondet job = Set("j1").oneOf() deliver(job) },
    all { t == 5, nondet doc = Set("c").oneOf() diskWrite(doc) },
    all { t == 6, nondet doc = Set("c").oneOf() noticeDisk(doc) },
    all { t == 7, nondet doc = Set("a").oneOf() startJob(doc) },
    all { t == 8, nondet job = Set("j1").oneOf() complete(job) },
    all { t == 9, nondet job = Set("j1").oneOf() deliver(job) },
  }

  // The scenario tests replay each script as named actions, step for step (a test that draws a
  // nondeterministic value runs 10000 times, so the scripts themselves serve only the traces).

  run twoQuickEditsReverseOrderTest =
    init
      .then(didOpen("a", 1))
      .then(restart)
      .then(startJob("a"))
      .then(didChange("a"))
      .then(startJob("a"))
      .then(didChange("a"))
      .then(startJob("a"))
      .then(complete("j3"))
      .then(complete("j1"))
      .then(all { deliver("j3"), assert(current(jobs.get("j3"))) })
      .then(complete("j2"))
      .then(all { deliver("j2"), assert(not(current(jobs.get("j2")))) })
      .then(all { deliver("j1"), assert(not(current(jobs.get("j1")))) })
      .expect(and {
        // only version 3's diagnostics were ever published, and they are current
        client.get("a").version == 3, upToDate("a"), high.get("a") == NO_HIGH,
        JOBS.forall(j => jobFree(j)),
        allInvariants,
      })

  run crashMidCompileTest =
    init
      .then(didOpen("a", 1))
      .then(restart)
      .then(startJob("a"))
      .then(complete("j1"))
      .then(toolEdit("a"))
      .then(startJob("a"))
      .then(all { crash, assert(lostBy == Set("j2")) })
      .then(all { restart, assert(docs.get("a").dirty) })
      .then(startJob("a"))
      .then(complete("j2"))
      .then(all { deliver("j1"), assert(not(current(jobs.get("j1")))) })
      .then(deliver("j2"))
      .then(all { noticeDisk("a"), assert(disk.get("a") == docs.get("a").overlay) })
      .expect(and {
        // the restarted worker's diagnostics of version 2, and the disk's own write of the same
        // text changed nothing
        worker == { up: true, inc: 2, lost: 1 },
        client.get("a").version == 2, client.get("a").inc == 2, upToDate("a"),
        not(docs.get("a").dirty), allInvariants,
      })

  run closeDuringCompileTest =
    init
      .then(didOpen("a", 1))
      .then(restart)
      .then(startJob("a"))
      .then(didClose("a"))
      .then(complete("j1"))
      .then(all { deliver("j1"), assert(not(current(jobs.get("j1")))) })
      .then(all { didOpen("a", 1), assert(client.get("a") == CLEARED) })
      .then(startJob("a"))
      .then(didClose("a"))
      .then(didOpen("a", 1))
      .then(startJob("a"))
      .then(complete("j1"))
      .then(complete("j2"))
      .then(all {
        deliver("j1"),
        // session 2's reply, at version 1 and the same text as session 3's version 1
        assert(jobs.get("j1").version == 1 and jobs.get("j1").session == 2 and docs.get("a").session == 3),
      })
      .then(deliver("j2"))
      .expect(and {
        client.get("a").session == 3, client.get("a").version == 1, upToDate("a"), allInvariants,
      })

  run dependencyEditMakesStaleTest =
    init
      .then(didOpen("a", 1))
      .then(restart)
      .then(startJob("a"))
      .then(diskWrite("b"))
      .then(all { noticeDisk("b"), assert(docs.get("a").version == 1) })
      .then(all { startJob("a"), assert(docs.get("a").epoch == 2) })
      .then(complete("j2"))
      .then(deliver("j2"))
      .then(complete("j1"))
      .then(all { deliver("j1"), assert(jobs.get("j1").version == 1 and not(current(jobs.get("j1")))) })
      .expect(and {
        // a's diagnostics read b's new text, at a's unchanged version 1
        client.get("a").inputs == { a: 1, b: 3, c: 0 }, client.get("a").version == 1, upToDate("a"),
        allInvariants,
      })

  run diskWinsOverOverlayTest =
    init
      .then(didOpen("a", 1))
      .then(didChange("a"))
      .then(diskWrite("a"))
      .then(restart)
      .then(all { startJob("a"), assert(tagIn(docs, "a") == 2) })
      .then(noticeDisk("a"))
      .then(all { startJob("a"), assert(tagIn(docs, "a") == NO_VERSION) })
      .then(complete("j2"))
      .then(complete("j1"))
      .then(all { deliver("j2"), assert(current(jobs.get("j2"))) })
      .then(all { deliver("j1"), assert(not(current(jobs.get("j1")))) })
      .then(all { toolEdit("a"), assert(client.get("a").version == NO_VERSION and client.get("a").inputs.a == 4) })
      .then(startJob("a"))
      .then(all { noticeDisk("a"), assert(not(docs.get("a").dirty)) })
      .then(complete("j1"))
      .then(deliver("j1"))
      .expect(and {
        client.get("a").version == 3, client.get("a").inputs.a == 5, upToDate("a"), allInvariants,
      })

  run timeoutLateReplyTest =
    init
      .then(didOpen("b", 1))
      .then(restart)
      .then(startJob("b"))
      .then(timeout("j1"))
      .then(all { startJob("b"), assert(docs.get("b").dirty) })
      .then(complete("j2"))
      .then(deliver("j2"))
      .then(complete("j1"))
      .then(all { deliver("j1"), assert(current(jobs.get("j1"))) })
      .expect(and { client.get("b").inc == 1, upToDate("b"), allInvariants })

  run createdFileMakesStaleTest =
    init
      .then(didOpen("a", 1))
      .then(restart)
      .then(startJob("a"))
      .then(complete("j1"))
      .then(deliver("j1"))
      .then(all { diskWrite("c"), assert(upToDate("a") and client.get("a").inputs.c == 0) })
      .then(noticeDisk("c"))
      .then(all { startJob("a"), assert(docs.get("a").dirty and not(upToDate("a"))) })
      .then(complete("j1"))
      .then(deliver("j1"))
      .expect(and { client.get("a").inputs == { a: 1, b: 2, c: 3 }, client.get("a").version == 1, upToDate("a"), allInvariants })

  // More tests without a trace.

  // Deleting `c` makes `a` pending again: its compile no longer finds `C`.
  run deletedFileMakesStaleTest =
    init
      .then(diskWrite("c"))
      .then(noticeDisk("c"))
      .then(didOpen("a", 1))
      .then(restart)
      .then(startJob("a"))
      .then(complete("j1"))
      .then(deliver("j1"))
      .then(all { diskDelete("c"), assert(client.get("a").inputs.c == 3) })
      .then(noticeDisk("c"))
      .expect(and { docs.get("a").dirty, not(upToDate("a")), inputsIn(docs, seenC, "a").c == 0, allInvariants })

  // A proxy re-sends `a` to a restarted daemon at the client's version 3, not renumbered: the
  // diagnostics carry the version whose text the client holds.
  run reopenAtTheClientVersionTest =
    init
      .then(didOpen("a", 3))
      .then(restart)
      .then(startJob("a"))
      .then(complete("j1"))
      .then(deliver("j1"))
      .expect(and { client.get("a").version == 3, upToDate("a"), allInvariants })

  // A disk write to a closed document is never compiled for it, and publishes nothing for it.
  run closedDocumentGetsNothingTest =
    init
      .then(diskWrite("b"))
      .then(noticeDisk("b"))
      .expect(and {
        client.get("b") == NO_PUB, not(docs.get("b").dirty), docs.get("b").epoch == 1, allInvariants,
      })

  // An editor change of the dependency `b`, open and unsaved, makes `a` stale, and `a`'s
  // compile reads `b`'s overlay.
  run dependencyOverlayIsReadTest =
    init
      .then(didOpen("a", 1))
      .then(didOpen("b", 1))
      .then(didChange("b"))
      .then(all { restart, assert(docs.get("a").epoch == 2 and docs.get("a").dirty) })
      .then(all { startJob("a"), assert(jobFor("a", 1).inputs == { a: 1, b: 3, c: 0 }) })
      .then(complete("j1"))
      .then(deliver("j1"))
      .expect(and { upToDate("a"), client.get("a").inputs.b == 3, allInvariants })

  // Closing the dependency `b` with an unsaved change makes `a` stale again: its compile goes
  // back to `b`'s disk text.
  run closingAnUnsavedDependencyTest =
    init
      .then(didOpen("a", 1))
      .then(didOpen("b", 1))
      .then(didChange("b"))
      .then(restart)
      .then(startJob("a"))
      .then(complete("j1"))
      .then(deliver("j1"))
      .then(all { didClose("b"), assert(upToDate("a")) })
      .expect(and {
        not(upToDate("a")), docs.get("a").dirty, inputsIn(docs, seenC, "a") == { a: 1, b: 2, c: 0 }, allInvariants,
      })

  // A disk write of the open document, seen before the client's newer change: the overlay is
  // newer and wins (plan §2.5).
  run newerOverlayWinsTest =
    init
      .then(didOpen("a", 1))
      .then(diskWrite("a"))
      .then(didChange("a"))
      .then(noticeDisk("a"))
      .expect(and { effIn(docs, "a") == 4, tagIn(docs, "a") == 2, allInvariants })

  // A document reopened after an agent's unseen disk write is compiled from the text the client
  // read, at version 1.
  run reopenReadsTheDiskTest =
    init
      .then(diskWrite("a"))
      .then(didOpen("a", 1))
      .expect(and { effIn(docs, "a") == 3, tagIn(docs, "a") == 1, docs.get("a").dirty, allInvariants })

  // ---- Broken-model controls (ver §6.1): each must violate its named invariant ----

  // Plan §7.3's control: publishes every reply as it arrives.
  action deliverEveryResult(job: str): bool = deliverWith(job, true)

  // Orders replies by the LSP version alone: a reply is taken when the document is open at the
  // reply's version. A dependency's change does not move the version, and a reopened document
  // starts again at 1.
  action deliverByVersion(job: str): bool = {
    val k = jobs.get(job)
    deliverWith(job, docOf(k.doc).open and k.version == docOf(k.doc).version)
  }

  // Forgets that the document was closed: a reply for its last epoch in its last session is
  // published after the close.
  action deliverIgnoringClose(job: str): bool = {
    val k = jobs.get(job)
    deliverWith(job, k.session == docOf(k.doc).session and k.epoch == docOf(k.doc).epoch)
  }

  // Closes without publishing the empty set: the client keeps showing the old diagnostics.
  action didCloseWithoutClear(doc: str): bool = {
    val x = docs.get(doc)
    val ds1 = docs.set(doc, { ...x, open: false, overlay: 0, dirty: false })
    val ch = changed(docs, ds1, Set())
    all {
      x.open,
      docs' = bump(ds1, ch),
      sinceEdit' = staleAfter(ch),
      disk' = disk, worker' = worker, jobs' = jobs, client' = client, high' = high, sent' = sent,
      nextText' = nextText, seenC' = seenC, t' = t + 1,
    }
  }

  // Reads plan §3.2's rule without "pending": takes a reply not older than the published
  // diagnostics, though a newer compile is pending.
  action deliverNotOlderThanPublished(job: str): bool = {
    val k = jobs.get(job)
    val p = pubOf(k.doc)
    val x = docOf(k.doc)
    deliverWith(job, and { x.open, k.session == x.session, p.kind != "diags" or p.session != x.session or k.epoch >= p.epoch })
  }

  // Tracks only the files a compile read: a file the compile looked up and did not find (`c`
  // while absent) is no input, so creating it changes nothing tracked and `a` stays "current".
  pure def trackedIn(ds0: str -> Doc, sc0: int, ds1: str -> Doc, sc1: int, d: str): Inputs = {
    val i = inputsIn(ds1, sc1, d)
    if (inputsIn(ds0, sc0, d).c == 0) { ...i, c: 0 } else i
  }
  action noticeDiskTrackingReads(doc: str): bool = {
    val after = seenAfter(doc)
    val ch = DOCS.filter(d => trackedIn(docs, seenC, after._1, after._2, d) != inputsIn(docs, seenC, d))
    all {
      unseen(doc),
      docs' = bump(after._1, ch),
      seenC' = after._2,
      sinceEdit' = staleAfter(ch),
      disk' = disk, worker' = worker, jobs' = jobs, client' = client, high' = high, sent' = sent,
      nextText' = nextText, t' = t + 1,
    }
  }

  // Tags a compile with the client's version even when the disk has overtaken the overlay.
  action startJobTaggingClientVersion(doc: str): bool = startJobWith(doc, docs.get(doc).version)

  run deliverEveryResultViolatesNeverOlderOverNewerTest =
    init
      .then(didOpen("a", 1))
      .then(restart)
      .then(startJob("a"))
      .then(didChange("a"))
      .then(startJob("a"))
      .then(complete("j2"))
      .then(complete("j1"))
      .then(deliverEveryResult("j2"))
      .then(deliverEveryResult("j1"))
      .expect(not(NeverOlderOverNewer))

  // The dependency case: two replies for `a` at version 1, the one that read `b`'s old text
  // last. The version does not tell them apart; the epoch does.
  run deliverByVersionViolatesNeverOlderOverNewerTest =
    init
      .then(didOpen("a", 1))
      .then(restart)
      .then(startJob("a"))
      .then(diskWrite("b"))
      .then(noticeDisk("b"))
      .then(startJob("a"))
      .then(complete("j2"))
      .then(deliverByVersion("j2"))
      .then(complete("j1"))
      .then(all { deliverByVersion("j1"), assert(jobs.get("j1").version == client.get("a").version) })
      .expect(not(NeverOlderOverNewer))

  // The reopen case: a reply of the first session's version 1 is published in the second
  // session's version 1, whose text an agent changed in between.
  run deliverByVersionViolatesPublishedMatchesVersionTest =
    init
      .then(didOpen("a", 1))
      .then(restart)
      .then(startJob("a"))
      .then(didClose("a"))
      .then(diskWrite("a"))
      .then(didOpen("a", 1))
      .then(complete("j1"))
      .then(deliverByVersion("j1"))
      .expect(not(PublishedMatchesVersion))

  run deliverIgnoringCloseViolatesClosedPublishesNothingTest =
    init
      .then(didOpen("a", 1))
      .then(restart)
      .then(startJob("a"))
      .then(all { didClose("a"), assert(docs.get("a").epoch == 1) })
      .then(complete("j1"))
      .then(deliverIgnoringClose("j1"))
      .expect(not(ClosedPublishesNothing))

  run didCloseWithoutClearViolatesClosedPublishesNothingTest =
    init
      .then(didOpen("a", 1))
      .then(restart)
      .then(startJob("a"))
      .then(complete("j1"))
      .then(deliver("j1"))
      .then(didCloseWithoutClear("a"))
      .expect(not(ClosedPublishesNothing))

  // The first compile's reply arrives after a second edit and before anything was published.
  run deliverNotOlderThanPublishedViolatesFreshAfterEditTest =
    init
      .then(didOpen("a", 1))
      .then(restart)
      .then(startJob("a"))
      .then(didChange("a"))
      .then(complete("j1"))
      .then(deliverNotOlderThanPublished("j1"))
      .expect(and { NeverOlderOverNewer, PublishedMatchesVersion, not(FreshAfterEdit) })

  // The review's counterexample: "cannot find symbol C" stays published as current after `c`
  // is created.
  run noticeDiskTrackingReadsViolatesFreshAfterEditTest =
    init
      .then(didOpen("a", 1))
      .then(restart)
      .then(startJob("a"))
      .then(complete("j1"))
      .then(deliver("j1"))
      .then(diskWrite("c"))
      .then(noticeDiskTrackingReads("c"))
      .expect(and { not(docs.get("a").dirty), NeverOlderOverNewer, PublishedMatchesVersion, not(FreshAfterEdit) })

  run startJobTaggingClientVersionViolatesPublishedMatchesVersionTest =
    init
      .then(didOpen("a", 1))
      .then(diskWrite("a"))
      .then(noticeDisk("a"))
      .then(restart)
      .then(startJobTaggingClientVersion("a"))
      .then(complete("j1"))
      .then(deliver("j1"))
      .expect(not(PublishedMatchesVersion))

  // ---- EventuallyUpToDate as a simulation (doc/formal-models.md, Liveness) ----
  //
  // Plan §3.2 and §3.4: a document whose edits stop eventually shows diagnostics of its current
  // inputs, through crashes and timeouts. Under weak fairness of the coordinator's and the
  // worker's moves, once the client, the agent and the faults stop, that fails only if the
  // system can stop with an open document not up to date (a deadlock, such as a lost compile
  // never started again) or move forever without updating it (a livelock). A run (`--step
  // progressStep`) takes `PREFIX` random steps of `step`, then only system moves, each picked at
  // random among the enabled ones, until nothing is enabled; `Progress` fails if that takes more
  // than `RECOVERY` moves or ends with an open document not up to date.
  pure val PREFIX = 30
  pure val RECOVERY = 30

  def systemEnabled: bool = or {
    FILES.exists(f => unseen(f)),
    DOCS.exists(d => startJobEnabled(d)),
    JOBS.exists(j => jobs.get(j).state == "done"),
    JOBS.exists(j => jobs.get(j).state == "running" and worker.up and jobs.get(j).inc == worker.inc),
    and { not(worker.up), worker.inc < MAX_INC, DOCS.exists(d => docs.get(d).open and docs.get(d).dirty) },
  }

  action systemMove = any {
    nondet doc = FILES.oneOf() noticeDisk(doc),
    nondet doc = DOCS.oneOf() startJob(doc),
    nondet job = JOBS.oneOf() deliver(job),
    nondet job = JOBS.oneOf() complete(job),
    restart,
  }

  // Nothing is enabled: the run stays put, so `Progress` is checked at its end.
  action rest = all {
    not(systemEnabled),
    disk' = disk, docs' = docs, worker' = worker, jobs' = jobs, client' = client, high' = high,
    sinceEdit' = sinceEdit, sent' = sent, nextText' = nextText, seenC' = seenC, t' = t + 1,
  }

  action progressStep = if (t < PREFIX) any { step, rest } else any { systemMove, rest }

  val Progress = t >= PREFIX + RECOVERY implies and {
    not(systemEnabled),
    DOCS.forall(d => docs.get(d).open implies upToDate(d)),
  }

  val allProgress = and { allInvariants, Progress }

  // The liveness control: a crash that forgets the compiles it lost (`crashWith(false)`).
  // `Progress` must fail: a document whose compile a crash lost is never compiled again.
  action crashForgetting = crashWith(false)
  action stepForgetting = any {
    clientStep,
    coordinatorStep,
    any {
      nondet job = JOBS.oneOf() complete(job),
      restart,
      crashForgetting,
      nondet job = JOBS.oneOf() timeout(job),
    },
  }
  action progressStepForgetting = if (t < PREFIX) any { stepForgetting, rest } else any { systemMove, rest }

  run crashForgettingNeverUpdatesTest =
    init
      .then(didOpen("a", 1))
      .then(restart)
      .then(startJob("a"))
      .then(crashForgetting)
      .expect(and { not(systemEnabled), not(upToDate("a")), allInvariants })
}
