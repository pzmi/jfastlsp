# jfastlsp: formal models

The contracts the conformance drivers implement (plan §7, ver §6, ADR 0007). Each section states
a model's:

- **stimuli:** the actions a trace takes, and the picks the driver decodes;
- **observation:** the state the driver must produce from the real code after every step, which
  `jfl-conformance`'s replay compares with the model's;
- **invariants;**
- **named scenarios** and their regression traces;
- **broken-model controls;**
- **verification budget.**

A model lands before its driver (the FORMAL lane). Until its driver lands, the model is listed as
pending in `crates/jfl-conformance/src/registry.rs`, with the task that brings the driver. That
task must have an open card in `doc/tasks.md`, and the `conformance-replay` stage warns about the
model.

## `views`: views, generations, freshness and completeness

`formal/views.qnt` (FORMAL-03; plan §4, §7.1, §2.1; ADR 0014, ADR 0017, ADR 0037). The driver
is IDX-09's, over `jfl-application`'s view registry with the testkit's scripted source tree,
watcher, repository probe and summary store. `views` is pending on IDX-09 until then.

### The world

- **Two worktrees.** `m`, and `n`, a linked agent worktree nested inside `m` at
  `m/.claude/worktrees/n`.
- **Three files:** `m/A`, `m/B` and `n/A`.
  - `n/A` lies under both roots and belongs to `n`, its innermost worktree (plan §4.1).
  - `m`'s index never holds `n/A` (plan §4.2).
- **Git metadata.** Each worktree has a HEAD and a git index.
  - `m`'s live in `m/.git`, which `m`'s watcher covers.
  - `n`'s live in `m/.git/worktrees/n/`, outside `n`'s root, so no watcher of `n` sees them change
    (plan §4.3).
- **One watcher instance** serves both views. Its events arrive in order.
- **Two requests,** `r1` and `r2`, and one `sync` per view.

A file on disk carries:

- **`ver`:** its content. Every change gives it a higher one.
- **`present`:** whether it exists.
- **`stat`:** what a stat sees, its mtime and size. A racy write keeps it.
- **`recent`:** whether it was written within the current mtime tick.

An index entry carries the same fields as the file was when summarised, with one difference:
there `recent` marks the summary as racy, taken within the tick of the file's last write.

Bounds: versions up to 6, HEAD up to 3, generations up to 8. An action that would pass a bound is
disabled.

### Design decisions the model fixes (ADR 0037, Proposed)

1. **A view is one worktree root.**
   - A request binds to the innermost worktree enclosing its file.
   - A view's scan skips nested worktrees.
   - Pinned by `NoCrossView`, and the controls `requestStartInParent` and
     `reconcileStartScanningNested`.
2. **A request pins a (view, generation) pair, and its completeness is tied to that pair.**
   - Every view counts its own generations: `m` and `n` both start at generation 1.
   - So a completeness taken from another view at the same number is a lie, even when the numbers
     agree.
   - Pinned by `CompleteIsHonest` and `NoCrossView`, the scenario `twoViewsShareAGeneration` and
     the control `requestPinWithOtherCompleteness`. This came from the IDX-02 review.
3. **The stat decides whether a request must wait.**
   - It covers the file, HEAD and the index, and applies the racy rule (item 7).
   - If the published generation does not provably hold what the stat saw, the stat marks the
     path dirty, or starts a rescan for HEAD or the index.
   - The request then waits for a generation built after its stat.
   - Pinned by `FreshAfterStat`, and the controls `requestStartUnstatted` and
     `requestStartWithoutRacyRule`.
4. **Watcher events are hints, delivered in order.**
   - An event can be duplicated.
   - **An overflow** (inotify's `IN_Q_OVERFLOW`; FSEvents' `MustScanSubDirs` or dropped-events
     flags) drops events and says only that something was lost. It makes every view that shares
     the instance rescan and become uncertain (control `dropEventMarkingOne`).
   - **A lost watch** makes the view unwatched and uncertain, and its changes raise no event:
     - an `add_watch` error (ENOSPC, EMFILE);
     - `IN_IGNORED`, `IN_DELETE_SELF` or `IN_UNMOUNT`;
     - FSEvents' `RootChanged` or `EventIdsWrapped`.
   - **Re-establishing the watch** starts a rescan.
   - **A view becomes certain again** only when a full scan that started with its watch up, and
     saw no loss while it ran, publishes. This is watch, then scan.
5. **A request's sync point is its stat and its cookie.**
   - While the view is watched, the request writes a cookie file into the view.
   - The cookie's event comes back through the same in-order stream, so every event raised before
     it has been delivered by then.
   - The cookie is modelled as an event, not read from the queue: the daemon never sees in-flight
     events.
