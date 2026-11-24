// The views model (FORMAL-03; plan §4, §7.1, §2.1; ADR 0014, ADR 0017, ADR 0037): worktrees with
// files, HEADs and git indexes; checkouts, HEAD-only moves and staging; one watcher whose events
// arrive in order and can be duplicated or dropped (an overflow), and whose watches can be lost
// and re-established; reconcile steps, atomic publish; requests that stat, write a cookie, pin,
// read and finish; and `sync`. It is the contract the view registry of IDX-09 conforms to;
// doc/formal-models.md states its stimuli, the observation the driver produces and the invariants.
//
// The design it fixes (ADR 0037, doc/formal-models.md):
// - A view is one worktree root; a path belongs to its innermost enclosing worktree (plan §4.1,
//   §4.2), and a view's index holds only its own paths, never a nested worktree's.
// - A request pins a (view, generation) pair; its completeness is tied to that pair.
// - The stat (file, HEAD, git index) decides whether a request must wait; it applies git's
//   racy-mtime rule: an entry summarised within the mtime granularity of its file's last write
//   is never proven fresh by a stat.
// - complete=true needs a watched, certain view (no loss since a full scan started after its
//   watch was set up), the request's cookie back through the in-order event stream, no published
//   summary read after that cookie was written (else a second cookie at the pin, at most two),
//   nothing delivered and unpublished, and HEAD and the git index re-statted at the pin. The
//   promise is a snapshot: the answer's index is one state the view's files had between the
//   request's sync point and its pin.
module views {
  // ---- The world: two worktrees, one nested inside the other, three files ----

  // `n` is a linked agent worktree at `m/.claude/worktrees/n`: its file `n/A` lies under both
  // roots. Its HEAD and git index live in `m/.git/worktrees/n/`, outside its root, so no watcher
  // of `n` sees them change; `m`'s live in `m/.git`, which `m`'s watcher covers.
  pure val PATHS: Set[str] = Set("m/A", "m/B", "n/A")
  pure val VIEWS: Set[str] = Set("m", "n")
  pure val REQS: Set[str] = Set("r1", "r2")
  pure val ENCLOSING: str -> Set[str] = Map("m/A" -> Set("m"), "m/B" -> Set("m"), "n/A" -> Set("m", "n"))
  pure val DEPTH: str -> int = Map("m" -> 1, "n" -> 2)
  // The event a HEAD or index change raises in the view's watcher, or "" when none does.
  pure val GIT_EVENT: str -> str = Map("m" -> "m/.git", "n" -> "")
  // A fixed order among the events one stimulus raises (their sequence numbers).
  pure val RANK: str -> int = Map("m/A" -> 0, "m/B" -> 1, "n/A" -> 2, "m/.git" -> 3)

  // Bounds that keep simulation and Apalache finite.
  pure val MAX_VER = 6
  pure val MAX_HEAD = 3
  pure val MAX_GEN = 8

  pure def innermost(p: str): str =
    ENCLOSING.get(p).fold("", (best, w) =>
      if (best == "") w else if (DEPTH.get(w) > DEPTH.get(best)) w else best)

  // The outermost worktree enclosing `p` (only the broken binding control uses it).
  pure def outermost(p: str): str =
    ENCLOSING.get(p).fold("", (best, w) =>
      if (best == "") w else if (DEPTH.get(w) < DEPTH.get(best)) w else best)

  pure def viewPaths(w: str): Set[str] = PATHS.filter(p => innermost(p) == w)

  // ---- Types ----

  // A file. On disk: `ver` its content (every change gives a higher one), `present`, `stat` what
  // a stat sees (mtime and size: a racy write keeps it), `recent` written within the current
  // mtime tick. In an index: the same as summarised, `recent` meaning the summary is racy (taken
  // within the tick of the file's last write), so no stat may prove the entry fresh.
  type Entry = { ver: int, present: bool, stat: int, recent: bool }
  type Index = str -> Entry
  // The disk: files, and each worktree's HEAD and git index.
  type Tree = { files: Index, heads: str -> int, gitIndexes: str -> int }
  // One watcher event: a file's or git metadata's (`path`), or a request's cookie (`req`, with
  // path ""). Events are delivered in `seq` order: one inotify instance (or FSEvents stream)
  // delivers in order, which is what makes the cookie a barrier.
  type Event = { seq: int, view: str, path: str, req: str }
  type Watcher = { events: Set[Event], nextSeq: int }
  // `readAt`: when each summary was read, on a daemon-local clock ordered with cookie writes (the
  // model uses the watcher's next sequence number): a summary read after a request's cookie was
  // written has `readAt` above the cookie's `seq`.
  type Generation = { gen: int, head: int, gitIndex: int, index: Index, readAt: str -> int }
  type Build = {
    active: bool, head: int, gitIndex: int, index: Index, readAt: str -> int,
    pending: Set[str],   // paths whose summaries are not stored yet
    certifies: bool,     // a full scan started with the watch up and no loss since
    syncEpoch: int,      // the sync whose rescan this is, or -1
  }
  type View = {
    dirty: Set[str],     // delivered file events no build has taken yet
    rescan: bool,        // a full reconcile is due
    watched: bool,       // the watch is established
    certain: bool,       // no loss since a full scan that started with the watch up
    build: Build,
    published: Generation,
    prevGen: int,        // the generation the published one replaced
  }
  type Read = { view: str, gen: int, path: str, ver: int }
  // One request. `sync*` is the disk at its sync point (its stat and cookie); `pin*` the disk
  // when it pinned: both are the driver's records of the scripted tree, not the registry's.
  type Request = {
    phase: str,          // idle, waiting, pinned, done
    path: str, view: str,
    needGen: int,        // the first generation it may pin: one a build started after its stat
    cookie: str,         // its latest cookie: none (no watch), queued, delivered, lost
    cookieSeq: int,      // that cookie's place on the clock
    cookies: int,        // cookies written: at most two (the second only at the pin)
    history: Set[Index], // ghost: every state of the view's files from the sync point to the pin
    syncDisk: Index, syncHead: int, syncGitIndex: int,
    gen: int, snap: Index, snapHead: int, snapGitIndex: int,
    pinDisk: Index, pinHead: int, pinGitIndex: int,
    complete: bool, completeView: str, completeGen: int,
    reads: Set[Read],
  }
  type Sync = { phase: str, epoch: int, scanned: bool, need: Index, needHead: int }
  type Summary = { path: str, ver: int }

  pure val NO_INDEX: Index = Map()
  pure val NO_PATHS: Set[str] = Set()
  pure val NO_EVENT: Event = { seq: -1, view: "", path: "", req: "" }
  pure val IDLE_BUILD: Build = {
    active: false, head: 0, gitIndex: 0, index: NO_INDEX, readAt: Map(), pending: Set(),
    certifies: false,
    syncEpoch: -1,
  }
  pure val IDLE_REQUEST: Request = {
    phase: "idle", path: "", view: "", needGen: 0, cookie: "none", cookieSeq: -1, cookies: 0,
    history: Set(),
    syncDisk: NO_INDEX, syncHead: 0, syncGitIndex: 0,
    gen: 0, snap: NO_INDEX, snapHead: 0, snapGitIndex: 0,
    pinDisk: NO_INDEX, pinHead: 0, pinGitIndex: 0,
    complete: false, completeView: "", completeGen: 0, reads: Set(),
  }
  pure val IDLE_SYNC: Sync = { phase: "idle", epoch: 0, scanned: false, need: NO_INDEX, needHead: 0 }

  // ---- State ----

  var tree: Tree
  var watcher: Watcher
  var views: str -> View
  var store: Set[Summary]
  var requests: str -> Request
  var syncs: str -> Sync

  // ---- Helpers ----

  pure def restrict(ix: Index, w: str): Index = viewPaths(w).mapBy(p => ix.get(p))
  pure def answered(r: Request): bool = r.phase == "pinned" or r.phase == "done"
  // What a stat can tell: the entry changed, or it is racy and so proves nothing.
  pure def statChanged(indexed: Entry, disk: Entry): bool =
    indexed.recent or indexed.stat != disk.stat or indexed.present != disk.present
  pure def covers(g: Generation, need: Index, needHead: int): bool = and {
    need.keys().forall(p => g.index.get(p).ver >= need.get(p).ver),
    g.head >= needHead,
  }
  // The events one stimulus raises in `view`'s watcher, after every queued one.
  pure def raise(w: Watcher, view: str, paths: Set[str]): Watcher = {
    events: w.events.union(paths.map(p => { seq: w.nextSeq + RANK.get(p), view: view, path: p, req: "" })),
    nextSeq: w.nextSeq + 4,
  }
  // The next event in order, or NO_EVENT.
  pure def first(w: Watcher): Event =
    w.events.fold(NO_EVENT, (best, e) => if (best.seq < 0 or e.seq < best.seq) e else best)

  def file(p: str): Entry = tree.files.get(p)
  def ver(p: str): int = tree.files.get(p).ver
  def headOf(w: str): int = tree.heads.get(w)
  def gitIndex(w: str): int = tree.gitIndexes.get(w)
  def gen(w: str): int = views.get(w).published.gen
  def phase(r: str): str = requests.get(r).phase
  def queued(w: str): Set[str] = watcher.events.filter(e => e.view == w and e.req == "").map(e => e.path)
  def cookieQueued(r: str): bool = watcher.events.exists(e => e.req == r)
  // The events a disk change raises: only while the view's watch is up.
  def raised(view: str, paths: Set[str]): Watcher =
    if (views.get(view).watched) raise(watcher, view, paths) else watcher
  // The ghost history: each waiting request records its view's files after a disk change.
  def recorded(files: Index): str -> Request = requests.keys().mapBy(id => {
    val r = requests.get(id)
    if (r.phase == "waiting") { ...r, history: r.history.union(Set(restrict(files, r.view))) } else r
  })

  def gitRaised(view: str): Watcher =
    if (GIT_EVENT.get(view) == "") watcher else raised(view, Set(GIT_EVENT.get(view)))

  // ---- Initial state: watch, then scan, then publish generation 1 of each view ----