6. **`complete=true` needs all of these at the pin (`completeAt`):**
   - the cookie back;
   - no published summary read after that cookie was written (`lateReads` empty). Every summary
     carries a read stamp (`readAt`) on a clock ordered with cookie writes. While a summary was
     read after the cookie, the generation can hold a newer version of one file beside an older
     version of another, a state the disk never held: a call moved from A to B could be in
     neither. The request then writes a second cookie at the pin (`requestRecookie`), and is
     complete only if that round trip leaves no summary read after it. At most two cookies per
     request;
   - the view watched and certain;
   - no delivered event awaiting a build, no rescan due and no build in progress;
   - HEAD and the index re-statted at the pin and equal to the pinned generation's. A change there
     starts a rescan, and the answer is incomplete. The FORMAL-03 review found this missing.

   Pinned by `CompleteIsHonest`, the scenarios `headMovedOutsideTheRoot` and
   `concurrentEditsDuringRequest`, and the controls `requestPinWithoutRestat`,
   `requestPinIgnoringLateReads` (the review's round 2 counterexample) and `requestPinTrusting`.
7. **The racy-mtime rule.** A write within the mtime tick of the file's previous write, of the
   same size, keeps the stat, though the watcher still reports it. A summary taken within its
   file's write tick is racy, and no stat proves a racy entry fresh: neither the request's stat
   nor a rescan's.
8. **A generation is published atomically**, only once every summary it names is stored
   (`AtomicPublish`; control `publishEarly`).
9. **A request reads only from the generation it pinned** (`NoMixedGeneration`; control
   `readNewest`).
10. **`sync` is a barrier.** It starts a rescan and returns only once a rescan that started after it
    has published (`SyncIsBarrier`; control `syncDoneEarly`).

What `complete=true` promises is a **snapshot**: the answer's index equals one state the view's
files actually had between the request's sync point and its pin, and HEAD and the index equal the
disk's at the pin. So `exact ∪ possible` holds every reference of a real disk state (ADR 0014).
Without the second-cookie rule, round 2 of this model promised less (each file somewhere between
its sync-point and pin versions), and the review showed that admitted a mix no disk ever held.

**Cost:** at most two cookie round trips per request that claims completeness, the second only
when its view was re-summarised during the request. IDX-09 measures them against plan §1.7's
100 ms freshness target.

### IDX-09's obligations

These are the ADR 0037 design as IDX-09 must build it. The driver checks each one against the
model.

1. **Watch, then scan, then publish:** at view open, at daemon start and after every watch is
   re-established.
2. **Scan each new directory after adding its watch.** Files created in it before the watch was
   added raise no event.
3. **Treat every loss signal as a loss:**
   - an `add_watch` error (ENOSPC, EMFILE);
   - `IN_IGNORED`, `IN_DELETE_SELF` and `IN_UNMOUNT`;
   - `IN_Q_OVERFLOW`, for every view sharing the inotify instance;
   - FSEvents' `MustScanSubDirs`, `UserDropped`, `KernelDropped`, `RootChanged` and
     `EventIdsWrapped`.

   Each makes the view uncertain until a full scan, started after the watch is back, publishes.
4. **Check the filesystem type (`statfs`).** A view on a network, FUSE or overlay filesystem is
   never watched, so it never claims `complete`. This covers NFS, SMB, sshfs, 9p, virtiofs and
   the lower layer of an overlayfs. Its answers say why they are incomplete.
5. **Apply git's racy-mtime rule in every stat:** an entry whose mtime is within the timestamp
   granularity of the scan that summarised it is not proven fresh.
6. **The cookie:**
   - one per request, written at the stat into a watched directory of the view;
   - the cookie and the view's files share one inotify instance or FSEvents stream, with no
     debouncer, coalescer or other reordering layer between the kernel and the daemon (the
     ordering is the cookie's whole proof);
   - waited for with a deadline (an answer that cannot wait is incomplete);
   - on macOS, behind `FSEventStreamFlushSync`;
   - every summary stamped with when it was read, on a clock ordered with cookie writes; a
     second cookie at the pin when the pinned generation holds a summary read after the first,
     and incomplete if that is not enough (two at most). Measure both round trips against the
     100 ms freshness target (plan §1.7).
7. **Re-stat HEAD and the index at the pin.** For a linked worktree, `HEAD` and `index` live under
   the common directory's `worktrees/<name>/`.
8. **Writes through a writable `mmap`** raise no event and change the mtime only at writeback, so
   such a file cannot be proven fresh. This is an accepted limitation (ADR 0037): `complete=true`
   does not cover a file another process maps writable.
9. **The observation is exact all the way down.** Every type of the driver's observation (the
   View, the Request, the Build, every nested record) uses `deny_unknown_fields` and a derived
   `PartialEq`, never a hand-written one. The replay checks only the top-level keys against the
   trace's variables, so a narrower nested type would compare less than the model holds; IDX-09's
   review checks this.

### Not modelled yet

- **Overlays** (plan §4.4: an open buffer applies only while it is newer than the disk, and disk
  wins over an older overlay).
- **Worktrees added or removed** under a running parent view.
- **Lazily opened views.** They are covered by obligation 1 (watch, then scan).
- **Deadlines** (plan §2.7), including the bound on a request's wait for its cookie: the model's
  `requestPin` waits for a queued cookie; IDX-09 answers incomplete when the deadline passes
  first.

A change that implements any of these extends the model first (ADR 0007).

### Stimuli: the driver's actions

`step` picks one of four equally likely groups, `diskStep`, `watcherStep` (a delivery, or one of
the rarer losses), `daemonStep` and `requestStep`, then an enabled action in it. Flat, the six
disk and four watcher stimuli crowded out the request path: a complete answer through a second
cookie was not reached in 40000 samples. `--mbt` records the innermost action taken, so the
groups never appear in a trace.

Each action's parameters are nondeterministic picks named as the parameter. `quint run --mbt`
records them in `mbt::nondetPicks`; decode them with `Step::pick`. A pick the action did not use
is `None`. The action itself is `mbt::actionTaken`.

| Action | Picks | What the driver does to the real code |
|:--|:--|:--|
| `init` | none | Builds a fresh registry over a scripted tree that holds every file at version 1 and both HEADs and indexes at 1. It watches, scans and publishes generation 1 of both views. |
| `write` | `path`, `racy` | Writes a new version of the file. With `racy`, the write keeps the file's mtime and size; it is enabled only when the file was written in the current tick. The watcher queues an event if the view is watched. |
| `delete` | `path` | Deletes the file. The watcher queues an event if the view is watched. |
| `tick` | none | The scripted clock passes the mtime tick of every recent write. |
| `checkout` | `view` | Moves HEAD, the index and every file of the worktree. The watcher queues one event per file, plus a git event for `m`. |
| `moveHead` | `view` | `commit` or `reset --soft`: HEAD moves. A git event is queued for `m` only. |
| `stage` | `view` | `git add`: the index changes. A git event is queued for `m` only. |
| `deliver` | `keep` | The watcher delivers the next event in order. With `keep`, a file event is also queued again, as a duplicate. |
| `dropEvent` | `seq` | An overflow drops that event and reports a loss. |
| `watchLost` | `view` | The view's watch is lost. |
| `watchRestored` | `view` | The view's watch is re-established. |
| `reconcileStart` | `view` | The registry starts the next build. On a rescan, the build takes every path a stat cannot prove unchanged. |
| `summarise` | `view`, `path` | The build re-reads the path, summarises it and stores the summary. |
| `publish` | `view` | The registry publishes the build atomically. |
| `requestStart` | `req`, `path` | Starts the request: bind, stat, and write a cookie. |
| `requestRecookie` | `req` | The second cookie, at the pin: written when the published generation holds a summary read after the request's first cookie. |
| `requestPin` | `req` | Pins the request, after its queued cookie is back: re-stat, then compute completeness. |
| `read` | `req`, `path` | The request reads the path from its pinned generation. |
| `finish` | `req` | The request answers. |
| `syncStart` | `view` | Starts `sync` on the worktree. |
| `syncDone` | `view` | The `sync` returns. |

A step the real code refuses is a divergence.

A scenario's scripted action is not a driver action. In a scenario trace, `mbt::actionTaken`
records the action the script took, such as `write`, so a regression trace holds only the
actions in the table.

### Observation: what the driver produces after every step

The observation is a `Serialize` and `Deserialize` type into which the model's state decodes
(`deny_unknown_fields`). The replay refuses an observation whose serialised fields are not
exactly the trace's variables. ITF encodes a map as `#map`, a set as `#set`, a record as a JSON
object and an integer as `#bigint` (`i64`).

The model has six variables:

| Variable | Type | Observed from |
|:--|:--|:--|
| `tree` | `{ files, heads, gitIndexes }` | The scripted tree: each file's `{ ver, present, stat, recent }`, and each worktree's HEAD and index. |
| `watcher` | `{ events, nextSeq }` | The scripted watcher's queue. Each event is `{ seq, view, path, req }`: a file's or `m/.git`'s event, or a cookie (`path` `""`, `req` set). The scripted watcher numbers its events as the model does: a stimulus's events are `nextSeq + RANK`, and `nextSeq` then moves on by 4; a cookie or a duplicate is `nextSeq`, and it moves on by 1. |
| `views` | view → View | The registry, per view (below). |
| `store` | set of `{ path, ver }` | The summaries the store holds. |
| `requests` | req → Request | Each request the driver started (below). |
| `syncs` | view → `{ phase, epoch, scanned, need, needHead }` | Each `sync`. `epoch` counts the syncs started. `scanned` means a rescan started after this `sync` has published. `need` and `needHead` are the scripted tree at `syncStart`. |

Each View has these fields:

- **`dirty`:** the delivered file events that no build has taken.
- **`rescan`:** whether a full reconcile is due.
- **`watched`:** whether the view's watch is established.
- **`certain`:** whether the view has seen no loss since a full scan that started with its watch up.
- **`build`:** the build in progress, or the idle build.
  - `active` and `pending`: whether a build is in progress, and the paths it has yet to summarise.
  - `head`, `gitIndex` and `index`: what the build records.
  - `readAt`: each summary's read stamp (path → int; -1 for the initial scan). The driver's
    clock is the scripted watcher's `nextSeq` when the summary was read.
  - `certifies`: whether, on publish, the build makes the view certain.
  - `syncEpoch`: the `sync` the build serves, or -1.
- **`published`:** the published generation: `{ gen, head, gitIndex, index, readAt }`, over the
  view's own paths only.
- **`prevGen`:** the generation the last publish replaced. The driver remembers it at each
  publish.

Each Request has these fields:

- **`phase`:** `idle`, `waiting`, `pinned` or `done`.
- **`path`** and **`view`:** the request's file, and the view it was bound to.
- **`needGen`:** the first generation the request may pin.
- **`cookie`:** the latest cookie's state: `none`, `queued`, `delivered` or `lost`.
- **`cookieSeq`** and **`cookies`:** the latest cookie's place on the clock, and how many the
  request wrote (one or two).
- **`history`:** the set of states of the view's files from the sync point to the pin. The driver
  records the scripted tree's files after each disk change while the request waits.
- **`syncDisk`**, **`syncHead`** and **`syncGitIndex`:** the disk at the sync point.
- **`gen`**, **`snap`**, **`snapHead`** and **`snapGitIndex`:** the pinned generation.
- **`pinDisk`**, **`pinHead`** and **`pinGitIndex`:** the disk at the pin.
- **`complete`**, **`completeView`** and **`completeGen`:** the completeness, and the view and
  generation it was computed for.
- **`reads`:** one `{ view, gen, path, ver }` per value read.

The `sync*`, `pin*` and `history` fields are the driver's records of the scripted tree at those steps; the
registry need not keep them.

**The driver must observe the view identity** (the IDX-02 review):

- each request's `view` and `completeView`;
- the `view` of each read.

The generation number alone cannot tell `m`'s generation 1 from `n`'s. The control
`requestPinWithOtherCompleteness` is the case only the view identity catches.

### Mapping to IDX-02's domain types

The model's completeness is IDX-02's `completeness(visible, index)` (`jfl-domain`'s
`envelope.rs`), with the view added. Before M3 a module is the worktree's whole source set
(plan §2.1). So `visible` is `[ModuleName::worktree()]`, and there is one `ModuleIndex`.

- **`completeGen`** is `Completeness::generation()`.
- **`completeView`** has no field in IDX-02's types, because `Completeness` records only its
  `Generation`.
  - IDX-09 computes completeness from the `IndexState` of the request's own pinned view and
    generation.
  - The driver observes that `IndexState`'s worktree.
  - IDX-02's `GenerationMismatch` catches a completeness computed at another generation.
    Nothing in IDX-02 catches one computed at another view's generation of the same number.
    IDX-09 closes that, for example by keying the pinned state by (worktree, generation).
- **`complete`** is `Completeness::is_complete()`, with `completeAt` folded in:
  - **`ModuleIndex::indexed == files`:** the snap holds every path of the view.
  - **`ModuleIndex::stale == 0`:** no path of the view is dirty.
  - **`IncompleteReason::PendingChanges { events }`:** counts a cookie not yet back, a due
    rescan, a build in progress and the dirty paths.
  - **A view that is unwatched or uncertain:** IDX-09 adds a reason for it, "filesystem not
    watched" or "watcher lost events", since IDX-02 has none.
- **A view whose files are all deleted** is still listed and complete: its index holds entries
  with `present: false`. That is `ModuleIndex::empty(module, generation, 0)`, never "not
  indexed".
- **`GenerationMismatch`** is the model's `completeGen != gen`, which violates
  `CompleteIsHonest`.

### Invariants

`val allInvariants` is the conjunction of these seven. Apalache and the simulation check it.

| Invariant | States |
|:--|:--|
| `NoMixedGeneration` | Every value a request read belongs to its pinned generation, at that generation's version. |
| `GenerationsIncrease` | A view's published generation is greater than the one it replaced, and a pinned generation is never past the published one. |
| `AtomicPublish` | Every summary a published generation names is in the store. |
| `FreshAfterStat` | The pinned generation holds what the request's stat observed (the file's version, HEAD and the index), or newer. |
| `CompleteIsHonest` | `complete=true` only for the request's own view at its pinned generation, when the pinned index equals one state of the request's `history` (a real disk state between its sync point and its pin, file for file), and HEAD and the index equal the disk's at the pin. |
| `NoCrossView` | Each view's index holds exactly its own paths. A started request is bound to the innermost worktree of its path. An answered request's pin, reads and completeness are all from that view. |
| `SyncIsBarrier` | A finished `sync`'s view has published everything the disk held when the `sync` started. |

`SyncIsBarrier` adds the barrier of plan §4.3 to the six invariants of plan §7.1.

### Scenarios and regression traces

Each scenario is a scripted action. Its guarded branches enable exactly one action in each state
of the run, and the script stops after its last step. So a scenario's trace does not depend on
the seed: each script, run at 12 seeds from `0x1` to `0xdead`, gave one action sequence.

Each scenario has a regression trace in `formal/traces/regression/manifest.toml`, generated at
seed `0x3`, and a test of the same name. The test takes the same steps as named actions and
asserts the outcome.

| Scenario (`--step`) | Steps | What it shows |
|:--|:--|:--|
| `checkoutDuringReconcile` | 20 | A checkout lands while a build is in progress. Generation 2 holds the re-read file but the old HEAD. A request's stat sees the new HEAD and waits for the rescan's generation 3, whose summaries were read after its cookie; a second cookie proves nothing changed since, and the answer is complete. |
| `lostHintCaughtByStat` | 12 | An overflow drops a write's event and makes both views uncertain. The request's own stat sees the write and waits. A full scan makes `m` certain again, while `n` stays uncertain. After a second cookie the answer is complete. `lostHintKeepsTheRequestWaitingTest` checks it cannot pin earlier. |
| `nestedWorktreeRequest` | 16 | A request on `n/A` binds to `n`, never to `m`. Its event is duplicated; the copy comes after the first cookie and is reconciled, and a second cookie proves nothing changed since, so the answer is complete. `m` stays at generation 1 without `n/A`. |
| `syncAfterBurst` | 10 | Three writes, one of them racy, and one event dropped; then `sync`. The `sync` returns only once a rescan that started after it holds every write. `syncWaitsForTheBurstTest` checks it cannot return earlier. |
| `twoViewsShareAGeneration` | 8 | Both views are at generation 1, `m` with an event in flight. Requests in `n` and in `m` both pin generation 1, and only `n`'s is complete. |
| `concurrentEditsDuringRequest` | 10 | The review's round 2 counterexample, with the fix. Edits land while a request waits, and `m/A` is summarised at its newest version beside the old `m/B`, a mix the disk never held. The summary was read after the cookie, so the answer is incomplete (`nonSnapshotIsIncompleteTest`; with a second cookie and the edits still arriving, `secondCookieWithEditsInFlightStaysIncompleteTest`). |
| `headMovedOutsideTheRoot` | 15 | The review's round 1 counterexample, with the fix. `n` checks out while a request waits. Its file events are reconciled, but nothing sees its HEAD. The pin's re-stat does see it, so the answer is incomplete and a rescan starts. |