  action init = {
    val files = PATHS.mapBy(_ => { ver: 1, present: true, stat: 1, recent: false })
    all {
      tree' = { files: files, heads: VIEWS.mapBy(_ => 1), gitIndexes: VIEWS.mapBy(_ => 1) },
      watcher' = { events: Set(), nextSeq: 0 },
      views' = VIEWS.mapBy(w => {
        dirty: Set(), rescan: false, watched: true, certain: true, build: IDLE_BUILD,
        published: { gen: 1, head: 1, gitIndex: 1, index: restrict(files, w),
                     readAt: viewPaths(w).mapBy(_ => -1) },
        prevGen: 0,
      }),
      store' = PATHS.map(p => { path: p, ver: 1 }),
      requests' = REQS.mapBy(_ => IDLE_REQUEST),
      syncs' = VIEWS.mapBy(_ => IDLE_SYNC),
    }
  }

  // ---- Disk stimuli ----

  // A write; `racy`: within the mtime tick of the file's previous write and of the same size, so
  // a stat cannot see it (the watcher still does).
  action write(path: str, racy: bool): bool = {
    val f = file(path)
    val files = tree.files.set(path,
      { ver: f.ver + 1, present: true, stat: if (racy) f.stat else f.ver + 1, recent: true })
    all {
      f.ver < MAX_VER,
      not(racy) or f.recent,
      tree' = { ...tree, files: files },
      watcher' = raised(innermost(path), Set(path)),
      requests' = recorded(files),
      views' = views, store' = store, syncs' = syncs,
    }
  }

  action delete(path: str): bool = {
    val f = file(path)
    val files = tree.files.set(path, { ver: f.ver + 1, present: false, stat: f.ver + 1, recent: true })
    all {
      f.present,
      f.ver < MAX_VER,
      tree' = { ...tree, files: files },
      watcher' = raised(innermost(path), Set(path)),
      requests' = recorded(files),
      views' = views, store' = store, syncs' = syncs,
    }
  }

  // The clock passes the mtime tick of every recent write.
  action tick: bool = all {
    PATHS.exists(p => file(p).recent),
    tree' = { ...tree, files: tree.files.keys().mapBy(p => { ...file(p), recent: false }) },
    watcher' = watcher, views' = views, store' = store, requests' = requests, syncs' = syncs,
  }

  // `git checkout`, `stash` or `reset --hard`: HEAD, the index and every file of the worktree.
  action checkout(view: str): bool = {
    val files = tree.files.keys().mapBy(p =>
      if (innermost(p) == view) { ver: ver(p) + 1, present: true, stat: ver(p) + 1, recent: true }
      else file(p))
    all {
      headOf(view) < MAX_HEAD,
      gitIndex(view) < MAX_HEAD + 2,
      viewPaths(view).forall(p => ver(p) < MAX_VER),
      tree' = {
        files: files,
        heads: tree.heads.set(view, headOf(view) + 1),
        gitIndexes: tree.gitIndexes.set(view, gitIndex(view) + 1),
      },
      watcher' = raised(view, viewPaths(view).union(
        if (GIT_EVENT.get(view) == "") Set() else Set(GIT_EVENT.get(view)))),
      requests' = recorded(files),
      views' = views, store' = store, syncs' = syncs,
    }
  }

  // `git commit` or `reset --soft`: HEAD moves, no file changes.
  action moveHead(view: str): bool = all {
    headOf(view) < MAX_HEAD,
    tree' = { ...tree, heads: tree.heads.set(view, headOf(view) + 1) },
    watcher' = gitRaised(view),
    views' = views, store' = store, requests' = requests, syncs' = syncs,
  }

  // `git add`: the index changes, no file does.
  action stage(view: str): bool = all {
    gitIndex(view) < MAX_HEAD + 2,
    tree' = { ...tree, gitIndexes: tree.gitIndexes.set(view, gitIndex(view) + 1) },
    watcher' = gitRaised(view),
    views' = views, store' = store, requests' = requests, syncs' = syncs,
  }

  // ---- The watcher ----

  // Delivers the next event in order; `keep` duplicates a file event (it arrives again later).
  // A file event marks the path dirty, a git event a rescan, a cookie its request's barrier.
  action deliver(keep: bool): bool = {
    val e = first(watcher)
    all {
      e.seq >= 0,
      not(keep) or e.req == "",
      watcher' = {
        events: watcher.events.exclude(Set(e)).union(
          if (keep) Set({ ...e, seq: watcher.nextSeq }) else Set()),
        nextSeq: if (keep) watcher.nextSeq + 1 else watcher.nextSeq,
      },
      views' =
        if (e.req != "") views
        else if (PATHS.contains(e.path)) views.setBy(e.view, v => { ...v, dirty: v.dirty.union(Set(e.path)) })
        else views.setBy(e.view, v => { ...v, rescan: true }),
      requests' =
        if (e.req != "") requests.setBy(e.req, r => { ...r, cookie: "delivered" }) else requests,
      tree' = tree, store' = store, syncs' = syncs,
    }
  }

  // An overflow: the kernel drops an event and reports only that something was lost. One
  // instance serves every view, so every view rescans and is uncertain.
  action dropEvent(seq: int): bool = {
    val lost = watcher.events.filter(e => e.seq == seq)
    all {
      lost != Set(),
      watcher' = { ...watcher, events: watcher.events.exclude(lost) },
      views' = views.keys().mapBy(w => { ...views.get(w),
        rescan: true, certain: false, build: { ...views.get(w).build, certifies: false } }),
      requests' = requests.keys().mapBy(id =>
        if (lost.exists(e => e.req == id)) { ...requests.get(id), cookie: "lost" } else requests.get(id)),
      tree' = tree, store' = store, syncs' = syncs,
    }
  }

  // A watch is lost: an add_watch error (ENOSPC, EMFILE), IN_IGNORED, IN_DELETE_SELF,
  // IN_UNMOUNT, FSEvents' RootChanged or EventIdsWrapped. Changes now raise no event.
  action watchLost(view: str): bool = all {
    views.get(view).watched,
    views' = views.setBy(view, v => { ...v, watched: false, certain: false, rescan: true,
      build: { ...v.build, certifies: false } }),
    tree' = tree, watcher' = watcher, store' = store, requests' = requests, syncs' = syncs,
  }

  // The watch is set up again: watch, then scan.
  action watchRestored(view: str): bool = all {
    not(views.get(view).watched),
    views' = views.setBy(view, v => { ...v, watched: true, rescan: true }),
    tree' = tree, watcher' = watcher, store' = store, requests' = requests, syncs' = syncs,
  }

  // ---- Reconcile ----

  // Takes the delivered events and, on a rescan (`git diff --name-only` plus `git status`, plan
  // §4.3), every path a stat cannot prove unchanged; names their disk entries. A rescan records
  // HEAD and the index, certifies the view when the watch is up, and serves a waiting sync.
  //
  // A rescan may `leave` delivered paths unread (IDX-09e): they stay in `dirty`, so no answer is
  // complete until a build takes them, and the daemon's settle takes them a slice at a time
  // instead of the rescan reading every file still deferred in one build. Only delivered paths
  // (a path a stat or a listing found changed and no event named is read here), never one a
  // waiting request needs, and never without a rescan (a part is `reconcilePart`). The scan
  // still certifies: it started with the watch up, and a left path is known changed.
  action reconcileStart(view: str, leave: Set[str]): bool = {
    val v = views.get(view)
    val g = v.published
    val changed =
      if (v.rescan) viewPaths(view).filter(p => statChanged(g.index.get(p), file(p))) else Set()
    val todo = v.dirty.union(changed).exclude(leave)
    all {
      not(v.build.active),
      v.dirty != Set() or v.rescan,
      v.rescan or leave == Set(),
      leave.subseteq(v.dirty),
      leave.intersect(needed(view)) == Set(),
      views' = views.set(view, { ...v, dirty: leave, rescan: false, build: {
        active: true,
        head: if (v.rescan) headOf(view) else g.head,
        gitIndex: if (v.rescan) gitIndex(view) else g.gitIndex,
        index: g.index.keys().mapBy(p => if (todo.contains(p)) file(p) else g.index.get(p)),
        readAt: g.readAt,
        pending: todo,
        certifies: v.rescan and v.watched,
        syncEpoch: if (v.rescan and syncs.get(view).phase == "waiting") syncs.get(view).epoch else -1,
      }}),
      tree' = tree, watcher' = watcher, store' = store, requests' = requests, syncs' = syncs,
    }
  }

  // The paths the waiting requests of `view` need: each one's file, while the generation it waits
  // for is not published (its stat saw the file change, so the build that publishes that
  // generation must read it).
  def needed(view: str): Set[str] =
    REQS.filter(id => {
      val r = requests.get(id)
      r.phase == "waiting" and r.view == view and r.needGen > gen(view)
    }).map(id => requests.get(id).path)

  // Takes part of the delivered events as a build of its own (IDX-09d): the daemon settles a view
  // it opened in two steps a slice at a time, so its loop is never held for the whole view. The
  // rest stay delivered and unpublished, so no answer is complete until a build takes them. Never
  // in place of a rescan, which reads every changed path but the delivered ones it leaves (it
  // certifies, records HEAD and the index, and serves `sync`); and
  // every path a waiting request needs is taken, so the generation it waits for holds what its
  // stat saw.
  action reconcilePart(view: str, take: Set[str]): bool = {
    val v = views.get(view)
    val g = v.published
    all {
      not(v.build.active),
      not(v.rescan),
      take != Set(),
      take.subseteq(v.dirty),
      needed(view).subseteq(take),
      views' = views.set(view, { ...v, dirty: v.dirty.exclude(take), build: {
        active: true, head: g.head, gitIndex: g.gitIndex,
        index: g.index.keys().mapBy(p => if (take.contains(p)) file(p) else g.index.get(p)),
        readAt: g.readAt,
        pending: take,
        certifies: false,
        syncEpoch: -1,
      }}),
      tree' = tree, watcher' = watcher, store' = store, requests' = requests, syncs' = syncs,
    }
  }

  // Re-reads one pending path, summarises it (racy when the file was written this tick) and
  // stores the summary.
  action summarise(view: str, path: str): bool = {
    val b = views.get(view).build
    all {
      b.active,
      b.pending.contains(path),
      store' = store.union(Set({ path: path, ver: ver(path) })),
      views' = views.setBy(view, v => { ...v, build: { ...b,
        index: b.index.set(path, file(path)),
        readAt: b.readAt.set(path, watcher.nextSeq),
        pending: b.pending.exclude(Set(path)),
      }}),
      tree' = tree, watcher' = watcher, requests' = requests, syncs' = syncs,
    }
  }

  // Publishes the build atomically, once every summary it names is stored.
  action publish(view: str): bool = {
    val v = views.get(view)
    val b = v.build
    val s = syncs.get(view)
    all {
      b.active,
      b.pending == Set(),
      b.index.keys().forall(p => store.contains({ path: p, ver: b.index.get(p).ver })),
      v.published.gen < MAX_GEN,
      views' = views.set(view, { ...v,
        prevGen: v.published.gen,
        published: { gen: v.published.gen + 1, head: b.head, gitIndex: b.gitIndex, index: b.index,
                     readAt: b.readAt },
        certain: v.certain or b.certifies,
        build: IDLE_BUILD,
      }),
      syncs' = if (b.syncEpoch == s.epoch and s.phase == "waiting")
        syncs.set(view, { ...s, scanned: true }) else syncs,
      tree' = tree, watcher' = watcher, store' = store, requests' = requests,
    }
  }

  // ---- Requests ----

  // Binds the path to its innermost worktree, stats the file, HEAD and the index (the racy rule
  // applied), and writes a cookie when the watch is up. A change the stat sees marks the path
  // dirty (or a rescan, for HEAD or the index), and the request waits for a build started after.
  action requestStart(req: str, path: str): bool = {
    val view = innermost(path)
    val v = views.get(view)
    val g = v.published
    val fileFresh = not(statChanged(g.index.get(path), file(path)))
    val gitFresh = g.head == headOf(view) and g.gitIndex == gitIndex(view)
    val queue = v.watched
    all {
      phase(req) == "idle" or phase(req) == "done",
      not(cookieQueued(req)),
      requests' = requests.set(req, { ...IDLE_REQUEST, phase: "waiting", path: path, view: view,
        needGen: if (fileFresh and gitFresh) g.gen else g.gen + (if (v.build.active) 2 else 1),
        cookie: if (queue) "queued" else "none", cookieSeq: watcher.nextSeq,
        cookies: if (queue) 1 else 0, history: Set(restrict(tree.files, view)),
        syncDisk: restrict(tree.files, view), syncHead: headOf(view), syncGitIndex: gitIndex(view) }),
      watcher' = if (queue) {
          events: watcher.events.union(Set({ seq: watcher.nextSeq, view: view, path: "", req: req })),
          nextSeq: watcher.nextSeq + 1,
        } else watcher,
      views' = if (fileFresh and gitFresh) views else views.set(view, { ...v,
        dirty: if (fileFresh) v.dirty else v.dirty.union(Set(path)),
        rescan: v.rescan or not(gitFresh) }),
      tree' = tree, store' = store, syncs' = syncs,
    }
  }

  // Whether `r` may claim completeness at its pin in view `v`, HEAD and the index re-statted.
  pure def completeAt(r: Request, v: View, restat: bool): bool = and {
    r.cookie == "delivered", lateReads(r, v) == Set(),
    v.watched, v.certain, v.dirty == Set(), not(v.rescan),
    not(v.build.active), restat,
  }

  // The published summaries read after `r`'s latest cookie was written: while there are any, the
  // generation may hold a version newer than that cookie's moment next to an older one.
  pure def lateReads(r: Request, v: View): Set[str] =
    v.published.readAt.keys().filter(p => v.published.readAt.get(p) > r.cookieSeq)

  // A second cookie, at the pin, when the published generation holds a summary read after the
  // first: its round trip makes the generation a snapshot of the disk at its write, or the
  // answer is incomplete. At most two cookies per request.
  action requestRecookie(req: str): bool = {
    val r = requests.get(req)
    val v = views.get(r.view)
    all {
      r.phase == "waiting",
      r.cookie == "delivered",
      r.cookies < 2,
      v.watched,
      lateReads(r, v) != Set(),
      requests' = requests.set(req, { ...r, cookie: "queued", cookieSeq: watcher.nextSeq,
        cookies: r.cookies + 1 }),
      watcher' = {
        events: watcher.events.union(Set({ seq: watcher.nextSeq, view: r.view, path: "", req: req })),
        nextSeq: watcher.nextSeq + 1,
      },
      tree' = tree, views' = views, store' = store, syncs' = syncs,
    }
  }

  def pinnable(req: str): bool = {
    val r = requests.get(req)
    r.phase == "waiting" and gen(r.view) >= r.needGen
  }

  // Pins the published generation and computes completeness from the request's own view at it,
  // re-statting HEAD and the index; a change there starts a rescan. It waits for a queued cookie
  // (IDX-09 bounds that wait with a deadline, which is not modelled: an expired one answers
  // incomplete).
  action requestPin(req: str): bool = {
    val r = requests.get(req)
    val v = views.get(r.view)
    val g = v.published
    val restat = g.head == headOf(r.view) and g.gitIndex == gitIndex(r.view)
    all {
      pinnable(req),
      r.cookie != "queued",
      requests' = requests.set(req, { ...r, phase: "pinned",
        gen: g.gen, snap: g.index, snapHead: g.head, snapGitIndex: g.gitIndex,
        pinDisk: restrict(tree.files, r.view), pinHead: headOf(r.view), pinGitIndex: gitIndex(r.view),
        complete: completeAt(r, v, restat), completeView: r.view, completeGen: g.gen }),
      views' = if (restat) views else views.setBy(r.view, x => { ...x, rescan: true }),
      tree' = tree, watcher' = watcher, store' = store, syncs' = syncs,
    }
  }

  action read(req: str, path: str): bool = {
    val r = requests.get(req)
    all {
      r.phase == "pinned",
      r.snap.keys().contains(path),
      requests' = requests.set(req, { ...r,
        reads: r.reads.union(Set({ view: r.view, gen: r.gen, path: path, ver: r.snap.get(path).ver })),
      }),
      tree' = tree, watcher' = watcher, views' = views, store' = store, syncs' = syncs,
    }
  }

  action finish(req: str): bool = all {
    phase(req) == "pinned",
    requests' = requests.setBy(req, r => { ...r, phase: "done" }),
    tree' = tree, watcher' = watcher, views' = views, store' = store, syncs' = syncs,
  }

  // ---- sync: a rescan that started after it, published (plan §4.3) ----

  action syncStart(view: str): bool = {
    val s = syncs.get(view)
    all {
      s.phase != "waiting",
      syncs' = syncs.set(view, { phase: "waiting", epoch: s.epoch + 1, scanned: false,
        need: restrict(tree.files, view), needHead: headOf(view) }),
      views' = views.setBy(view, v => { ...v, rescan: true }),
      tree' = tree, watcher' = watcher, store' = store, requests' = requests,
    }
  }

  // Its rescan has published, and no delivered path waits for a build (IDX-09e: a rescan may
  // leave delivered paths for the settle's parts, which the barrier then waits for; a path leaves
  // `dirty` only into a build, so with none in progress every one it held is published).
  def syncable(view: str): bool = and {
    syncs.get(view).phase == "waiting",
    syncs.get(view).scanned,
    views.get(view).dirty == Set(),
    not(views.get(view).build.active),
  }

  action syncDone(view: str): bool = all {
    syncable(view),
    syncs' = syncs.setBy(view, s => { ...s, phase: "done" }),
    tree' = tree, watcher' = watcher, views' = views, store' = store, requests' = requests,
  }

  // ---- Every stimulus, for simulation, Apalache and the generated conformance traces ----

  // Each parameter is a nondeterministic pick named as the parameter (`path`, `view`, `req`,
  // `keep`, `racy`, `seq`), so `quint run --mbt` records it for the driver. The stimuli are grouped
  // so each kind is equally likely: flat, the six disk and four watcher stimuli crowd out the
  // request path, and a complete answer through a second cookie was not reached in 40000 samples.
  // `--mbt` records the innermost action taken, so the groups never appear in a trace.
  action diskStep = any {
    nondet path = PATHS.oneOf() nondet racy = Set(true, false).oneOf() write(path, racy),
    nondet path = PATHS.oneOf() delete(path),
    tick,
    nondet view = VIEWS.oneOf() checkout(view),
    nondet view = VIEWS.oneOf() moveHead(view),
    nondet view = VIEWS.oneOf() stage(view),
  }

  // Losses are rarer than deliveries: one loss in a run already leaves the views uncertain until
  // a full scan.
  action lossStep = any {
    all { watcher.events != Set(), nondet seq = watcher.events.map(e => e.seq).oneOf() dropEvent(seq) },
    nondet view = VIEWS.oneOf() watchLost(view),
    nondet view = VIEWS.oneOf() watchRestored(view),
  }

  action watcherStep = any {
    nondet keep = Set(true, false).oneOf() deliver(keep),
    lossStep,
  }

  // sync is rarer than a build's steps: a started sync makes a rescan due, which refuses every
  // part, so as one of six daemon stimuli it kept the simulation from parts (IDX-09e).
  action syncStep = any {
    nondet view = VIEWS.oneOf() syncStart(view),
    nondet view = VIEWS.oneOf() syncDone(view),
  }

  action daemonStep = any {
    // A rescan reads everything, or leaves every delivered path no request needs (the daemon
    // leaves the settle's deferred files, IDX-09e), every delivered path, or one of them. The last
    // two may hold a needed path, which the guard refuses: so the simulation reaches a rescan
    // that would leave one while a request waits, the case the guard is for (IDX-09d's lesson).
    // Without a rescan nothing is left.
    nondet view = VIEWS.oneOf()
    nondet leave = (if (views.get(view).rescan) {
        val dirty = views.get(view).dirty
        dirty.map(p => Set(p)).union(Set(NO_PATHS, dirty, dirty.exclude(needed(view))))
      } else Set(NO_PATHS)).oneOf()
    reconcileStart(view, leave),
    // A part of the view's delivered events: one delivered path alone, or with every path the
    // waiting requests need. Picking among these rather than every subset makes the simulation
    // reach a part while a request waits, the case the `needed` guard is for (IDX-09d's review:
    // with every subset, 20000 samples of 30 steps never reached it, and the broken control
    // `reconcilePartIgnoringRequests` went unseen).
    nondet view = VIEWS.oneOf()
    nondet take = views.get(view).dirty.map(p => Set(Set(p), needed(view).union(Set(p))))
      .flatten().union(Set(Set())).oneOf()
    reconcilePart(view, take),
    nondet view = VIEWS.oneOf() nondet path = PATHS.oneOf() summarise(view, path),
    nondet view = VIEWS.oneOf() publish(view),
    syncStep,
  }

  action requestStep = any {
    nondet req = REQS.oneOf() nondet path = PATHS.oneOf() requestStart(req, path),
    nondet req = REQS.oneOf() requestRecookie(req),
    nondet req = REQS.oneOf() requestPin(req),
    nondet req = REQS.oneOf() nondet path = PATHS.oneOf() read(req, path),
    nondet req = REQS.oneOf() finish(req),
  }

  action step = any { diskStep, watcherStep, daemonStep, requestStep }

  // ---- Invariants (plan §7.1, ver §6.1) ----

  // Every value a request read belongs to the generation it pinned.
  val NoMixedGeneration = REQS.forall(id => {
    val r = requests.get(id)
    answered(r) implies r.reads.forall(x => and {
      x.gen == r.gen,
      r.snap.keys().contains(x.path),
      if (r.snap.keys().contains(x.path)) x.ver == r.snap.get(x.path).ver else false,
    })
  })

  // A view's generations increase strictly, and none is pinned before it is published.
  val GenerationsIncrease = and {
    VIEWS.forall(w => views.get(w).published.gen > views.get(w).prevGen),
    REQS.forall(id => {
      val r = requests.get(id)
      answered(r) implies (VIEWS.contains(r.view) and r.gen <= gen(r.view))
    }),
  }

  // A published generation's summaries are all stored.
  val AtomicPublish = VIEWS.forall(w => {
    val g = views.get(w).published
    g.index.keys().forall(p => store.contains({ path: p, ver: g.index.get(p).ver }))
  })

  // What the request's stat observed (its file, its worktree's HEAD and index) is in the
  // generation it answers from, or something newer.
  val FreshAfterStat = REQS.forall(id => {
    val r = requests.get(id)
    answered(r) implies and {
      r.snap.keys().contains(r.path),
      if (r.snap.keys().contains(r.path)) r.snap.get(r.path).ver >= r.syncDisk.get(r.path).ver else false,
      r.snapHead >= r.syncHead,
      r.snapGitIndex >= r.syncGitIndex,
    }
  })

  // complete=true only for the request's own view at its pinned generation, when the pinned index
  // is one state the view's files actually had between the request's sync point and its pin (a
  // snapshot, never a mix of versions no disk state held: a reference moved from one file to
  // another is then in the answer), and HEAD and the index are the disk's at the pin (ADR 0014:
  // complete=true is never wrong; ADR 0037).
  val CompleteIsHonest = REQS.forall(id => {
    val r = requests.get(id)
    (answered(r) and r.complete) implies and {
      r.completeView == r.view,
      r.completeGen == r.gen,
      r.snap.keys() == viewPaths(r.view),
      r.history.exists(h => viewPaths(r.view).forall(p => r.snap.get(p).ver == h.get(p).ver)),
      r.snapHead == r.pinHead,
      r.snapGitIndex == r.pinGitIndex,
    }
  })

  // A request binds to the innermost worktree of its path and is answered only from that view:
  // its pin, its reads and its completeness; and no view indexes a nested worktree's files.
  val NoCrossView = and {
    VIEWS.forall(w => views.get(w).published.index.keys() == viewPaths(w)),
    REQS.forall(id => {
      val r = requests.get(id)
      r.phase != "idle" implies and {
        r.view == innermost(r.path),
        answered(r) implies and {
          r.completeView == r.view,
          r.snap.keys() == viewPaths(r.view),
          r.reads.forall(x => x.view == r.view),
        },
      }
    }),
  }

  // A finished sync's view has published everything the disk held when the sync started.
  val SyncIsBarrier = VIEWS.forall(w => {
    val s = syncs.get(w)
    s.phase == "done" implies covers(views.get(w).published, s.need, s.needHead)
  })

  val allInvariants = and {
    NoMixedGeneration,
    GenerationsIncrease,
    AtomicPublish,
    FreshAfterStat,
    CompleteIsHonest,
    NoCrossView,
    SyncIsBarrier,
  }

  // ---- Named scenarios (ver §6.1) ----
  //
  // Each scenario is a scripted step: guarded branches of which exactly one is enabled in each
  // state of the run, so `quint run --mbt --step <scenario>` writes its regression trace with the
  // same action names and picks as `step` (formal/traces/regression/manifest.toml); its test
  // below takes the same steps as named actions.

  // A checkout lands while a reconcile builds: the build publishes the re-read file with the old
  // HEAD; a request's stat sees the new HEAD and waits for the rescan's generation, complete.
  action checkoutDuringReconcile = any {
    all { ver("m/A") == 1,
          nondet path = Set("m/A").oneOf() nondet racy = Set(false).oneOf() write(path, racy) },
    all { ver("m/A") == 2, queued("m") == Set("m/A"),
          nondet keep = Set(false).oneOf() deliver(keep) },
    all { gen("m") == 1, headOf("m") == 1, views.get("m").dirty == Set("m/A"),
          nondet view = Set("m").oneOf() nondet leave = Set(NO_PATHS).oneOf() reconcileStart(view, leave) },
    all { views.get("m").build.active, headOf("m") == 1,
          nondet view = Set("m").oneOf() checkout(view) },
    all { gen("m") == 1, headOf("m") == 2, views.get("m").build.pending == Set("m/A"),
          nondet view = Set("m").oneOf() nondet path = Set("m/A").oneOf() summarise(view, path) },
    all { gen("m") == 1, views.get("m").build.active, views.get("m").build.pending == Set(),
          nondet view = Set("m").oneOf() publish(view) },
    all { gen("m") == 2, phase("r1") == "idle",
          nondet req = Set("r1").oneOf() nondet path = Set("m/B").oneOf() requestStart(req, path) },
    all { phase("r1") == "waiting", first(watcher).seq >= 0,
          nondet keep = Set(false).oneOf() deliver(keep) },
    all { phase("r1") == "waiting", watcher.events == Set(), gen("m") == 2,
          not(views.get("m").build.active),
          nondet view = Set("m").oneOf() nondet leave = Set(NO_PATHS).oneOf() reconcileStart(view, leave) },
    all { views.get("m").build.pending == Set("m/A", "m/B"),
          nondet view = Set("m").oneOf() nondet path = Set("m/A").oneOf() summarise(view, path) },
    all { views.get("m").build.pending == Set("m/B"),
          nondet view = Set("m").oneOf() nondet path = Set("m/B").oneOf() summarise(view, path) },
    all { gen("m") == 2, views.get("m").build.active, views.get("m").build.pending == Set(),
          nondet view = Set("m").oneOf() publish(view) },
    all { gen("m") == 3, phase("r1") == "waiting", requests.get("r1").cookies == 1,
          nondet req = Set("r1").oneOf() requestRecookie(req) },
    all { gen("m") == 3, requests.get("r1").cookie == "queued", requests.get("r1").cookies == 2,
          nondet keep = Set(false).oneOf() deliver(keep) },
    all { gen("m") == 3, phase("r1") == "waiting", requests.get("r1").cookies == 2,
          requests.get("r1").cookie == "delivered",
          nondet req = Set("r1").oneOf() requestPin(req) },
    all { phase("r1") == "pinned", requests.get("r1").reads == Set(),
          nondet req = Set("r1").oneOf() nondet path = Set("m/B").oneOf() read(req, path) },
    all { phase("r1") == "pinned", requests.get("r1").reads != Set(),
          nondet req = Set("r1").oneOf() finish(req) },
  }

  // A write's event is dropped (an overflow): the request's own stat still sees the write and
  // waits; the overflow leaves both views uncertain until a full scan, which certifies `m`.
  action lostHintCaughtByStat = any {
    all { ver("m/A") == 1,
          nondet path = Set("m/A").oneOf() nondet racy = Set(false).oneOf() write(path, racy) },
    all { ver("m/A") == 2, views.get("m").certain, watcher.events != Set(), phase("r1") == "idle",
          nondet seq = Set(first(watcher).seq).oneOf() dropEvent(seq) },
    all { not(views.get("m").certain), phase("r1") == "idle",
          nondet req = Set("r1").oneOf() nondet path = Set("m/A").oneOf() requestStart(req, path) },
    all { phase("r1") == "waiting", gen("m") == 1, not(views.get("m").build.active),
          nondet view = Set("m").oneOf() nondet leave = Set(NO_PATHS).oneOf() reconcileStart(view, leave) },
    all { views.get("m").build.pending == Set("m/A"),
          nondet view = Set("m").oneOf() nondet path = Set("m/A").oneOf() summarise(view, path) },
    all { gen("m") == 1, views.get("m").build.active, views.get("m").build.pending == Set(),
          nondet view = Set("m").oneOf() publish(view) },
    all { gen("m") == 2, requests.get("r1").cookie == "queued",
          nondet keep = Set(false).oneOf() deliver(keep) },
    all { gen("m") == 2, phase("r1") == "waiting", requests.get("r1").cookie == "delivered",
          requests.get("r1").cookies == 1,
          nondet req = Set("r1").oneOf() requestRecookie(req) },
    all { gen("m") == 2, requests.get("r1").cookie == "queued", requests.get("r1").cookies == 2,
          nondet keep = Set(false).oneOf() deliver(keep) },
    all { gen("m") == 2, phase("r1") == "waiting", requests.get("r1").cookie == "delivered",
          requests.get("r1").cookies == 2,
          nondet req = Set("r1").oneOf() requestPin(req) },
    all { phase("r1") == "pinned", requests.get("r1").reads == Set(),
          nondet req = Set("r1").oneOf() nondet path = Set("m/A").oneOf() read(req, path) },
    all { phase("r1") == "pinned", requests.get("r1").reads != Set(),
          nondet req = Set("r1").oneOf() finish(req) },
  }

  // A request about a file of the nested worktree binds to it, never to the parent. Its event is
  // duplicated; the copy arrives after the first cookie and is reconciled, and a second cookie
  // proves nothing changed since, so the answer is complete.
  action nestedWorktreeRequest = any {
    all { ver("n/A") == 1,
          nondet path = Set("n/A").oneOf() nondet racy = Set(false).oneOf() write(path, racy) },
    all { ver("n/A") == 2, phase("r1") == "idle",
          nondet req = Set("r1").oneOf() nondet path = Set("n/A").oneOf() requestStart(req, path) },
    all { phase("r1") == "waiting", gen("n") == 1, watcher.nextSeq == 5,
          nondet keep = Set(true).oneOf() deliver(keep) },
    all { gen("n") == 1, watcher.nextSeq == 6, not(views.get("n").build.active),
          nondet view = Set("n").oneOf() nondet leave = Set(NO_PATHS).oneOf() reconcileStart(view, leave) },
    all { views.get("n").build.pending == Set("n/A"),
          nondet view = Set("n").oneOf() nondet path = Set("n/A").oneOf() summarise(view, path) },
    all { gen("n") == 1, views.get("n").build.active, views.get("n").build.pending == Set(),
          nondet view = Set("n").oneOf() publish(view) },
    all { gen("n") == 2, watcher.events != Set(),
          nondet keep = Set(false).oneOf() deliver(keep) },
    all { gen("n") == 2, watcher.events == Set(), not(views.get("n").build.active),
          views.get("n").dirty == Set("n/A"),
          nondet view = Set("n").oneOf() nondet leave = Set(NO_PATHS).oneOf() reconcileStart(view, leave) },
    all { gen("n") == 2, views.get("n").build.pending == Set("n/A"),
          nondet view = Set("n").oneOf() nondet path = Set("n/A").oneOf() summarise(view, path) },
    all { gen("n") == 2, views.get("n").build.active, views.get("n").build.pending == Set(),
          nondet view = Set("n").oneOf() publish(view) },
    all { gen("n") == 3, requests.get("r1").cookies == 1,
          nondet req = Set("r1").oneOf() requestRecookie(req) },
    all { gen("n") == 3, requests.get("r1").cookie == "queued",
          nondet keep = Set(false).oneOf() deliver(keep) },
    all { gen("n") == 3, phase("r1") == "waiting", requests.get("r1").cookie == "delivered",
          requests.get("r1").cookies == 2,
          nondet req = Set("r1").oneOf() requestPin(req) },
    all { phase("r1") == "pinned", requests.get("r1").reads == Set(),
          nondet req = Set("r1").oneOf() nondet path = Set("n/A").oneOf() read(req, path) },
    all { phase("r1") == "pinned", requests.get("r1").reads != Set(),
          nondet req = Set("r1").oneOf() finish(req) },
  }

  // A burst of writes, one of them racy and one event dropped, then `sync`: it returns only
  // once a rescan started after it has published every write.
  action syncAfterBurst = any {
    all { ver("m/A") == 1,
          nondet path = Set("m/A").oneOf() nondet racy = Set(false).oneOf() write(path, racy) },
    all { ver("m/A") == 2, ver("m/B") == 1,
          nondet path = Set("m/B").oneOf() nondet racy = Set(false).oneOf() write(path, racy) },
    all { ver("m/A") == 2, ver("m/B") == 2,
          nondet path = Set("m/A").oneOf() nondet racy = Set(true).oneOf() write(path, racy) },
    all { ver("m/A") == 3, queued("m") == Set("m/A", "m/B"), views.get("m").certain,
          syncs.get("m").phase == "idle",
          nondet seq = Set(first(watcher).seq).oneOf() dropEvent(seq) },
    all { syncs.get("m").phase == "idle", not(views.get("m").certain),
          nondet view = Set("m").oneOf() syncStart(view) },
    all { syncs.get("m").phase == "waiting", gen("m") == 1, not(views.get("m").build.active),
          nondet view = Set("m").oneOf() nondet leave = Set(NO_PATHS).oneOf() reconcileStart(view, leave) },
    all { views.get("m").build.pending == Set("m/A", "m/B"),
          nondet view = Set("m").oneOf() nondet path = Set("m/A").oneOf() summarise(view, path) },
    all { views.get("m").build.pending == Set("m/B"),
          nondet view = Set("m").oneOf() nondet path = Set("m/B").oneOf() summarise(view, path) },
    all { gen("m") == 1, views.get("m").build.active, views.get("m").build.pending == Set(),
          nondet view = Set("m").oneOf() publish(view) },
    all { syncs.get("m").phase == "waiting", gen("m") == 2,
          nondet view = Set("m").oneOf() syncDone(view) },
  }

  // Both views are at generation 1, `m` with an event in flight: a request in each pins
  // generation 1, and only `n`'s is complete. Completeness is tied to the view, not the number.
  action twoViewsShareAGeneration = any {
    all { ver("m/A") == 1,
          nondet path = Set("m/A").oneOf() nondet racy = Set(false).oneOf() write(path, racy) },
    all { ver("m/A") == 2, phase("r1") == "idle",
          nondet req = Set("r1").oneOf() nondet path = Set("n/A").oneOf() requestStart(req, path) },
    all { phase("r1") == "waiting", phase("r2") == "idle",
          nondet req = Set("r2").oneOf() nondet path = Set("m/B").oneOf() requestStart(req, path) },
    all { phase("r2") == "waiting", first(watcher).seq >= 0,
          nondet keep = Set(false).oneOf() deliver(keep) },
    all { watcher.events == Set(), phase("r1") == "waiting",
          nondet req = Set("r1").oneOf() requestPin(req) },
    all { phase("r1") == "pinned", phase("r2") == "waiting",
          nondet req = Set("r2").oneOf() requestPin(req) },
  }

  // The linked worktree `n` checks out while a request waits; its file events are reconciled,
  // but no watcher sees its HEAD, which lives outside its root. The pin's re-stat sees HEAD has
  // moved: the answer is incomplete and a rescan starts (the FORMAL-03 review's counterexample).
  action headMovedOutsideTheRoot = any {
    all { ver("n/A") == 1,
          nondet path = Set("n/A").oneOf() nondet racy = Set(false).oneOf() write(path, racy) },
    all { ver("n/A") == 2, phase("r1") == "idle",
          nondet req = Set("r1").oneOf() nondet path = Set("n/A").oneOf() requestStart(req, path) },
    all { headOf("n") == 1, not(views.get("n").build.active), gen("n") == 1,
          nondet view = Set("n").oneOf() nondet leave = Set(NO_PATHS).oneOf() reconcileStart(view, leave) },
    all { headOf("n") == 1, views.get("n").build.active,
          nondet view = Set("n").oneOf() checkout(view) },
    all { headOf("n") == 2, views.get("n").build.pending == Set("n/A"),
          nondet view = Set("n").oneOf() nondet path = Set("n/A").oneOf() summarise(view, path) },
    all { gen("n") == 1, views.get("n").build.active, views.get("n").build.pending == Set(),
          nondet view = Set("n").oneOf() publish(view) },
    all { gen("n") == 2, first(watcher).seq >= 0,
          nondet keep = Set(false).oneOf() deliver(keep) },
    all { gen("n") == 2, watcher.events == Set(), not(views.get("n").build.active),
          nondet view = Set("n").oneOf() nondet leave = Set(NO_PATHS).oneOf() reconcileStart(view, leave) },
    all { views.get("n").build.pending == Set("n/A"), gen("n") == 2,
          nondet view = Set("n").oneOf() nondet path = Set("n/A").oneOf() summarise(view, path) },
    all { gen("n") == 2, views.get("n").build.active, views.get("n").build.pending == Set(),
          nondet view = Set("n").oneOf() publish(view) },
    all { gen("n") == 3, phase("r1") == "waiting", requests.get("r1").cookies == 1,
          nondet req = Set("r1").oneOf() requestRecookie(req) },
    all { gen("n") == 3, requests.get("r1").cookie == "queued", requests.get("r1").cookies == 2,
          nondet keep = Set(false).oneOf() deliver(keep) },
    all { gen("n") == 3, phase("r1") == "waiting", requests.get("r1").cookie == "delivered",
          requests.get("r1").cookies == 2,
          nondet req = Set("r1").oneOf() requestPin(req) },
  }

  // Edits land while a request waits: A is dirty from before the cookie, then B and A are
  // written again and A is summarised at its newest version beside the old B, a mix the disk
  // never held. The summary was read after the cookie, so the answer is incomplete (the review's
  // round 2 counterexample, with the fix).
  action concurrentEditsDuringRequest = any {
    all { ver("m/A") == 1,
          nondet path = Set("m/A").oneOf() nondet racy = Set(false).oneOf() write(path, racy) },
    all { ver("m/A") == 2, queued("m") == Set("m/A"),
          nondet keep = Set(false).oneOf() deliver(keep) },
    all { ver("m/A") == 2, phase("r1") == "idle", watcher.events == Set(),
          nondet req = Set("r1").oneOf() nondet path = Set("m/B").oneOf() requestStart(req, path) },
    all { phase("r1") == "waiting", requests.get("r1").cookie == "queued",
          nondet keep = Set(false).oneOf() deliver(keep) },
    all { requests.get("r1").cookie == "delivered", gen("m") == 1, ver("m/B") == 1,
          not(views.get("m").build.active),
          nondet view = Set("m").oneOf() nondet leave = Set(NO_PATHS).oneOf() reconcileStart(view, leave) },
    all { views.get("m").build.active, ver("m/B") == 1,
          nondet path = Set("m/B").oneOf() nondet racy = Set(false).oneOf() write(path, racy) },
    all { views.get("m").build.active, ver("m/B") == 2, ver("m/A") == 2,
          nondet path = Set("m/A").oneOf() nondet racy = Set(false).oneOf() write(path, racy) },
    all { ver("m/A") == 3, views.get("m").build.pending == Set("m/A"),
          nondet view = Set("m").oneOf() nondet path = Set("m/A").oneOf() summarise(view, path) },
    all { gen("m") == 1, views.get("m").build.active, views.get("m").build.pending == Set(),
          nondet view = Set("m").oneOf() publish(view) },
    all { gen("m") == 2, phase("r1") == "waiting",
          nondet req = Set("r1").oneOf() requestPin(req) },
  }

  // A view settled in slices (IDX-09d): two files are delivered; a part takes one and publishes;
  // a request on that file is answered between the slices from the new generation, incomplete
  // while the other waits. The first file changes again; a request on the second waits, so the
  // next part must take it beside the first; after its second cookie the answer is complete.
  action settleInSlices = any {
    all { ver("m/A") == 1,
          nondet path = Set("m/A").oneOf() nondet racy = Set(false).oneOf() write(path, racy) },
    all { ver("m/A") == 2, ver("m/B") == 1,
          nondet path = Set("m/B").oneOf() nondet racy = Set(false).oneOf() write(path, racy) },
    all { ver("m/B") == 2, file("m/B").recent, tick },
    all { ver("m/B") == 2, not(file("m/B").recent), queued("m") != Set(), phase("r2") == "idle",
          nondet keep = Set(false).oneOf() deliver(keep) },
    all { queued("m") == Set(), gen("m") == 1, not(views.get("m").build.active),
          views.get("m").dirty == Set("m/A", "m/B"),
          nondet view = Set("m").oneOf() nondet take = Set(Set("m/A")).oneOf() reconcilePart(view, take) },
    all { gen("m") == 1, views.get("m").build.pending == Set("m/A"),
          nondet view = Set("m").oneOf() nondet path = Set("m/A").oneOf() summarise(view, path) },
    all { gen("m") == 1, views.get("m").build.active, views.get("m").build.pending == Set(),
          nondet view = Set("m").oneOf() publish(view) },
    all { gen("m") == 2, phase("r2") == "idle", not(views.get("m").build.active),
          nondet req = Set("r2").oneOf() nondet path = Set("m/A").oneOf() requestStart(req, path) },
    all { requests.get("r2").cookie == "queued",
          nondet keep = Set(false).oneOf() deliver(keep) },
    all { phase("r2") == "waiting", requests.get("r2").cookie == "delivered",
          nondet req = Set("r2").oneOf() requestPin(req) },
    all { phase("r2") == "pinned",
          nondet req = Set("r2").oneOf() finish(req) },
    all { phase("r2") == "done", ver("m/A") == 2,
          nondet path = Set("m/A").oneOf() nondet racy = Set(false).oneOf() write(path, racy) },
    all { ver("m/A") == 3, file("m/A").recent, tick },
    all { phase("r2") == "done", ver("m/A") == 3, not(file("m/A").recent), queued("m") != Set(),
          nondet keep = Set(false).oneOf() deliver(keep) },
    all { phase("r2") == "done", phase("r1") == "idle", queued("m") == Set(),
          views.get("m").dirty == Set("m/A", "m/B"),
          nondet req = Set("r1").oneOf() nondet path = Set("m/B").oneOf() requestStart(req, path) },
    all { requests.get("r1").cookie == "queued",
          nondet keep = Set(false).oneOf() deliver(keep) },
    all { phase("r1") == "waiting", requests.get("r1").cookie == "delivered", gen("m") == 2,
          not(views.get("m").build.active), views.get("m").dirty == Set("m/A", "m/B"),
          nondet view = Set("m").oneOf() nondet take = Set(Set("m/A", "m/B")).oneOf()
          reconcilePart(view, take) },
    all { views.get("m").build.pending == Set("m/A", "m/B"),
          nondet view = Set("m").oneOf() nondet path = Set("m/A").oneOf() summarise(view, path) },
    all { views.get("m").build.pending == Set("m/B"),
          nondet view = Set("m").oneOf() nondet path = Set("m/B").oneOf() summarise(view, path) },
    all { gen("m") == 2, views.get("m").build.active, views.get("m").build.pending == Set(),
          nondet view = Set("m").oneOf() publish(view) },
    all { gen("m") == 3, phase("r1") == "waiting", requests.get("r1").cookie == "delivered",
          requests.get("r1").cookies == 1,
          nondet req = Set("r1").oneOf() requestRecookie(req) },
    all { gen("m") == 3, phase("r1") == "waiting", requests.get("r1").cookie == "delivered",
          requests.get("r1").cookies == 2,
          nondet req = Set("r1").oneOf() requestPin(req) },
    all { phase("r1") == "pinned", requests.get("r1").reads == Set(),
          nondet req = Set("r1").oneOf() nondet path = Set("m/B").oneOf() read(req, path) },
    all { phase("r1") == "pinned", requests.get("r1").reads != Set(),
          nondet req = Set("r1").oneOf() finish(req) },
  }

  // Rescans during a settle (IDX-09e): two files are delivered, as the settle delivers the files
  // an open deferred. A commit moves HEAD; a request on `m/A` sees it before the watcher's event
  // arrives (a linked worktree's commit raises none at all), so its stat sets the rescan. The
  // rescan reads the request's file and leaves `m/B`, and the request is answered from it,
  // incomplete. The commit's `.git` event then sets a second rescan (as a `.git` hint, an overflow
  // or a file created during the settle does), which leaves `m/B` again; a part takes it, and a
  // request after it is complete.
  action rescanDuringASettle = any {
    all { ver("m/A") == 1,
          nondet path = Set("m/A").oneOf() nondet racy = Set(false).oneOf() write(path, racy) },
    all { ver("m/A") == 2, ver("m/B") == 1,
          nondet path = Set("m/B").oneOf() nondet racy = Set(false).oneOf() write(path, racy) },
    all { ver("m/B") == 2, file("m/B").recent, tick },
    all { ver("m/B") == 2, not(file("m/B").recent), headOf("m") == 1, queued("m") != Set(),
          nondet keep = Set(false).oneOf() deliver(keep) },
    all { headOf("m") == 1, views.get("m").dirty == Set("m/A", "m/B"),
          nondet view = Set("m").oneOf() moveHead(view) },
    all { headOf("m") == 2, phase("r1") == "idle",
          nondet req = Set("r1").oneOf() nondet path = Set("m/A").oneOf() requestStart(req, path) },
    all { phase("r1") == "waiting", gen("m") == 1, not(views.get("m").build.active),
          views.get("m").rescan,
          nondet view = Set("m").oneOf() nondet leave = Set(Set("m/B")).oneOf()
          reconcileStart(view, leave) },
    all { gen("m") == 1, views.get("m").build.pending == Set("m/A"),
          nondet view = Set("m").oneOf() nondet path = Set("m/A").oneOf() summarise(view, path) },
    all { gen("m") == 1, views.get("m").build.active, views.get("m").build.pending == Set(),
          nondet view = Set("m").oneOf() publish(view) },
    all { gen("m") == 2, phase("r1") == "waiting", first(watcher).seq >= 0,
          nondet keep = Set(false).oneOf() deliver(keep) },
    all { gen("m") == 2, phase("r1") == "waiting", watcher.events == Set(),
          requests.get("r1").cookie == "delivered",
          nondet req = Set("r1").oneOf() requestPin(req) },
    all { phase("r1") == "pinned",
          nondet req = Set("r1").oneOf() finish(req) },
    all { phase("r1") == "done", gen("m") == 2, not(views.get("m").build.active),
          views.get("m").rescan,
          nondet view = Set("m").oneOf() nondet leave = Set(Set("m/B")).oneOf()
          reconcileStart(view, leave) },
    all { gen("m") == 2, phase("r1") == "done", views.get("m").build.active,
          views.get("m").build.pending == Set(),
          nondet view = Set("m").oneOf() publish(view) },
    all { gen("m") == 3, not(views.get("m").build.active), views.get("m").dirty == Set("m/B"),
          nondet view = Set("m").oneOf() nondet take = Set(Set("m/B")).oneOf()
          reconcilePart(view, take) },
    all { views.get("m").build.pending == Set("m/B"),
          nondet view = Set("m").oneOf() nondet path = Set("m/B").oneOf() summarise(view, path) },
    all { gen("m") == 3, views.get("m").build.active, views.get("m").build.pending == Set(),
          nondet view = Set("m").oneOf() publish(view) },
    all { gen("m") == 4, phase("r2") == "idle",
          nondet req = Set("r2").oneOf() nondet path = Set("m/B").oneOf() requestStart(req, path) },
    all { phase("r2") == "waiting", requests.get("r2").cookie == "queued",
          nondet keep = Set(false).oneOf() deliver(keep) },
    all { phase("r2") == "waiting", requests.get("r2").cookie == "delivered",
          nondet req = Set("r2").oneOf() requestPin(req) },
    all { phase("r2") == "pinned", requests.get("r2").reads == Set(),
          nondet req = Set("r2").oneOf() nondet path = Set("m/B").oneOf() read(req, path) },
    all { phase("r2") == "pinned", requests.get("r2").reads != Set(),
          nondet req = Set("r2").oneOf() finish(req) },
  }

  // The scenario tests replay each script as named actions, step for step (a test that draws a
  // nondeterministic value runs 10000 times, so the scripts themselves serve only the traces).

  run checkoutDuringReconcileTest =
    init
      .then(write("m/A", false))
      .then(deliver(false))
      .then(reconcileStart("m", NO_PATHS))
      .then(checkout("m"))
      .then(summarise("m", "m/A"))
      .then(publish("m"))
      .then(requestStart("r1", "m/B"))
      .then(4.reps(_ => deliver(false)))
      .then(reconcileStart("m", NO_PATHS))
      .then(summarise("m", "m/A"))
      .then(summarise("m", "m/B"))
      .then(publish("m"))
      .then(requestRecookie("r1"))
      .then(deliver(false))
      .then(requestPin("r1"))
      .then(read("r1", "m/B"))
      .then(finish("r1"))
      .expect({
        val r = requests.get("r1")
        and {
          // generation 2 holds the checkout's m/A but the old HEAD; the stat waited for 3
          r.phase == "done", r.gen == 3, r.syncHead == 2, r.snapHead == 2,
          r.snap.get("m/B").ver == 2, r.complete, allInvariants,
        }
      })

  run lostHintCaughtByStatTest =
    init
      .then(write("m/A", false))
      .then(dropEvent(0))
      .then(requestStart("r1", "m/A"))
      .then(reconcileStart("m", NO_PATHS))
      .then(summarise("m", "m/A"))
      .then(publish("m"))
      .then(deliver(false))
      .then(requestRecookie("r1"))
      .then(deliver(false))
      .then(requestPin("r1"))
      .then(read("r1", "m/A"))
      .then(finish("r1"))
      .expect({
        val r = requests.get("r1")
        and {
          r.phase == "done", r.gen == 2, r.snap.get("m/A").ver == 2, r.complete,
          views.get("m").certain, not(views.get("n").certain), allInvariants,
        }
      })

  // Before the generation that holds the write is published, the request cannot pin.
  run lostHintKeepsTheRequestWaitingTest =
    init
      .then(write("m/A", false))
      .then(dropEvent(0))
      .then(requestStart("r1", "m/A"))
      .then(reconcileStart("m", NO_PATHS))
      .then(summarise("m", "m/A"))
      .expect(and { phase("r1") == "waiting", not(pinnable("r1")), allInvariants })

  run nestedWorktreeRequestTest =
    init
      .then(write("n/A", false))
      .then(requestStart("r1", "n/A"))
      .then(deliver(true))
      .then(reconcileStart("n", NO_PATHS))
      .then(summarise("n", "n/A"))
      .then(publish("n"))
      .then(2.reps(_ => deliver(false)))
      .then(reconcileStart("n", NO_PATHS))
      .then(summarise("n", "n/A"))
      .then(publish("n"))
      .then(requestRecookie("r1"))
      .then(deliver(false))
      .then(requestPin("r1"))
      .then(read("r1", "n/A"))
      .then(finish("r1"))
      .expect({
        val r = requests.get("r1")
        and {
          r.phase == "done", r.view == "n", r.gen == 3, r.snap.keys() == Set("n/A"),
          r.snap.get("n/A").ver == 2, r.cookies == 2,
          // the duplicate came after the first cookie and was reconciled; the second cookie
          // proves nothing changed since
          r.complete, watcher.events == Set(),
          views.get("m").published.index.keys() == Set("m/A", "m/B"), gen("m") == 1,
          allInvariants,
        }
      })

  run syncAfterBurstTest =
    init
      .then(write("m/A", false))
      .then(write("m/B", false))
      .then(write("m/A", true))
      .then(dropEvent(0))
      .then(syncStart("m"))
      .then(reconcileStart("m", NO_PATHS))
      .then(summarise("m", "m/A"))
      .then(summarise("m", "m/B"))
      .then(publish("m"))
      .then(syncDone("m"))
      .expect(and {
        syncs.get("m").phase == "done", gen("m") == 2,
        views.get("m").published.index.get("m/A").ver == 3,
        views.get("m").published.index.get("m/B").ver == 2, allInvariants,
      })

  // sync does not return while a write it saw is unpublished.
  run syncWaitsForTheBurstTest =
    init
      .then(write("m/A", false))
      .then(write("m/B", false))
      .then(syncStart("m"))
      .then(reconcileStart("m", NO_PATHS))
      .then(summarise("m", "m/A"))
      .expect(and { syncs.get("m").phase == "waiting", not(syncable("m")), allInvariants })

  run twoViewsShareAGenerationTest =
    init
      .then(write("m/A", false))
      .then(requestStart("r1", "n/A"))
      .then(requestStart("r2", "m/B"))
      .then(3.reps(_ => deliver(false)))
      .then(requestPin("r1"))
      .then(requestPin("r2"))
      .expect({
        val r1 = requests.get("r1")
        val r2 = requests.get("r2")
        and {
          r1.view == "n", r2.view == "m", r1.gen == 1, r2.gen == 1,
          r1.complete, r1.completeView == "n", not(r2.complete), r2.completeView == "m",
          allInvariants,
        }
      })

  run headMovedOutsideTheRootTest =
    init
      .then(write("n/A", false))
      .then(requestStart("r1", "n/A"))
      .then(reconcileStart("n", NO_PATHS))
      .then(checkout("n"))
      .then(summarise("n", "n/A"))
      .then(publish("n"))
      .then(3.reps(_ => deliver(false)))
      .then(reconcileStart("n", NO_PATHS))
      .then(summarise("n", "n/A"))
      .then(publish("n"))
      .then(requestRecookie("r1"))
      .then(deliver(false))
      .then(requestPin("r1"))
      .expect({
        val r = requests.get("r1")
        and {
          r.gen == 3, r.snapHead == 1, r.pinHead == 2, not(r.complete), views.get("n").rescan,
          allInvariants,
        }
      })

  // HEAD-only moves and staging: `m`'s git event forces a rescan; `n`'s raise none, and the
  // stat alone sees them.
  run headOnlyMovesTest =
    init
      .then(moveHead("m"))
      .then(stage("n"))
      .then(requestStart("r1", "m/A"))
      .then(requestStart("r2", "n/A"))
      .expect(and {
        phase("r1") == "waiting", requests.get("r1").needGen == 2,
        phase("r2") == "waiting", requests.get("r2").needGen == 2,
        views.get("m").rescan, views.get("n").rescan, queued("m") == Set("m/.git"),
        queued("n") == Set(), allInvariants,
      })

  // A lost watch: changes raise no event, the view is uncertain and answers incompletely until
  // the watch is back and a full scan started after it has published.
  run watchLostAndRestoredTest =
    init
      .then(watchLost("m"))
      .then(write("m/A", false))
      .then(watchRestored("m"))
      .then(requestStart("r1", "m/B"))
      .then(reconcileStart("m", NO_PATHS))
      .then(summarise("m", "m/A"))
      .then(publish("m"))
      .then(deliver(false))
      .then(requestRecookie("r1"))
      .then(deliver(false))
      .then(requestPin("r1"))
      .expect({
        val r = requests.get("r1")
        and {
          watcher.events == Set(), views.get("m").certain, r.complete,
          r.snap.get("m/A").ver == 2, allInvariants,
        }
      })

  // The racy-mtime rule: a racy write keeps the stat; the summary taken in the same tick is
  // racy, so the stat does not prove the entry fresh and the request waits.
  run racyWriteCaughtByTheRuleTest =
    init
      .then(write("m/A", false))
      .then(deliver(false))
      .then(reconcileStart("m", NO_PATHS))
      .then(summarise("m", "m/A"))
      .then(publish("m"))
      .then(write("m/A", true))
      .then(requestStart("r1", "m/A"))
      .expect(and {
        views.get("m").published.index.get("m/A").recent,
        file("m/A").stat == views.get("m").published.index.get("m/A").stat,
        requests.get("r1").needGen == 3, not(pinnable("r1")), allInvariants,
      })

  run settleInSlicesTest =
    init
      .then(write("m/A", false))
      .then(write("m/B", false))
      .then(tick)
      .then(2.reps(_ => deliver(false)))
      .then(reconcilePart("m", Set("m/A")))
      .then(summarise("m", "m/A"))
      .then(publish("m"))
      .then(requestStart("r2", "m/A"))
      .then(deliver(false))
      .then(requestPin("r2"))
      .expect({
        val r = requests.get("r2")
        and {
          r.gen == 2, not(r.complete), r.snap.get("m/A").ver == 2,
          views.get("m").dirty == Set("m/B"), allInvariants,
        }
      })
      .then(finish("r2"))
      .then(write("m/A", false))
      .then(tick)
      .then(deliver(false))
      .then(requestStart("r1", "m/B"))
      .then(deliver(false))
      .expect(needed("m") == Set("m/B") and views.get("m").dirty == Set("m/A", "m/B"))
      .then(reconcilePart("m", Set("m/A", "m/B")))
      .then(summarise("m", "m/A"))
      .then(summarise("m", "m/B"))
      .then(publish("m"))
      .then(requestRecookie("r1"))
      .then(deliver(false))
      .then(requestPin("r1"))
      .then(read("r1", "m/B"))
      .then(finish("r1"))
      .expect({
        val r = requests.get("r1")
        and {
          r.gen == 3, r.complete, r.snap.get("m/A").ver == 3, r.snap.get("m/B").ver == 2,
          views.get("m").dirty == Set(), allInvariants,
        }
      })

  run rescanDuringASettleTest =
    init
      .then(write("m/A", false))
      .then(write("m/B", false))
      .then(tick)
      .then(2.reps(_ => deliver(false)))
      .then(moveHead("m"))
      .then(requestStart("r1", "m/A"))
      .expect(views.get("m").rescan and needed("m") == Set("m/A"))
      .then(reconcileStart("m", Set("m/B")))
      .expect(and {
        views.get("m").build.certifies, views.get("m").build.pending == Set("m/A"),
        views.get("m").build.head == 2, views.get("m").dirty == Set("m/B"),
      })
      .then(summarise("m", "m/A"))
      .then(publish("m"))
      .then(2.reps(_ => deliver(false)))
      .then(requestPin("r1"))
      .expect({
        val r = requests.get("r1")
        and {
          // the rescan's generation holds what the request's stat saw, and m/B is still dirty
          r.gen == 2, not(r.complete), r.snap.get("m/A").ver == 2, r.snap.get("m/B").ver == 1,
          r.snapHead == 2, views.get("m").certain, views.get("m").rescan, allInvariants,
        }
      })
      .then(finish("r1"))
      .then(reconcileStart("m", Set("m/B")))
      .expect(views.get("m").build.pending == Set() and views.get("m").dirty == Set("m/B"))
      .then(publish("m"))
      .then(reconcilePart("m", Set("m/B")))
      .then(summarise("m", "m/B"))
      .then(publish("m"))
      .then(requestStart("r2", "m/B"))
      .then(deliver(false))
      .then(requestPin("r2"))
      .then(read("r2", "m/B"))
      .then(finish("r2"))
      .expect({
        val r = requests.get("r2")
        and {
          r.gen == 4, r.complete, r.snap.get("m/A").ver == 2, r.snap.get("m/B").ver == 2,
          views.get("m").dirty == Set(), allInvariants,
        }
      })

  // A rescan may not leave a path a waiting request needs, nor leave anything without a rescan
  // due; each refusal has a twin from the same state that is taken.
  run twoDeliveredAndACommitSeenByARequestOnA =
    init
      .then(write("m/A", false))
      .then(write("m/B", false))
      .then(tick)
      .then(2.reps(_ => deliver(false)))
      .then(moveHead("m"))
      .then(requestStart("r1", "m/A"))

  run rescanLeavingANeededPathIsRefusedTest =
    twoDeliveredAndACommitSeenByARequestOnA.then(reconcileStart("m", Set("m/A"))).fail()

  run rescanLeavingAnotherPathIsTakenTest =
    twoDeliveredAndACommitSeenByARequestOnA
      .then(reconcileStart("m", Set("m/B")))
      .expect(views.get("m").dirty == Set("m/B") and allInvariants)

  run twoDelivered =
    init
      .then(write("m/A", false))
      .then(write("m/B", false))
      .then(2.reps(_ => deliver(false)))

  run leavingWithoutARescanIsRefusedTest =
    twoDelivered.then(reconcileStart("m", Set("m/B"))).fail()

  run takingEverythingWithoutARescanIsTakenTest =
    twoDelivered
      .then(reconcileStart("m", NO_PATHS))
      .expect(views.get("m").build.pending == Set("m/A", "m/B") and views.get("m").dirty == Set())

  // sync waits for its rescan and for every part after it that takes what the rescan left.
  run syncWaitsForTheLeftPathsTest =
    twoDelivered
      .then(syncStart("m"))
      .then(reconcileStart("m", Set("m/B")))
      .then(summarise("m", "m/A"))
      .then(publish("m"))
      .expect(and { syncs.get("m").scanned, not(syncable("m")), allInvariants })
      .then(reconcilePart("m", Set("m/B")))
      .expect(not(syncable("m")))
      .then(summarise("m", "m/B"))
      .then(publish("m"))
      .then(syncDone("m"))
      .expect(and { syncs.get("m").phase == "done", gen("m") == 3, allInvariants })

  // A part may not leave out a path a waiting request needs, nor run while a rescan is due; each
  // refusal has a twin from the same state that is taken.
  run twoDeliveredAndARequestOnB =
    init
      .then(write("m/A", false))
      .then(write("m/B", false))
      .then(tick)
      .then(2.reps(_ => deliver(false)))
      .then(requestStart("r1", "m/B"))

  run partLeavingOutANeededPathIsRefusedTest =
    twoDeliveredAndARequestOnB.then(reconcilePart("m", Set("m/A"))).fail()

  run partTakingTheNeededPathIsTakenTest =
    twoDeliveredAndARequestOnB
      .then(reconcilePart("m", Set("m/A", "m/B")))
      .expect(views.get("m").build.pending == Set("m/A", "m/B") and allInvariants)

  run deliveredWithARescanDue =
    init
      .then(write("m/A", false))
      .then(moveHead("m"))
      .then(2.reps(_ => deliver(false)))

  run partDuringARescanIsRefusedTest =
    deliveredWithARescanDue.then(reconcilePart("m", Set("m/A"))).fail()

  run rescanTakesWhatAPartWouldTest =
    deliveredWithARescanDue
      .then(reconcileStart("m", NO_PATHS))
      .expect(views.get("m").build.certifies and views.get("m").build.pending == Set("m/A"))

  // ---- Broken-model controls (ver §6.1): each must violate its named invariant ----

  // Publishes before every summary the build names is stored.
  action publishEarly(view: str): bool = {
    val v = views.get(view)
    all {
      v.build.active,
      v.published.gen < MAX_GEN,
      views' = views.set(view, { ...v, prevGen: v.published.gen,
        published: { gen: v.published.gen + 1, head: v.build.head, gitIndex: v.build.gitIndex,
                     index: v.build.index, readAt: v.build.readAt },
        build: IDLE_BUILD }),
      tree' = tree, watcher' = watcher, store' = store, requests' = requests, syncs' = syncs,
    }
  }

  // Publishes the build under the same generation number.
  action publishSameGeneration(view: str): bool = {
    val v = views.get(view)
    all {
      v.build.active,
      v.build.pending == Set(),
      views' = views.set(view, { ...v, prevGen: v.published.gen,
        published: { ...v.published, head: v.build.head, gitIndex: v.build.gitIndex,
                     index: v.build.index, readAt: v.build.readAt },
        build: IDLE_BUILD }),
      tree' = tree, watcher' = watcher, store' = store, requests' = requests, syncs' = syncs,
    }
  }

  // Reads from the newest published generation mid-request instead of the pinned one.
  action readNewest(req: str, path: str): bool = {
    val r = requests.get(req)
    val g = views.get(r.view).published
    all {
      r.phase == "pinned",
      g.index.keys().contains(path),
      requests' = requests.set(req, { ...r,
        reads: r.reads.union(Set({ view: r.view, gen: g.gen, path: path, ver: g.index.get(path).ver })),
      }),
      tree' = tree, watcher' = watcher, views' = views, store' = store, syncs' = syncs,
    }
  }

  // Skips the freshness stat: may pin whatever is published.
  action requestStartUnstatted(req: str, path: str): bool = {
    val view = innermost(path)
    all {
      phase(req) == "idle" or phase(req) == "done",
      requests' = requests.set(req, { ...IDLE_REQUEST, phase: "waiting", path: path, view: view,
        needGen: gen(view), syncDisk: restrict(tree.files, view), syncHead: headOf(view),
        syncGitIndex: gitIndex(view), history: Set(restrict(tree.files, view)) }),
      tree' = tree, watcher' = watcher, views' = views, store' = store, syncs' = syncs,
    }
  }

  // Stats without the racy-mtime rule: trusts an entry summarised in its file's write tick.
  action requestStartWithoutRacyRule(req: str, path: str): bool = {
    val view = innermost(path)
    val g = views.get(view).published
    val i = g.index.get(path)
    val fresh = i.stat == file(path).stat and i.present == file(path).present
      and g.head == headOf(view) and g.gitIndex == gitIndex(view)
    all {
      phase(req) == "idle" or phase(req) == "done",
      fresh,
      requests' = requests.set(req, { ...IDLE_REQUEST, phase: "waiting", path: path, view: view,
        needGen: g.gen, syncDisk: restrict(tree.files, view), syncHead: headOf(view),
        syncGitIndex: gitIndex(view), history: Set(restrict(tree.files, view)) }),
      tree' = tree, watcher' = watcher, views' = views, store' = store, syncs' = syncs,
    }
  }

  // Binds a request to the outermost worktree, as if the parent owned its nested worktrees.
  action requestStartInParent(req: str, path: str): bool = {
    val view = outermost(path)
    all {
      phase(req) == "idle" or phase(req) == "done",
      requests' = requests.set(req, { ...IDLE_REQUEST, phase: "waiting", path: path, view: view,
        needGen: gen(view), syncDisk: restrict(tree.files, view), syncHead: headOf(view),
        syncGitIndex: gitIndex(view), history: Set(restrict(tree.files, view)) }),
      tree' = tree, watcher' = watcher, views' = views, store' = store, syncs' = syncs,
    }
  }

  // The parent's scan includes the nested worktree's files.
  action reconcileStartScanningNested(view: str): bool = {
    val v = views.get(view)
    val scanned = PATHS.filter(p => ENCLOSING.get(p).contains(view))
    all {
      not(v.build.active),
      views' = views.set(view, { ...v, dirty: Set(), rescan: false, build: { ...IDLE_BUILD,
        active: true, head: headOf(view), gitIndex: gitIndex(view),
        index: scanned.mapBy(p => file(p)), readAt: scanned.mapBy(_ => -1), pending: scanned } }),
      tree' = tree, watcher' = watcher, store' = store, requests' = requests, syncs' = syncs,
    }
  }

  // Pins with another view's completeness, at the same generation number.
  action requestPinWithOtherCompleteness(req: str, other: str): bool = {
    val r = requests.get(req)
    val o = views.get(other)
    all {
      pinnable(req),
      other != r.view,
      o.published.gen == gen(r.view),
      requests' = requests.set(req, { ...r, phase: "pinned", gen: gen(r.view),
        snap: views.get(r.view).published.index, snapHead: views.get(r.view).published.head,
        snapGitIndex: views.get(r.view).published.gitIndex,
        pinDisk: restrict(tree.files, r.view), pinHead: headOf(r.view), pinGitIndex: gitIndex(r.view),
        complete: completeAt(r, o, o.published.head == headOf(other)), completeView: other,
        completeGen: o.published.gen }),
      tree' = tree, watcher' = watcher, views' = views, store' = store, syncs' = syncs,
    }
  }

  // Pins without the re-stat of HEAD and the index (the design the review found wanting).
  action requestPinWithoutRestat(req: str): bool = {
    val r = requests.get(req)
    val v = views.get(r.view)
    val g = v.published
    all {
      pinnable(req),
      requests' = requests.set(req, { ...r, phase: "pinned",
        gen: g.gen, snap: g.index, snapHead: g.head, snapGitIndex: g.gitIndex,
        pinDisk: restrict(tree.files, r.view), pinHead: headOf(r.view), pinGitIndex: gitIndex(r.view),
        complete: completeAt(r, v, true), completeView: r.view, completeGen: g.gen }),
      tree' = tree, watcher' = watcher, views' = views, store' = store, syncs' = syncs,
    }
  }

  // Pins without the late-read rule: a generation holding a summary read after the cookie is
  // taken as complete without a second cookie (round 2's design, the review's round 2 HIGH).
  action requestPinIgnoringLateReads(req: str): bool = {
    val r = requests.get(req)
    val v = views.get(r.view)
    val g = v.published
    val restat = g.head == headOf(r.view) and g.gitIndex == gitIndex(r.view)
    all {
      pinnable(req),
      requests' = requests.set(req, { ...r, phase: "pinned",
        gen: g.gen, snap: g.index, snapHead: g.head, snapGitIndex: g.gitIndex,
        pinDisk: restrict(tree.files, r.view), pinHead: headOf(r.view), pinGitIndex: gitIndex(r.view),
        complete: completeAt({ ...r, cookieSeq: watcher.nextSeq }, v, restat),
        completeView: r.view, completeGen: g.gen }),
      views' = if (restat) views else views.setBy(r.view, x => { ...x, rescan: true }),
      tree' = tree, watcher' = watcher, store' = store, syncs' = syncs,
    }
  }

  // Claims completeness without waiting for the cookie, or trusting a view whose watch is down.
  action requestPinTrusting(req: str): bool = {
    val r = requests.get(req)
    val v = views.get(r.view)
    val g = v.published
    all {
      pinnable(req),
      requests' = requests.set(req, { ...r, phase: "pinned",
        gen: g.gen, snap: g.index, snapHead: g.head, snapGitIndex: g.gitIndex,
        pinDisk: restrict(tree.files, r.view), pinHead: headOf(r.view), pinGitIndex: gitIndex(r.view),
        complete: completeAt({ ...r, cookie: "delivered" }, { ...v, watched: true, certain: true },
          g.head == headOf(r.view) and g.gitIndex == gitIndex(r.view)),
        completeView: r.view, completeGen: g.gen }),
      tree' = tree, watcher' = watcher, views' = views, store' = store, syncs' = syncs,
    }
  }

  // An overflow marks only one view, though the instance serves both.
  action dropEventMarkingOne(seq: int, view: str): bool = {
    val lost = watcher.events.filter(e => e.seq == seq)
    all {
      lost != Set(),
      watcher' = { ...watcher, events: watcher.events.exclude(lost) },
      views' = views.setBy(view, v => { ...v, rescan: true, certain: false,
        build: { ...v.build, certifies: false } }),
      tree' = tree, store' = store, requests' = requests, syncs' = syncs,
    }
  }

  // Takes part of the delivered events without the paths the waiting requests need.
  action reconcilePartIgnoringRequests(view: str, take: Set[str]): bool = {
    val v = views.get(view)
    val g = v.published
    all {
      not(v.build.active),
      not(v.rescan),
      take != Set(),
      take.subseteq(v.dirty),
      views' = views.set(view, { ...v, dirty: v.dirty.exclude(take), build: {
        active: true, head: g.head, gitIndex: g.gitIndex,
        index: g.index.keys().mapBy(p => if (take.contains(p)) file(p) else g.index.get(p)),
        readAt: g.readAt, pending: take, certifies: false, syncEpoch: -1,
      }}),
      tree' = tree, watcher' = watcher, store' = store, requests' = requests, syncs' = syncs,
    }
  }

  // A rescan that leaves paths unread and drops them from `dirty` (IDX-09e's fault): the view
  // looks settled while a generation holds their old versions.
  action reconcileStartDroppingLeft(view: str, leave: Set[str]): bool = {
    val v = views.get(view)
    val g = v.published
    val changed =
      if (v.rescan) viewPaths(view).filter(p => statChanged(g.index.get(p), file(p))) else Set()
    val todo = v.dirty.union(changed).exclude(leave)
    all {
      not(v.build.active),
      v.dirty != Set() or v.rescan,
      v.rescan or leave == Set(),
      leave.subseteq(v.dirty),
      leave.intersect(needed(view)) == Set(),
      views' = views.set(view, { ...v, dirty: Set(), rescan: false, build: {
        active: true,
        head: if (v.rescan) headOf(view) else g.head,
        gitIndex: if (v.rescan) gitIndex(view) else g.gitIndex,
        index: g.index.keys().mapBy(p => if (todo.contains(p)) file(p) else g.index.get(p)),
        readAt: g.readAt,
        pending: todo,
        certifies: v.rescan and v.watched,
        syncEpoch: if (v.rescan and syncs.get(view).phase == "waiting") syncs.get(view).epoch else -1,
      }}),
      tree' = tree, watcher' = watcher, store' = store, requests' = requests, syncs' = syncs,
    }
  }

  // A rescan that may leave the file a waiting request's stat saw change.
  action reconcileStartLeavingNeeded(view: str, leave: Set[str]): bool = {
    val v = views.get(view)
    val g = v.published
    val changed =
      if (v.rescan) viewPaths(view).filter(p => statChanged(g.index.get(p), file(p))) else Set()
    val todo = v.dirty.union(changed).exclude(leave)
    all {
      not(v.build.active),
      v.dirty != Set() or v.rescan,
      v.rescan or leave == Set(),
      leave.subseteq(v.dirty),
      views' = views.set(view, { ...v, dirty: leave, rescan: false, build: {
        active: true,
        head: if (v.rescan) headOf(view) else g.head,
        gitIndex: if (v.rescan) gitIndex(view) else g.gitIndex,
        index: g.index.keys().mapBy(p => if (todo.contains(p)) file(p) else g.index.get(p)),
        readAt: g.readAt,
        pending: todo,
        certifies: v.rescan and v.watched,
        syncEpoch: if (v.rescan and syncs.get(view).phase == "waiting") syncs.get(view).epoch else -1,
      }}),
      tree' = tree, watcher' = watcher, store' = store, requests' = requests, syncs' = syncs,
    }
  }

  // Returns from sync once its rescan has published, without waiting for the paths it left.
  action syncDoneBeforeTheParts(view: str): bool = all {
    syncs.get(view).phase == "waiting",
    syncs.get(view).scanned,
    syncs' = syncs.setBy(view, s => { ...s, phase: "done" }),
    tree' = tree, watcher' = watcher, views' = views, store' = store, requests' = requests,
  }

  // Returns from sync without waiting for its rescan.
  action syncDoneEarly(view: str): bool = all {
    syncs.get(view).phase == "waiting",
    syncs' = syncs.setBy(view, s => { ...s, phase: "done" }),
    tree' = tree, watcher' = watcher, views' = views, store' = store, requests' = requests,
  }

  run publishEarlyViolatesAtomicPublishTest =
    init
      .then(write("m/A", false))
      .then(deliver(false))
      .then(reconcileStart("m", NO_PATHS))
      .then(publishEarly("m"))
      .expect(not(AtomicPublish))

  run publishSameGenerationViolatesGenerationsIncreaseTest =
    init
      .then(write("m/A", false))
      .then(deliver(false))
      .then(reconcileStart("m", NO_PATHS))
      .then(summarise("m", "m/A"))
      .then(publishSameGeneration("m"))
      .expect(not(GenerationsIncrease))

  run readNewestViolatesNoMixedGenerationTest =
    init
      .then(requestStart("r1", "m/B"))
      .then(deliver(false))
      .then(requestPin("r1"))
      .then(write("m/B", false))
      .then(deliver(false))
      .then(reconcileStart("m", NO_PATHS))
      .then(summarise("m", "m/B"))
      .then(publish("m"))
      .then(readNewest("r1", "m/B"))
      .expect(not(NoMixedGeneration))

  // The same interleaving with the correct read stays in the pinned generation.
  run readPinnedKeepsNoMixedGenerationTest =
    init
      .then(requestStart("r1", "m/B"))
      .then(deliver(false))
      .then(requestPin("r1"))
      .then(write("m/B", false))
      .then(deliver(false))
      .then(reconcileStart("m", NO_PATHS))
      .then(summarise("m", "m/B"))
      .then(publish("m"))
      .then(read("r1", "m/B"))
      .expect(and {
        allInvariants,
        requests.get("r1").reads == Set({ view: "m", gen: 1, path: "m/B", ver: 1 }),
      })

  run skipStatViolatesFreshAfterStatTest =
    init
      .then(write("m/A", false))
      .then(requestStartUnstatted("r1", "m/A"))
      .then(requestPin("r1"))
      .expect(not(FreshAfterStat))

  run noRacyRuleViolatesFreshAfterStatTest =
    init
      .then(write("m/A", false))
      .then(deliver(false))
      .then(reconcileStart("m", NO_PATHS))
      .then(summarise("m", "m/A"))
      .then(publish("m"))
      .then(write("m/A", true))
      .then(requestStartWithoutRacyRule("r1", "m/A"))
      .then(requestPin("r1"))
      .expect(not(FreshAfterStat))

  run otherViewsCompletenessViolatesCompleteIsHonestTest =
    init
      .then(write("m/A", false))
      .then(requestStart("r1", "n/A"))
      .then(requestStart("r2", "m/B"))
      .then(3.reps(_ => deliver(false)))
      .then(requestPin("r1"))
      .then(requestPinWithOtherCompleteness("r2", "n"))
      .expect(and {
        requests.get("r2").complete, requests.get("r2").gen == 1,
        not(CompleteIsHonest), not(NoCrossView),
      })

  run parentBindingViolatesNoCrossViewTest =
    init
      .then(requestStartInParent("r1", "n/A"))
      .then(requestPin("r1"))
      .expect(not(NoCrossView))

  run parentScanningNestedViolatesNoCrossViewTest =
    init
      .then(reconcileStartScanningNested("m"))
      .then(summarise("m", "m/A"))
      .then(summarise("m", "m/B"))
      .then(summarise("m", "n/A"))
      .then(publish("m"))
      .expect(not(NoCrossView))

  // The review's counterexample: without the re-stat at the pin, the stale HEAD stands behind a
  // complete=true; with it (headMovedOutsideTheRootTest), the answer is incomplete.
  run noRestatViolatesCompleteIsHonestTest =
    init
      .then(write("n/A", false))
      .then(requestStart("r1", "n/A"))
      .then(reconcileStart("n", NO_PATHS))
      .then(checkout("n"))
      .then(summarise("n", "n/A"))
      .then(publish("n"))
      .then(3.reps(_ => deliver(false)))
      .then(reconcileStart("n", NO_PATHS))
      .then(summarise("n", "n/A"))
      .then(publish("n"))
      .then(requestRecookie("r1"))
      .then(deliver(false))
      .then(requestPinWithoutRestat("r1"))
      .expect(and { requests.get("r1").complete, not(CompleteIsHonest) })

  // A HEAD-only move in the linked worktree raises no event: only the stat at the pin sees it.
  run noRestatAfterHeadOnlyMoveViolatesCompleteIsHonestTest =
    init
      .then(requestStart("r1", "n/A"))
      .then(moveHead("n"))
      .then(deliver(false))
      .then(requestPinWithoutRestat("r1"))
      .expect(and { requests.get("r1").complete, not(CompleteIsHonest) })

  run noCookieViolatesCompleteIsHonestTest =
    init
      .then(write("m/A", false))
      .then(requestStart("r1", "m/B"))
      .then(requestPinTrusting("r1"))
      .expect(and { requests.get("r1").complete, not(CompleteIsHonest) })

  run trustingALostWatchViolatesCompleteIsHonestTest =
    init
      .then(watchLost("m"))
      .then(reconcileStart("m", NO_PATHS))
      .then(publish("m"))
      .then(write("m/A", false))
      .then(requestStart("r1", "m/B"))
      .then(requestPinTrusting("r1"))
      .expect(and { requests.get("r1").complete, not(CompleteIsHonest) })

  run overflowMarkingOneViewViolatesCompleteIsHonestTest =
    init
      .then(write("m/A", false))
      .then(dropEventMarkingOne(0, "n"))
      .then(requestStart("r1", "m/B"))
      .then(deliver(false))
      .then(requestPin("r1"))
      .expect(and { requests.get("r1").complete, not(CompleteIsHonest) })

  // The review's round 2 counterexample: the disk went (A2,B1), (A2,B2), (A3,B2); a summary of A
  // read after the cookie sits beside the old B. Without the late-read rule the answer claims
  // complete over (A3,B1), a state the disk never held.
  run nonSnapshotCompleteViolatesCompleteIsHonestTest =
    init
      .then(write("m/A", false))
      .then(deliver(false))
      .then(requestStart("r1", "m/B"))
      .then(deliver(false))
      .then(reconcileStart("m", NO_PATHS))
      .then(write("m/B", false))
      .then(write("m/A", false))
      .then(summarise("m", "m/A"))
      .then(publish("m"))
      .then(requestPinIgnoringLateReads("r1"))
      .expect({
        val r = requests.get("r1")
        and {
          r.complete, not(CompleteIsHonest),
          r.snap.get("m/A").ver == 3, r.snap.get("m/B").ver == 1,
        }
      })

  // The same interleaving with the real pin: the late read makes the answer incomplete.
  run nonSnapshotIsIncompleteTest =
    init
      .then(write("m/A", false))
      .then(deliver(false))
      .then(requestStart("r1", "m/B"))
      .then(deliver(false))
      .then(reconcileStart("m", NO_PATHS))
      .then(write("m/B", false))
      .then(write("m/A", false))
      .then(summarise("m", "m/A"))
      .then(publish("m"))
      .then(requestPin("r1"))
      .expect(and { not(requests.get("r1").complete), allInvariants })

  // And with the second cookie: its round trip brings the queued writes in, so the generation is
  // rebuilt after it, its summaries are late again, and the answer stays incomplete (two cookies
  // at most).
  run secondCookieWithEditsInFlightStaysIncompleteTest =
    init
      .then(write("m/A", false))
      .then(deliver(false))
      .then(requestStart("r1", "m/B"))
      .then(deliver(false))
      .then(reconcileStart("m", NO_PATHS))
      .then(write("m/B", false))
      .then(write("m/A", false))
      .then(summarise("m", "m/A"))
      .then(publish("m"))
      .then(requestRecookie("r1"))
      .then(3.reps(_ => deliver(false)))
      .then(reconcileStart("m", NO_PATHS))
      .then(summarise("m", "m/A"))
      .then(summarise("m", "m/B"))
      .then(publish("m"))
      .then(requestPin("r1"))
      .expect(and {
        requests.get("r1").cookies == 2, not(requests.get("r1").complete), allInvariants,
      })

  run syncDoneEarlyViolatesSyncIsBarrierTest =
    init
      .then(write("m/A", false))
      .then(syncStart("m"))
      .then(syncDoneEarly("m"))
      .expect(not(SyncIsBarrier))

  // A slice that leaves out the file a waiting request's stat saw change publishes the generation
  // the request waits for without it: the request pins an older version than its stat saw.
  run partIgnoringRequestsViolatesFreshAfterStatTest =
    init
      .then(write("m/A", false))
      .then(write("m/B", false))
      .then(tick)
      .then(2.reps(_ => deliver(false)))
      .then(requestStart("r1", "m/B"))
      .then(deliver(false))
      .then(reconcilePartIgnoringRequests("m", Set("m/A")))
      .then(summarise("m", "m/A"))
      .then(publish("m"))
      .then(requestPin("r1"))
      .expect(not(FreshAfterStat))

  // A rescan during a settle leaves m/B unread and drops it from `dirty`: generation 2 holds
  // m/B's old version, the view looks settled, and a request after the write is answered
  // complete from a state its history never held.
  run rescanDroppingLeftViolatesCompleteIsHonestTest =
    twoDelivered
      .then(tick)
      .then(moveHead("m"))
      .then(deliver(false))
      .then(reconcileStartDroppingLeft("m", Set("m/B")))
      .then(summarise("m", "m/A"))
      .then(publish("m"))
      .then(requestStart("r1", "m/A"))
      .then(deliver(false))
      .then(requestPin("r1"))
      .expect(and { requests.get("r1").complete, not(CompleteIsHonest) })

  // The same rescan with the left path kept dirty: the answer is incomplete.
  run rescanLeavingIsIncompleteTest =
    twoDelivered
      .then(tick)
      .then(moveHead("m"))
      .then(deliver(false))
      .then(reconcileStart("m", Set("m/B")))
      .then(summarise("m", "m/A"))
      .then(publish("m"))
      .then(requestStart("r1", "m/A"))
      .then(deliver(false))
      .then(requestPin("r1"))
      .expect(and { not(requests.get("r1").complete), allInvariants })

  // A rescan leaves the file a waiting request's stat saw change: the generation the request
  // waits for holds the old version.
  run rescanLeavingNeededViolatesFreshAfterStatTest =
    twoDeliveredAndACommitSeenByARequestOnA
      .then(reconcileStartLeavingNeeded("m", Set("m/A")))
      .then(summarise("m", "m/B"))
      .then(publish("m"))
      .then(2.reps(_ => deliver(false)))
      .then(requestPin("r1"))
      .expect(not(FreshAfterStat))

  run syncDoneBeforeThePartsViolatesSyncIsBarrierTest =
    twoDelivered
      .then(syncStart("m"))
      .then(reconcileStart("m", Set("m/B")))
      .then(summarise("m", "m/A"))
      .then(publish("m"))
      .then(syncDoneBeforeTheParts("m"))
      .expect(not(SyncIsBarrier))
}