Three tests have no trace:

- **`headOnlyMovesTest`:** `m`'s HEAD-only move and staging raise a git event; `n`'s raise none,
  and the stat alone sees them.
- **`watchLostAndRestoredTest`:** a change while the watch is down raises no event, and the view is
  complete only after the restored watch's full scan.
- **`racyWriteCaughtByTheRuleTest`:** the racy rule makes the request wait.

### Broken-model controls

Each control is an action variant with a Quint test that expects its invariant to be false:

| Control | Fault | Violates |
|:--|:--|:--|
| `publishEarly` | Publishes before every summary is stored. | `AtomicPublish` |
| `publishSameGeneration` | Publishes under the same number. | `GenerationsIncrease` |
| `readNewest` | Reads from the newest generation mid-request. The same interleaving with `read` holds: `readPinnedKeepsNoMixedGenerationTest`. | `NoMixedGeneration` |
| `requestStartUnstatted` | Skips the stat. | `FreshAfterStat` |
| `requestStartWithoutRacyRule` | Trusts a racy entry. | `FreshAfterStat` |
| `requestPinWithOtherCompleteness` | Takes the other view's completeness at the same number. | `CompleteIsHonest`, `NoCrossView` |
| `requestStartInParent` | Binds a nested file to the parent worktree. | `NoCrossView` |
| `reconcileStartScanningNested` | The parent's scan includes the nested worktree. | `NoCrossView` |
| `requestPinIgnoringLateReads` | Takes a generation holding a summary read after the cookie as complete, without a second cookie. The review's round 2 counterexample: `nonSnapshotCompleteViolatesCompleteIsHonestTest` (a snapshot `(A3, B1)` the disk never held), which `nonSnapshotIsIncompleteTest` passes. | `CompleteIsHonest` |
| `requestPinWithoutRestat` | No re-stat at the pin. This is the review's round 1 counterexample: `noRestatViolatesCompleteIsHonestTest`, which `headMovedOutsideTheRootTest` passes. It is also run after a HEAD-only move. | `CompleteIsHonest` |
| `requestPinTrusting` | No cookie, and trusts a lost watch: two tests. | `CompleteIsHonest` |
| `dropEventMarkingOne` | An overflow marks one view of a shared instance. | `CompleteIsHonest` |
| `syncDoneEarly` | `sync` returns without its rescan. | `SyncIsBarrier` |

Plan §7.1 names IDX-09's own negative control: publishing before the last summary is stored. It
must diverge at the `publish` step of a trace whose build still has a pending path.

### Budget

Measured in the sandbox: 4 CPUs, Quint 0.33.1 on the TypeScript backend, Apalache 0.62.4 on the
Temurin JDK 21. The registry holds two numbers per model: `steps`, Apalache's bound, and
`sim_steps`, the depth of the simulation and of `conformance-replay`'s generated traces.

| Check | Budget | Time |
|:--|:--|:--|
| `formal-typecheck` and `formal-test` (29 views tests, 1 smoke) | every `check` | about 4 s |
| `formal-freshness`: eight traces regenerated | every `check` | about 2 s per trace |
| `conformance-replay` | 10 traces of `sim_steps` (30), generated twice, every `check` | a few seconds. The traces are read, not replayed, until IDX-09. |
| `formal-verify views`: Apalache | `steps = 5` | 858 s |
| `formal-verify views`: simulation | 20000 samples of `sim_steps` (30) | 172 s |
| `formal-verify views` in all | weekly in `formal.yml` (60-minute job) | 1048 s; perhaps twice that on a CI runner |

**Apalache's time grows steeply with the bound.** On round 3's model it took 188 s at length 4
and 858 s at length 5. Round 2's model, without the request history and read stamps, took 40 s at
length 3, 145 s at 4 and 636 s at 5. Length 6 would be roughly four times length 5 and would not
fit the job, so the bound is 5, down from round 1's 6.

**What length 5 covers:** short interleavings, exhaustively. A whole write, deliver, reconcile,
summarise and publish cycle takes 5 steps. It reaches no complete answer after a change: write,
deliver, reconcile, summarise, publish, request, cookie and pin take 8 steps or more, and the
second cookie adds 2. **The depth guarantee rests on the simulation,** run weekly in
`formal-verify` and at a fresh seed in every `conformance-replay`:

- **`allInvariants`, no violation:**
  - round 3: 20000 samples of 30 steps at seed 0x3c1, and of 40 steps at 0x7e5;
  - round 2's model: four seeds of 20000 × 30.
- **The grouped `step` reaches every completeness path.** Each of these witness states was found:

  | Witness state | Found after |
  |:--|:--|
  | a complete answer at generation 3 or later | 6 s |
  | an incomplete answer after a second cookie | 10 s |
  | a complete answer whose history holds two states or more | 7 s |
  | a complete answer through a second cookie | 89 s, about 12000 samples of 30 steps |

  The complete answer through a second cookie is the rarest. The flat `step` never reached it in
  40000 samples, so its honesty is pinned mainly by the scripted scenarios `checkoutDuringReconcile`,
  `lostHintCaughtByStat` and `nestedWorktreeRequest`, and by their tests.

The registry's simulation depth is what made the review's counterexamples findable. Raise either
number only with a measurement, while `formal.yml`'s 60-minute job stays well under its limit.
Never trim a bound to fit (ver §12).

## `daemon`: the lock, the socket, spawn, reconnect and overlays

`formal/daemon.qnt` (FORMAL-04; plan §5.2, §5.3, §7.2; ADR 0016, ADR 0021). The driver is
IDX-12's, over `jfl-composition`'s daemon host and launcher in-process, with real Unix sockets and
lock files in a tempdir. `daemon` is pending on IDX-12 until then.

### The world

- **One state directory**, mode `0700` (plan §5.3: the access control; its modes and the peer-uid
  check are IDX-12's integration tests, not the model's). In it:
  - `daemon.lock`, the owner's lock: an flock on the file's inode. The file is created by the
    first lock and never removed. The kernel drops an flock when the last descriptor of its open
    file description closes, so a dead daemon never holds it.
  - `sock`, the socket file: present or not, and which daemon bound it. A file nobody listens on
    is stale: connecting to it fails (`ECONNREFUSED`), and binding over it fails
    (`EADDRINUSE`). Plan §5.3's fallback path under `/tmp/jfl-<euid>/` behaves the same.
  - `spawn.lock`, the proxies' spawn lock, another flock.
- **Two proxies**, `p1` and `p2`: client sessions (`jfl lsp`, or a CLI call), each with its open
  documents and at most one unanswered request.
- **Three daemon slots**, `d1` to `d3`: processes. A slot is reused once its process is dead and
  nothing refers to it (no socket file it bound, no connection, no proxy waiting on it).
- **Three connection slots**, `c1` to `c3`, and two documents, `a` and `b`.
- **The initial state** is a state directory either clean, or as a crash left it: a stale socket
  file and the lock file (the pick `stale`).

### Design decisions the model fixes

1. **The owner is the daemon holding the flock on the inode `daemon.lock` names.** A pid file is
   never trusted (ADR 0021). The lock file is never unlinked: a daemon that removed it, or broke
   a lock it took for stale, would let a second daemon lock a new inode while the first still
   holds the old one (control `lockBreakingStale`).
2. **Start order: lock, unlink, bind, ready.** A daemon tries the lock first; if it is held, it
   reports the error on the notify socket and exits 1. As owner it removes any socket file it
   finds (stale: only the owner binds, and an owner removes its socket before it releases the
   lock), then binds, sets `0600`, listens, and reports ready. Binding before the lock serves
   clients from a daemon that is not the owner (control `listenBeforeLock`); not unlinking a
   stale file makes every later bind fail (the liveness control).
3. **Shutdown order: stop accepting, unlink, release.** On SIGTERM or an idle exit the daemon
   closes its listener (a connection still in the backlog is reset), cancels its queries, closes
   every connection and drops its overlays, stops its workers, removes the socket file, and only
   then releases the lock and exits 0. Releasing first lets the next owner bind before the old
   one's unlink, which then removes the new socket (plan §7.2's own negative control,
   `releaseBeforeUnlink`, which only `OwnerReachable` catches).
4. **Idle means no accepted connection.** The daemon cannot see its listener's backlog, so the
   idle exit can race a new connection; the reset proxy reconnects, spawning if it must, and a
   spawn that meets the stopping daemon's lock fails and is retried.
5. **Overlays belong to connections.** They are keyed by connection and die with it: when the
   daemon reads a closed connection's EOF, when it stops, and when it crashes. A new daemon
   starts with none; nothing is persisted for it to restore (control `crashKeepingOverlays`).
6. **Spawn: spawn lock, check again, spawn, handshake.** A proxy that finds no daemon takes the
   spawn lock (another proxy holding it: wait, then connect again), checks again under it (a
   racing proxy's daemon may listen by now), binds its notify socket, starts `jfl daemon`
   detached in its own session, and waits with a bounded budget. Ready, an error, a dead child
   or an expired wait all end the same way: release the spawn lock and connect again.
7. **Reconnect re-sends.** On every new connection the proxy re-sends its open documents and
   its client's unanswered request (or the client's retry of it, after the proxy answered the
   lost one with a retryable error; the model does not tell the two apart). A request is never
   silently dropped: a ready proxy's waiting client always has its request on the connection
   (`ReconnectRestores`, control `acceptWithoutResend`).

### IDX-12's obligations

These are the design above as IDX-12 must build it; the driver checks each against the model.

1. **The lock:** std `File::try_lock` on `<state>/daemon.lock`. Never unlink the lock file,
   never break a held lock. After the lock is taken, and again on every idle tick, check that the
   path still names the locked file (`stat` of the path against `fstat` of the descriptor, device
   and inode); if not (the file or the state directory was deleted from outside), shut down: a
   second daemon could lock a new file there.
2. **Close-on-exec for every descriptor**, not only the lock's: the lock, the listener, every
   accepted connection, and in the proxy the spawn lock it holds across the spawn. The only
   descriptor a child may inherit is the notify socket the proxy hands `jfl daemon`. A worker that
   outlives the daemon with the lock's descriptor keeps the flock held (control
   `exitLeakingLock`); one holding an inherited listener keeps a crashed daemon's socket
   accepting into a backlog nobody serves, which the model's crash (the kernel resets the
   backlog) assumes cannot happen. **Workers die with the daemon:** a parent-death signal
   (`PR_SET_PDEATHSIG`) or a watch on a pipe whose other end only the daemon holds, since ADR
   0021's amendment leaves each worker its own process group, out of reach of a crash.
3. **`setsid` first:** the daemon's first act, before the lock, so a signal to the launcher's
   group can only reach a daemon that holds nothing yet (the model counts either as a crash).
4. **The start order** of decision 2, and the error reported on the notify socket when the lock
   is held or the bind fails.
5. **The shutdown order** of decision 3, the unlink before the release; on SIGTERM and on the
   idle exit alike. The idle exit counts accepted connections only.
6. **A crash leaves the socket file and releases the lock.** The driver's crash is an abort of
   the in-process host that closes every descriptor (the lock, the listener, the connections)
   without running the shutdown: the socket file stays. So the host's cleanup must be explicit
   shutdown steps, never a `Drop` that an abort would run.
7. **Per-connection state:** overlays and requests keyed by connection, dropped when the daemon
   reads EOF, on stop and on crash; nothing persisted across a restart.
8. **The proxy:** the spawn lock, the second check under it, the notify handshake with a bounded
   wait; on a refused connect, a reset backlog connection, a failed or timed-out spawn: connect
   again. On a new connection: the version handshake, then the open documents, then the
   unanswered request. **The proxy's waits have deadlines:** for the accept and handshake, and
   for each reply. An expired one is a reset (close the connection, answer the client's request
   with a retryable error, connect again), so a daemon that is alive but hung cannot hold a
   client forever; the model's proxies wait without one (`connecting`, `ready`), as `spawnTimeout`
   is the only deadline it models.
9. **The driver can step the host.** Each model action is one step the driver applies: accept
   one queued connection, answer one request, read one EOF, take one shutdown step. The host
   needs a test hook that runs these one at a time (a manual accept loop), or the observation
   cannot be compared after every step.
10. **The observation is exact all the way down**, as for `views` (`deny_unknown_fields` and a
   derived `PartialEq` on every nested type).

### Not modelled

- **The version handshake's mismatch** (ADR 0021: the proxy asks the old daemon to stop and
  respawns): to the model it is a SIGTERM followed by a spawn.
- **Socket and directory modes, the peer-uid check, the socket path's length limit** (plan
  §5.3): IDX-12's integration tests.
- **The socket file removed by someone else** while its daemon serves, such as a tmp cleaner
  under `/tmp/jfl-<euid>/` (plan §5.3's fallback path) or a user's `rm`. No proxy can reach the
  daemon then, and a spawned daemon fails the lock, so no client is served until that daemon
  exits; it never idles out while it has connections. IDX-12 should have the daemon notice that
  its path no longer names its socket (a stat on its idle tick, comparing the inode) and stop;
  modelling that is a change to this model first (ADR 0007).
- **The lock file or the state directory deleted from outside** while a daemon holds the lock:
  a second daemon then locks a new file, and two own the directory. The model has this only as a
  code bug (`lockBreakingStale`); obligation 1's path check makes the old daemon stop.
- **Requests' contents, cancellation and deadlines:** one request per proxy, answered or not;
  obligation 8's proxy deadlines are not modelled.
- **Workers:** only through the lock-leak control.

### Stimuli: the driver's actions

`step` picks one of three equally likely groups, `clientStep`, `proxyStep` and `daemonStep`,
then an enabled action in it; the faults (`faultStep`) are one branch of `daemonStep`, so they are
rarer than the steps that recover from them. `--mbt` records the innermost action taken, so the
groups never appear in a trace. Each parameter is a nondeterministic pick named as the parameter
(`proxy`, `daemon`, `conn`, `doc`); a pick the action did not use is `None`.

| Action | Picks | What the driver does to the real code |
|:--|:--|:--|
| `init`, `initClean`, `initStale` | `stale` | A fresh tempdir state directory; with `stale`, a socket file nobody listens on (bind a listener and drop it) and the lock file, unlocked. The scenario traces start from `initClean` or `initStale`, `init` with the pick fixed; the driver treats all three as `init`. |
| `request` | `proxy` | The client asks; a ready proxy sends the request. |
| `didOpen`, `didClose` | `proxy`, `doc` | The client opens or closes a document; a ready proxy forwards it. |
| `quit` | `proxy` | The client session ends and a new one takes the slot: the proxy closes its connection and releases the spawn lock if it holds it. |
| `connect` | `proxy` | The proxy connects to the socket path: queued in the backlog of a listening daemon, or refused. |
| `takeSpawnLock` | `proxy` | The proxy tries the spawn lock; held by the other: it will connect again. |
| `spawn` | `proxy` | Under the spawn lock, the second check: connect again if a daemon listens, else spawn a daemon in the first free slot. |
| `handshake` | `proxy` | The notify socket answers (ready, or the error of a failed or dead daemon); the proxy releases the spawn lock. |
| `spawnTimeout` | `proxy` | The bounded wait expires while the daemon still starts. |
| `notice` | `proxy` | The proxy reads its connection's EOF. |
| `lock` | `daemon` | The daemon tries the lock; held: it reports the error and exits 1. |
| `unlinkStale` | `daemon` | The owner removes the socket file it found. |
| `listen` | `daemon` | Bind, `0600`, listen, report ready. |
| `accept` | `daemon`, `conn` | Accept the queued connection: the version handshake, then the re-sent documents and request. |
| `answer` | `daemon`, `conn` | Answer the request on the connection. |
| `reap` | `daemon`, `conn` | Read the EOF of a connection its proxy closed; drop its overlays. |
| `sigterm`, `idleExit` | `daemon` | The shutdown's first step: stop accepting, close every connection. The idle exit only with no accepted connection. |
| `unlink` | `daemon` | The shutdown removes the socket file. |
| `exit` | `daemon` | The shutdown releases the lock; the daemon exits 0. |
| `crash` | `daemon` | Abort the daemon (obligation 6). |

Connection and daemon slots are taken first free in order (`d1`, `d2`, `d3`; `c1`, `c2`, `c3`), so
an action has one outcome; the driver numbers its processes and connections the same way.

### Observation: what the driver produces after every step

The model has seven variables:

| Variable | Type | Observed from |
|:--|:--|:--|
| `daemons` | slot → `{ phase, lock }` | Each slot's process: `phase` one of `none`, `started`, `locked`, `serving`, `stopping` (stopped accepting), `unlinked` (socket removed, lock held), `exited`, `failed` (exited 1: the lock was held, or a control's failed bind), `crashed`; `lock` the inode of `daemon.lock` it holds an flock on, as a number (below), or 0. |
| `lockFile` | int | The inode at `<state>/daemon.lock`, 0 while there is none. The driver numbers inodes 1, 2, … in the order it first sees them. |
| `sock` | `{ present, owner }` | Whether `<state>/sock` exists, and the slot whose bound socket has its inode (`""` for the initial stale file). |
| `spawnLock` | string | The proxy holding `spawn.lock`, or `""`. |
| `conns` | slot → `{ state, proxy, daemon, overlays, inflight }` | `state` one of `free`, `backlog` (connected, not accepted), `open`, `peerClosed` (the proxy closed it, the daemon has not read EOF), `closed` (the daemon closed it, the proxy has not noticed); the daemon's overlays (document names) and whether it holds an unanswered request. |
| `t` | int | The steps taken since `init`: the driver's step index. It is the liveness simulation's clock (`Progress`, below), in the model because Apalache cannot read a file holding a second module that imports the first. |
| `proxies` | proxy → `{ phase, conn, daemon, docs, want }` | `phase` one of `disconnected`, `noDaemon`, `spawning`, `awaiting`, `connecting`, `ready`; its connection slot and the daemon slot it waits on (or `""`); the client's open documents and whether the client waits for an answer. |

The `released` phase appears only in the `releaseBeforeUnlink` control.

### Invariants

`val allInvariants` is the conjunction of these six. Apalache and the simulation check it.

| Invariant | States |
|:--|:--|
| `AtMostOneOwner` | At most one process holds an flock on `daemon.lock`, on any inode the path ever named. |
| `ServedOnlyByOwner` | Every connection a daemon has accepted and not closed belongs to the owner, and the owner is serving. |
| `NoGhostBuffers` | Every overlay belongs to a live connection (open, or closed by its proxy and not yet read to EOF) of the owner, and an open connection holds only documents its proxy has open. |
| `ShutdownReleases` | A daemon that exited holds no flock and left no socket file of its own. |
| `OwnerReachable` | A serving owner's socket is the file at the path. |
| `ReconnectRestores` | A ready proxy's open connection holds every document its client has open, and its client's unanswered request. |

The first four are plan §7.2's. Plan §7.2's own negative control, releasing the lock before
removing the socket, violates none of them: the old daemon has stopped serving before it
releases, and when it exits it leaves no socket of its own, having removed the new owner's. What
breaks is that the new owner can no longer be reached: `OwnerReachable`. `ReconnectRestores` is
plan §5.2's reconnect promise (re-sent documents, no request silently dropped), the other half of
"no ghost buffers": no buffer the client closed, none it still has open missing.

### Liveness: `EventuallyServed`

Plan §7.2: a proxy that keeps retrying is eventually served. `temporal EventuallyServed` states
it: under weak fairness of every proxy, daemon and connection move (`fairness`), and once faults
stop (`faultsStop`: crashes, SIGTERM, idle exits and expired waits happen finitely often), a
client waiting for an answer eventually gets one. The clients stay free: they ask, open, close
and quit as they like, but one waiting for an answer does not quit.

**Apalache cannot check it at a useful bound.** What the property needs, and what was measured:

- Apalache has no `ENABLED` and rejects Quint's `weakFair` ("Handling fairness is not supported
  yet"). Each fairness condition is written `always(eventually(not(enabled) or action))` with the
  action's own guard definition, which Apalache accepts (the action formula is checked on the
  loop's transitions).
- It cannot quantify over a temporal formula (a variable bound by `forall` outside an `always` is
  left unassigned), so the conditions are spelled out per proxy, daemon and connection. One per
  proxy, daemon and connection suffices (each enables at most one of its moves at a time), eight
  in all instead of 52.
- Its temporal pass cannot inline a fold in the model (an internal type error), so the first free
  slot is spelled out instead of folded.
- With all that, the check runs, but its cost rules it out: on the model without the clock `t`,
  length 3 took 735 s and 4.5 GB, more than length 8 of the safety check (and with all 52
  conditions spelled out singly, length 3 had not finished after 10 minutes). The liveness
  control's counterexample, a lasso from a stale start through a spawn whose bind fails back to
  the same state, needs length 10; length 8 of the safety check alone takes about 8 minutes.
- Now that the model carries the clock `t`, no state repeats, so Apalache would find no lasso at
  any bound: a temporal check of this model passes vacuously, and `formal-verify` does not run
  one. `EventuallyServed` stays in the model as the statement of what `Progress` checks.

**So `EventuallyServed` is covered by what it reduces to**, with the model's `progressStep` and
`Progress`. Under that fairness, once faults and clients stop, a waiting client goes unserved only
if the system can stop with it still waiting (a deadlock) or move forever without serving it (a
livelock). A run takes 30 random steps of `step` (faults and clients included), then only system
moves, each picked at random among the enabled ones; `Progress` fails unless, after 60 more
steps, nothing is enabled and no client waits.

It stops the clients too, which the temporal property does not. There, another client that keeps
connecting and quitting holds at most two connection slots (one open, one awaiting its EOF,
which the daemon reads as a fair move), so a third comes free for the waiting proxy; but while
the other client churns, the waiting proxy's `connect` is enabled only now and then, which weak
fairness does not force (that takes strong fairness). This starvation is an artifact of the
model's bounded slots: the real system has no slot bound, so its `connect` is never disabled
that way. That step of the argument is by hand.

**What runs where:**

- **`formal-verify daemon`** (weekly in `formal.yml`, and by hand) runs it after the safety
  simulation, from the model's `progress` entry in `formal/models.toml`: 3000 samples of 92 steps
  of `progressStep` keeping `allProgress`, which must find no violation; then the same of the
  liveness control `progressStepStaleBlocks` (`listenWithoutUnlinkingStale` in place of
  `unlinkStale` and `listen`), which must report a violation, or the stage fails. A step or an
  invariant that does not exist fails the run (closed); `jfl-conformance`'s
  `should_declare_the_liveness_steps_when_a_model_has_a_progress_entry` checks in every `check`
  that the three names are declared.
- **Every `check`** runs the Quint test `listenWithoutUnlinkingStaleNeverServesTest`, the
  control's cycle as named actions, and xtask's `should_see_a_violation_only_when_quint_reports_one`
  (what counts as the control's violation).
- **By hand, once, at a larger budget:** 20000 samples at seed `0x5d` found no violation (608 s);
  the control fails in its first samples (under 1 s). The commands:

```
quint run --backend typescript --main daemon --step progressStep --invariant allProgress \
  --max-steps 92 --max-samples 20000 formal/daemon.qnt
quint run --backend typescript --main daemon --step progressStepStaleBlocks --invariant Progress \
  --max-steps 92 --max-samples 2000 formal/daemon.qnt
```

IDX-12's `proptest-state-machine` test should assert the same reduction against the host: after
its last fault, a waiting client is served within a bounded number of steps.

### Scenarios and regression traces

Each scenario is a scripted step whose guards enable exactly one action in each state, from
`initClean` or `initStale`. Each, run at six seeds from `0x1` to `0xdead`, gave one action
sequence, and stops after its last step. Its regression trace, at seed `0x3`, is in
`formal/traces/regression/manifest.toml`, and its test takes the same steps as named actions.

| Scenario (`--step`) | Steps | What it shows |
|:--|:--|:--|
| `twoProxiesSpawnAtOnce` | 19 | Both proxies find no daemon. `p1` takes the spawn lock and spawns `d1`; `p2` is refused the spawn lock, connects again before `d1` listens, and when it takes the lock its second check finds `d1`, so it spawns nothing. One daemon serves both. |
| `crashWithOpenDocuments` | 26 | `d1` crashes while `p1` has `a` open and a request in flight, and `p2`'s closed connection still holds `b` awaiting its EOF. The stale socket refuses `p1`; `p1` spawns `d2`, which unlinks the stale file and binds; `p1` re-sends `a` and its request; `d2` holds no overlay of either dead connection, and answers. |
| `idleExitRacesConnect` | 32 | `d1`'s last connection closes; as its idle period ends, `p2` connects and sits in the backlog, and the idle exit resets it. `p2` is refused while `d1` stops, its spawned `d2` fails the lock `d1` still holds, and once `d1` has unlinked its socket and released the lock, `p2`'s next spawn (slot `d1` again) serves it. |
| `sigtermAfterStaleStart` | 14 | From a stale start: `p1` is refused and spawns; the daemon takes the existing lock file, unlinks the stale socket and binds. SIGTERM stops it with `p1` connected: `p1` reads EOF, the daemon unlinks and releases, nothing is left, and `p1` finds no socket. |

More tests without a trace: `spawnLockRefusedTest`, `idleExitWaitsForOpenConnectionsTest` (the
idle exit is refused while a connection is open), `secondDaemonFailsTheLockTest` (a daemon spawned
after a timed-out wait fails the lock, and its spawner's handshake reports it),
`didCloseDropsTheOverlayTest`, and `unlinkBeforeReleaseKeepsOwnerReachableTest` (the real
shutdown order on the control's interleaving).

### Broken-model controls

Each control is an action variant with a Quint test that expects its invariant to be false:

| Control | Fault | Violates |
|:--|:--|:--|
| `lockBreakingStale` | On a held lock, unlinks `daemon.lock` and locks a new file. | `AtMostOneOwner` |
| `listenBeforeLock` | Binds and serves before taking the lock. | `ServedOnlyByOwner` |
| `crashKeepingOverlays` | A dead daemon's overlays survive it, for the next daemon to restore. | `NoGhostBuffers` |
| `reapKeepingOverlays` | Reads a closed connection's EOF but keeps its overlays. | `NoGhostBuffers` |
| `exitWithoutUnlink` | Exits from the shutdown without removing the socket. | `ShutdownReleases` |
| `exitLeakingLock` | Exits while a surviving child holds the lock's descriptor. | `ShutdownReleases` |
| `releaseBeforeUnlink`, `unlinkAfterRelease` | Plan §7.2's control: releases the lock, then removes the socket. | `OwnerReachable` |
| `acceptWithoutResend` | Accepts a reconnecting proxy without its documents and request. | `ReconnectRestores` |
| `listenWithoutUnlinkingStale` | Binds without removing a stale socket file: the bind fails. | `EventuallyServed` (`Progress`) |

Plan §7.2 names IDX-12's own negative control: a host that releases the lock before removing the
socket. It must diverge in a trace with a shutdown (`idleExitRacesConnect` and
`sigtermAfterStaleStart` have one, and `conformance-replay`'s generated traces must include one),
at the first step after which the model's daemon still holds its lock and the faulty host has
released it.

### Budget

Measured in the sandbox: 4 CPUs, Quint 0.33.1 on the TypeScript backend, Apalache 0.62.4 on the
Temurin JDK 21.

| Check | Budget | Time |
|:--|:--|:--|
| `formal-test` (18 daemon tests) | every `check` | under a second |
| `formal-freshness`: four traces regenerated | every `check` | about 2 s per trace |
| `conformance-replay` | 10 traces of `sim_steps` (40), generated twice, every `check` | a few seconds. Read, not replayed, until IDX-12. |
| `formal-verify daemon`: Apalache | `steps = 8` | 462 s |
| `formal-verify daemon`: simulation | 20000 samples of `sim_steps` (40) | 194 s (170 s without the other lane running) |
| `formal-verify daemon`: liveness and its control | 3000 samples of 92 steps, each | 89 s, then the control fails within seconds |
| `formal-verify daemon` in all | weekly in `formal.yml` (60-minute job) | 762 s, with another lane building beside it; perhaps twice that on a CI runner |

**Apalache's time grows steeply with the bound:** 24 s at length 4, 63 s at 6 and 467 s at 8;
length 10 did not finish in 10 minutes. Length 8 covers every interleaving of a start (from a
clean or a stale state directory, two proxies racing to connect, take the spawn lock and spawn)
up to the first daemon's bind and the first connect; a crash and a respawn take about 20 steps
and are the simulation's. **The depth rests on the simulation**, run weekly in `formal-verify`
and at a fresh seed in every `conformance-replay`. Each of these witness states is found by
`quint run --invariant 'not(<witness>)' --max-steps 30 --max-samples 20000` at seed `0x77`:

| Witness (`val`) | State | Found after |
|:--|:--|:--|
| `witnessLockRefused` | a spawned daemon failed the lock while another owns it | under 1 s |
| `witnessBacklogReset` | a backlog connection reset by a stopping daemon | under 1 s |
| `witnessSecondCheck` | a proxy under the spawn lock finds a daemon listening | under 1 s |
| `witnessStaleFromCrash` | an owner about to unlink a socket left by a crash in the run | under 1 s |
| `witnessEofPending` | a closed connection's overlays await the daemon's read of its EOF | 1 s |
| `witnessServingAfterShutdown` | a daemon serves after another shut down cleanly | 4 s |
| `witnessRespawnHoldsOverlays` | a connection to a daemon started after a crash holds overlays | 15 s (4 s at 40 steps) |
| `witnessAnsweredAfterCrash` | a client answered, documents open, after a crash | 50 s, about 9000 samples (21 s at 40 steps) |

The rarest is an answer after a crash, which is why `sim_steps` is 40, not 30. Raise either
number only with a measurement, while `formal.yml`'s 60-minute job stays well under its limit.
