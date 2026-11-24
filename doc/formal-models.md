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

| Model | What it fixes | Model task | Driver task, over |
|:--|:--|:--|:--|
| [`views`](#views-views-generations-freshness-and-completeness) | generations, freshness, completeness | FORMAL-03 | IDX-09, `jfl-application`'s view registry |
| [`daemon`](#daemon-the-lock-the-socket-spawn-reconnect-and-overlays) | the single owner, spawn, reconnect, overlays | FORMAL-04 | IDX-12, `jfl-composition`'s daemon host and launcher |
| [`diagnostics`](#diagnostics-versions-epochs-and-publication) | which compile results are published, and with which version | FORMAL-05 | LSP-04, `jfl-application`'s diagnostics coordinator |

Each driver task's card names its section here; the section's obligations are what the driver's
review checks.

## `views`: views, generations, freshness and completeness

`formal/views.qnt` (FORMAL-03; plan §4, §7.1, §2.1; ADR 0014, ADR 0017, ADR 0037). The driver
is IDX-09's (`crates/jfl-conformance/src/views.rs`), over `jfl-application`'s view registry
(`crates/jfl-application/src/views.rs`, which does no I/O: each method is one transition below)
with a scripted tree, watcher and summary store beside the driver.

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

### Design decisions the model fixes (ADR 0037, Accepted)

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

### Where the shell meets each obligation

The view service (`crates/jfl-application/src/view_service.rs`, IDX-09 slice 2) does the I/O of
each transition in the model's order; its component tests (`tests/view_service.rs`) pin it over
the testkit's fakes, and `tests/views_machine.rs` runs the registry against a reference machine.

- **1, watch then scan:** a view opens by watch, then the clock, `HEAD`'s stat set and the listing
  (`should_hear_of_a_file_written_during_the_open_scan_when_the_watch_came_first`); a lost watch
  is re-established before the rescan that may certify the view
  (`should_watch_again_and_rescan_when_the_watch_degrades`).
- **2, new directories:** a hint naming anything but a `.java` file the view holds (a directory, a
  new file) asks for a rescan, which lists the view (`should_rescan_when_a_hint_names_no_java_file`);
  a hint under a nested root is dropped, and a rescan leaves out what its listing does not admit,
  so a parent never indexes a nested view's file (the model's assumption: `should_keep_*_out_of_*`).
- **3, losses:** a `Rescan` whose reasons degrade the watch is `watchLost`, any other an overflow
  of every view; either loses the view's queued cookies.
- **4, `statfs`:** only an allowlist of local filesystems is watched
  (`should_never_claim_complete_when_the_filesystem_cannot_be_watched`), and the walk does not
  cross into another device, which is a gap
  (`should_answer_partly_indexed_when_a_directory_is_another_filesystem`).
- **Atomic publish** (plan §7.1) holds by construction in the shell: each generation entry carries
  its summary, so the registry's `NotStored` guard is never what stops a shell build (the store is
  only the reopen cache); the guard is exercised by the `views` driver and
  `tests/views.rs::should_refuse_publish_when_a_named_summary_is_not_stored`. The model's `store`
  is therefore the generation's own summaries, and the disk store is a cache outside the model:
  the shell writes it after the publish (`ViewService::write_back`, IDX-09 slice 3), which
  changes no transition and no guard.
- **The open in two steps** (IDX-09 slice 3, plan §1.7's first useful answer). The model starts
  with every view published (`init`); the shell's open stands for it. A view of more than
  `ServiceConfig::open_whole` files reads only the request's directory and publishes every other
  file as `Summary::Pending`. In model terms a pending entry is the model's entry at the listed
  version (the listing's stat) with a racy summary (`recent: true`, reachable by a `summarise`
  within the file's write tick, so no later stat proves it fresh), plus a **coverage gap**, which
  the model does not contain (as with slice 2's `Summary::Gap`: coverage is outside the views
  model). The watch was up before the listing, so any change after it raises a hint, as the model
  assumes.
  - **The guard is the shell's coverage rule, not the registry.** The registry alone would call
    such a generation complete once its cookie is back. What keeps it incomplete is `count`
    (`view_service.rs`), which counts a pending file among the files and never among the
    indexed, and `ViewService::request`'s `indexed < files` rule, which adds
    `ModulePartlyIndexed` to every answer from that generation. It is pinned by
    `should_answer_partly_indexed_and_then_complete_when_the_open_defers_the_rest` and
    `should_read_only_the_requested_file_when_a_request_finds_it_pending`, both of which fail
    when `count` takes a pending file as indexed.
  - **AtomicPublish** ("every summary a published generation names is stored") holds in the shell
    only when a declared gap (`Gap`, `Pending`) counts as stored: generation 1 of a large view
    names files with no readable summary. It is safe for the same reason `Gap` is, since a gap is
    never read as an empty summary and always makes the answer partly indexed.
  - **Every exit from Pending reads the file or removes it**, through the model's transitions: a
    request's stat finds it changed (racy), so it is dirty (`requestStart`) and a build reads just
    it; a hint for it is `deliver` of a file event; a rescan re-reads every entry a stat cannot
    prove (`reconcileStart`), or leaves out what its listing lacks. `ViewService::settle` is the
    exception: its file events are **made by the shell, not raised by a watcher**, so they are not
    the model's `deliver`, which takes a queued event. They are a harmless over-approximation
    (an extra dirty path is re-read), after which `reconcilePart` (or `reconcileStart` on a
    rescan), `summarise` and `publish` are the model's.
  - **The settle in slices** (IDX-09d): the first slice delivers every deferred file, then each
    slice is the model's `reconcilePart` of at most `SETTLE_SLICE` of them (512 in the daemon),
    its reads and its publish, one daemon background step each. Until the last slice publishes,
    the deferred files are delivered and unpublished, so the model itself makes every answer
    incomplete (`completeAt` needs `dirty` empty), beside the coverage rule. A request takes
    the delivered files that are not deferred and the ones a waiting request needs (its own:
    `reconcilePart`'s guard), never the rest of the view; a rescan takes them all in its one
    build (`reconcileStart`, which is whole), and so does a request while one is due: a slice
    bounds the hold only while no rescan is due (IDX-09e has the triggers and the model change).
    Each publish shares the entries its build did not touch with the generation before
    (`SharedIndex`), and the registry's atomic-publish guard checks the touched entries only:
    the others passed it when they were published, and the model's `store` only grows. Pinned by
    `should_publish_a_generation_per_slice_and_answer_between_them_when_settled_in_slices` and,
    in the daemon, `crates/jfl-composition/tests/daemon.rs::should_answer_a_request_between_the_slices_of_a_settle_when_the_view_is_large`.
  - Slice 3 added no transition and changed no invariant (review of 2026-11-19: writing the
    store after the publish is a sound refinement of `store`, and Pending is a coverage gap);
    IDX-09d added `reconcilePart` for the slices. Pinned by `tests/view_service.rs`: the two tests
    above and `should_settle_from_the_disk_when_a_pending_file_changes_before_settle`,
    `should_drop_a_pending_file_from_the_count_when_it_is_deleted_before_settle` and
    `should_read_every_pending_file_when_sync_runs_before_settle`. Letting a stat prove a pending
    file fresh fails two of them.
- **6, the cookie:** written into the view's root through its own watch, waited for with a
  deadline, at most two (`should_answer_from_the_new_content_when_a_write_lands_before_the_request`).
- **7, the re-stat at the pin:** `HEAD`, the ref it names and `packed-refs`, and the index, from
  `RepoProbe::snapshot` (`should_rebuild_when_a_commit_moves_the_branch_and_head_stays`).

### Not modelled yet

- **Overlays** (plan §4.4: an open buffer applies only while it is newer than the disk, and disk
  wins over an older overlay).
- **Worktrees added or removed** under a running parent view.
- **Lazily opened views.** They are covered by obligation 1 (watch, then scan).
- **File creation.** `PATHS` is fixed, so no file appears that the views did not index at `init`.
  A file created while no event can be raised (the watch lost, an overflow) is seen only by a
  rescan's listing: the registry's `reconcile_start` takes the listing, reads every listed path
  the index lacks, and certifies the view only when it was given one. Adding created paths to the
  model would add path and listing state to every record, and Apalache's time grows steeply with
  the state (858 s at length 5 already, in a 60-minute job), so the case is pinned by component
  tests instead: `tests/views.rs::should_read_a_file_created_while_the_watch_was_lost_when_the_rescan_lists_it`
  and `should_stay_uncertain_when_a_rescan_has_no_listing` in `crates/jfl-application`.
- **A cookie raising several events.** A real cookie write raises several (`IN_CREATE`,
  `IN_CLOSE_WRITE`); the model's is one event. Each cookie event carries the stamp its cookie was
  written at (the model's `cookieSeq`, the driver's event `seq`), and the registry ignores one
  that is not the request's latest queued cookie:
  `tests/views.rs::should_wait_for_the_second_cookie_when_a_stale_first_cookie_event_arrives`.
- **Deadlines** (plan §2.7), including the bound on a request's wait for its cookie: the model's
  `requestPin` waits for a queued cookie; IDX-09's view service calls `cookie_lost` when the
  deadline passes first, the step the model's `dropEvent` of a cookie takes, so the answer is
  incomplete (`crates/jfl-application/tests/view_service.rs::should_answer_incomplete_when_the_cookie_misses_its_deadline`).
  A model action for it is left on IDX-09's card.

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
| `reconcilePart` | `view`, `take` | The registry starts a build of `take`, a part of the delivered events (IDX-09d: a settle's slice, or a request's own files during one); the rest stay delivered. Never while a rescan is due, and only with every path a waiting request needs (`needed`). `step` picks `take` as one delivered path, alone or with every needed path (not any subset: with every subset the simulation never took a part while a request waited). |
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
| `settleInSlices` | 26 | IDX-09d. Two files are delivered; a part takes `m/A` and publishes generation 2. A request on `m/A` is answered from it between the slices, incomplete while `m/B` waits. `m/A` changes again; a request on `m/B` waits for the next part, which must take `m/B` (here beside `m/A`) (`partLeavingOutANeededPathIsRefusedTest`, `partTakingTheNeededPathIsTakenTest`); after its second cookie the answer is complete at generation 3. A part is refused while a rescan is due (`partDuringARescanIsRefusedTest`, `rescanTakesWhatAPartWouldTest`). |

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
| `reconcilePartIgnoringRequests` | A part leaves out the file a waiting request's stat saw change (IDX-09d): `partIgnoringRequestsViolatesFreshAfterStatTest`. The simulation finds it too, put in `reconcilePart`'s place in `daemonStep`: `quint run --backend typescript --main views --step step --invariant FreshAfterStat --max-steps 30 --max-samples 20000 --seed <s>` found the violation within 12–13 s (about 1,800 samples) at each of the seeds 0x1 to 0x5 (2026-11-24); with the part picked from every subset of the delivered paths it found none in 20,000 (the IDX-09d review). Apalache at length 5 cannot reach it (the shortest violation is about 10 steps). | `FreshAfterStat` |

Plan §7.1 names IDX-09's own negative control: publishing before the last summary is stored. The
driver has three (`crates/jfl-conformance/src/views.rs`), each diverging where its fault first
shows:

- **The registered control** shows the registry a store that lags the observed one by its last
  write (a write acknowledged before it can be read back). The registry's atomic-publish guard
  refuses the next `publish` that names that summary (`NotStored`), so it diverges at the model's
  `publish` step: step 6 of `views-checkout-during-reconcile`.
- **The early-publish control** publishes each build as soon as it starts, before its summaries
  are stored. It diverges at `reconcileStart` (with a path pending the guard refuses
  `PendingSummaries`; with none the early generation differs from the model's): step 3 of the
  same trace.
- **The part-without-needed control** (IDX-09d) leaves out of each part the paths the waiting
  requests need, as a shell slicing a settle without them would. The registry's guard refuses
  it (`NeededNotTaken`): step 18 of `views-settle-in-slices`, whose part holds another path
  beside the needed one, so the refusal is the guard's and not an empty part's (`tests/replay.rs::should_diverge_at_the_part_when_the_views_control_leaves_out_a_needed_path`).

`conformance-replay` requires the registered control to diverge on the generated batch or the
committed regression traces, so the requirement holds at every seed: in a batch of ten 30-step
traces the lagging store shows only when a `summarise` is followed by a `publish`, which some
batch will lack.

### Budget

Measured in the sandbox: 4 CPUs, Quint 0.33.1 on the TypeScript backend, Apalache 0.62.4 on the
Temurin JDK 21. The registry holds two numbers per model: `steps`, Apalache's bound, and
`sim_steps`, the depth of the simulation and of `conformance-replay`'s generated traces.

| Check | Budget | Time |
|:--|:--|:--|
| `formal-typecheck` and `formal-test` (35 views tests, 1 smoke) | every `check` | about 6 s |
| `formal-freshness`: nine traces regenerated | every `check` | about 2 s per trace |
| `conformance-replay` | 10 traces of `sim_steps` (30), generated twice, every `check` | a few seconds; the views driver replays them and the eight regression traces, and its control diverges (IDX-09). |
| `formal-verify views`: Apalache | `steps = 5` | 858 s; 1169 s with IDX-09d's `reconcilePart` and its biased pick (2026-11-24, the other lane's work and this lane's benches running; 1093 s with the first, unbiased pick) |
| `formal-verify views`: simulation | 20000 samples of `sim_steps` (30) | 172 s; 185 s with `reconcilePart` |
| `formal-verify views` in all | weekly in `formal.yml` (60-minute job) | 1048 s; 1369 s with `reconcilePart` (23 min); perhaps twice that on a CI runner, about 46 min |

**The margin is about 14 minutes** of the 60-minute job at that doubling (IDX-09d's review). Another
transition of `reconcilePart`'s cost at `steps = 5` would not fit: measure it on a CI runner first,
or split the job (Apalache and the simulation in separate jobs), before adding one (IDX-09e).

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
IDX-12's (`crates/jfl-conformance/src/daemon.rs`), over `jfl-composition`'s daemon host
(`daemon::host`, stepped by `daemon::stepped`) and proxies (`daemon::proxy`) in process, with real
Unix sockets, `flock`s and files in a fresh state directory under the temporary directory.

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
8. **A swept socket stops its daemon** (IDX-12's decision, 2026-11-19). The socket file removed
   by someone else while its daemon serves (`sweep`: a tmp cleaner on plan §5.3's `/tmp`
   fallback, a user's `rm`) leaves the daemon listening on an inode no proxy can reach, while a
   spawned daemon fails the lock it still holds: no new client is served until it exits, and it
   never idles out while it has connections. So the daemon compares, on its tick, the file at the
   path with the one it bound, and on a difference shuts down as on SIGTERM (`noticeSwept`, a
   daemon move under fairness); its `unlink` then finds nothing to remove. `OwnerReachable`
   allows the swept state (`sock.owner` is the daemon, `present` false), the one fault no code can
   prevent; liveness requires the notice (the control `progressStepIgnoringSweep`, and the Quint
   test `sweptUnnoticedNeverServesTest`, its cycle as named actions).

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
   unanswered request. A re-sent document keeps the client's LSP version and text, never
   renumbered (the `diagnostics` model's obligation 11). **The proxy's waits have deadlines:** for the accept and handshake, and
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

**As built (IDX-12):** 1, `jfl_adapter_ipc::Flock` and `Host::tick` (checked after the lock and on
every tick of the run loop); 2, std and Tokio open every descriptor close-on-exec and nothing
clears it (`jfl-adapter-ipc/tests/endpoint.rs`), and the notify socket is a path the daemon sends
to, not an inherited descriptor, so a child inherits nothing; the daemon starts no worker yet, so
"workers die with the daemon" is the first worker's task (SEM-04's javac pool); 3, `run::run`
calls `setsid` first; 4 and 5, `run::run` and `Host::shutdown`; 6 and 7, `Host::crash` and the
per-connection `Conn`; 8, `daemon::proxy` (every wait a `poll(2)` bounded by the time left, an expiry a reset; a
version mismatch stops the old daemon with `jfl/stop` and spawns, ADR 0021); 9, `daemon::stepped`; 10, the driver's observation types.

### Not modelled

- **The version handshake's mismatch** (ADR 0021: the proxy asks the old daemon to stop and
  respawns): to the model it is a SIGTERM followed by a spawn.
- **Socket and directory modes, the peer-uid check, the socket path's length limit** (plan
  §5.3): IDX-12's integration tests. Nor the second check's probe: it connects and closes before
  any handshake, so it is never a connection slot; the host drops it from the backlog on its way
  to the connection `accept` names (on Linux it reads its EOF, on macOS its credentials can no
  longer be read; FND-16).
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
| `crash` | `daemon` | Abort the daemon (obligation 6). A swept socket's binding dies with it. |
| `sweep` | | Remove the socket file of the serving owner (decision 8). |
| `noticeSwept` | `daemon` | The tick finds the path no longer names the bound socket: the shutdown's first step. |

Connection and daemon slots are taken first free in order (`d1`, `d2`, `d3`; `c1`, `c2`, `c3`), so
an action has one outcome; the driver numbers its processes and connections the same way.

### Observation: what the driver produces after every step

The model has seven variables:

| Variable | Type | Observed from |
|:--|:--|:--|
| `daemons` | slot → `{ phase, lock }` | Each slot's process: `phase` one of `none`, `started`, `locked`, `serving`, `stopping` (stopped accepting), `unlinked` (socket removed, lock held), `exited`, `failed` (exited 1: the lock was held, or a control's failed bind), `crashed`; `lock` the inode of `daemon.lock` it holds an flock on, as a number (below), or 0. |
| `lockFile` | int | The inode at `<state>/daemon.lock`, 0 while there is none. The driver numbers inodes 1, 2, … in the order it first sees them. |
| `sock` | `{ present, owner }` | Whether `<state>/sock` exists, and the slot whose bound socket has its inode (`""` for the initial stale file); with no file, the live slot that still holds a bound socket the path no longer names (swept), else `""`. |
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
| `OwnerReachable` | A serving owner's socket is the file at the path, or was swept from under it (decision 8). |
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
| `sweptSocketStops` | 28 | IDX-12's: `d1`'s socket file is swept while `p1` is connected. `p2` finds no socket and spawns `d2`, which fails the lock `d1` still holds; `d1`'s tick notices the sweep and it shuts down (`p1` reads EOF, the unlink finds nothing, the release), and `p2`'s next spawn serves it. Guarded by the step count `t`. |

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
| `progressStepIgnoringSweep` | A daemon never notices its socket swept: every spawn fails its lock. | `EventuallyServed` (`Progress`; by hand, and `sweptUnnoticedNeverServesTest` in every `check`) |

Plan §7.2 names IDX-12's own negative control: a host that releases the lock before removing the
socket. It must diverge in a trace with a shutdown (`idleExitRacesConnect` and
`sigtermAfterStaleStart` have one, and `conformance-replay`'s generated traces must include one),
at the first step after which the model's daemon still holds its lock and the faulty host has
released it. **As built (IDX-12):** the control's host releases the lock in the shutdown's first
step (`sigterm`, `idleExit` or `noticeSwept`), so it diverges there, deterministically, on each
committed trace with a shutdown: step 10 of `sigtermAfterStaleStart`, step 15 of
`idleExitRacesConnect`, step 16 of `sweptSocketStops` (the model's `lock: 1` against the host's
`0`); `conformance-replay` tries the committed traces after the generated batch
(`cli::control_diverges`), so the gate never depends on the seed drawing a shutdown.
`tests/replay.rs::should_diverge_at_the_shutdown_when_the_daemon_control_releases_first` pins the
three.

**The driver as built (IDX-12).** A spawn starts an in-process host in the model's first free
slot, reporting on the proxy's notify path; a crash is the host's abort (every descriptor
dropped, no shutdown step run); the initial stale state is what a throwaway host leaves when it
locks, binds and crashes. The model's `accept` names one connection of the backlog: the host
takes connections from the kernel's backlog in order until it holds the one whose handshake
request carries that proxy's id, keeping the others for later. The proxy's half of `accept` (the
handshake's answer read, the documents and request re-sent) and the daemon's reading of each frame
a client sends are part of the same model step, since the model's overlays and requests change
with it. What the replay found: a notice from the daemon of an earlier, abandoned spawn reached the
next spawner over the shared notify path (generated `spawnTimeout` steps), so each spawn now has
its own path (`<state>/n-<12 hex digits>`: the process's pid and its spawn count; the counter is
the process's, since two proxies of one process, as the driver's, would otherwise collide).

### Budget

Measured in the sandbox: 4 CPUs, Quint 0.33.1 on the TypeScript backend, Apalache 0.62.4 on the
Temurin JDK 21.

| Check | Budget | Time |
|:--|:--|:--|
| `formal-test` (20 daemon tests) | every `check` | under a second |
| `formal-freshness`: five traces regenerated | every `check` | about 2 s per trace |
| `conformance-replay` | 10 traces of `sim_steps` (40), generated twice, every `check`, then the control | a few seconds (IDX-12: 240 generated traces at six seeds, 1640 states each batch, conform) |
| `formal-verify daemon`: Apalache | `steps = 8` | 462 s; 552 s with IDX-12's `sweep` and `noticeSwept` (2026-11-19, another lane building) |
| `formal-verify daemon`: simulation | 20000 samples of `sim_steps` (40) | 194 s (170 s without the other lane running); 157 s with IDX-12's actions |
| `formal-verify daemon`: liveness and its control | 3000 samples of 92 steps, each | 89 s, then the control fails within seconds; 72 s with IDX-12's actions, and its `progressStepIgnoringSweep` control (by hand) fails in 3 s |
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
| `witnessSweptNoticed` | a daemon stopping after its socket was swept (IDX-12) | under 1 s |

The rarest is an answer after a crash, which is why `sim_steps` is 40, not 30. Raise either
number only with a measurement, while `formal.yml`'s 60-minute job stays well under its limit.

## `diagnostics`: versions, epochs and publication

`formal/diagnostics.qnt` (FORMAL-05; plan §3.1, §3.2, §3.4, §7.3; ADR 0007, ADR 0036). The
driver is LSP-04's, over `jfl-application`'s diagnostics coordinator with the testkit's scripted
compiler and worker, a scripted disk, and a scripted LSP client that records every
`textDocument/publishDiagnostics`: `crates/jfl-conformance/src/diagnostics.rs` (LSP-04), which
replays every regression and generated trace against the real coordinator.

### The world

- **Three files** of one worktree and one `EnvironmentKey`. `a` imports `b`, so `a`'s compile
  reads `b`'s text (javac's `-sourcepath`, plan §3.1); `b`'s reads only its own. `a` also names a
  class `C` in its package: `a`'s compile reads `c` while `c` exists and fails "cannot find
  symbol" while it does not, so the **absence** of `c` is an input too (a negative lookup). The
  client opens `a` and `b`; `c` is only created, written and deleted on disk, by an agent.
- **Texts.** Every change of a text, a disk write or an editor change, takes the next number, so a
  larger number is a newer text; 0 is an absent file. On disk at the start: `a` is 1, `b` is 2,
  `c` is absent.
- **The disk and what the server has seen of it.** An agent's shell edit, a formatter or a `git
  checkout` writes the disk; the server learns of it later, through its watcher or the freshness
  stat (plan §3.2, §4.3), as `noticeDisk`.
- **A client** (an editor, or Claude Code) opens, changes and closes documents. Each open is a new
  session. Its version is a pick, 1 to 3: a client may number every open from 1 again, and a
  proxy that re-sends a document to a restarted daemon (plan §5.2) sends the client's current
  version.
- **One javac worker** (ADR 0036), spawned lazily, with incarnations: it crashes, times out and
  restarts. It runs up to three jobs at once and finishes them in any order. It stands for a
  worker pool and for the syntax pass beside javac, whose replies interleave.
- **What the client shows** of each document: the last `publishDiagnostics` it received.

Bounds: texts up to 14, versions up to 6 (a `didOpen` at 1, 2 or 3), three open sessions per
document, three worker incarnations, three job slots. An action that would pass a bound is disabled.

### Design decisions the model fixes

1. **Results are ordered by input epoch, not by LSP version.**
   - A document's diagnostics depend on its own text and on the texts its compile reads. A
     change to `b` makes `a`'s diagnostics stale, though `a`'s version did not move: two replies
     for `a` at one version, one of which read `b`'s older text.
   - So each document has an **epoch**, bumped whenever any of its inputs changes: its own text
     (a `didChange`, a seen disk write), the text it reads of a dependency (the dependency's
     `didChange`, its seen disk write, its `didOpen` or `didClose` when that changes which text
     wins), and every `didOpen` of the document. A compile records the epoch it read.
   - **Inputs are dynamic.** javac's inputs are what it read, and also what it looked up and did
     not find: creating `c` after `a` failed to find `C` must make `a` pending, though no file
     `a`'s compile read has changed. A coordinator that tracks only the files read leaves "cannot
     find symbol `C`" published as current (the FORMAL-05 review's counterexample).
   - Pinned by `NeverOlderOverNewer` and `FreshAfterEdit`, the scenarios
     `dependencyEditMakesStale` and `createdFileMakesStale`, and the controls `deliverByVersion`
     and `noticeDiskTrackingReads`.
2. **Open sessions.** Epochs never restart, so a reply computed in an earlier session never
   matches a later one, even when the reopened document is at version 1 again with other text.
   A re-sent document keeps the client's version (`reopenAtTheClientVersionTest`): renumbering
   it would tag every publication with a version the client never sent for that text. Pinned by
   `PublishedMatchesVersion`, the scenario `closeDuringCompile` and the control
   `deliverByVersion`'s reopen test.
3. **Every change makes a compile pending at once,** so plan §3.2's rule (drop a result older than
   the newest published or pending one) publishes a reply only when it was computed for the
   document's current epoch in its current session. A client then never receives, after an
   edit, diagnostics of the text it replaced. Pinned by `FreshAfterEdit` and the control
   `deliverNotOlderThanPublished` (the rule read without "pending").
4. **The newer text wins, disk or overlay** (plan §2.5). An open document's overlay is compiled
   while it is not older than the disk the server has seen. When the disk has overtaken it, the
   disk's text is compiled and published **without a version** (LSP's field is optional): it is
   no text the client sent. Pinned by `PublishedMatchesVersion`, the scenario
   `diskWinsOverOverlay` and the control `startJobTaggingClientVersion`.
5. **A compile reads every input's winning text,** overlays included: `b`'s overlay while `b` is
   open and its overlay is newer, else `b`'s seen disk. Closing `b` with an unsaved change makes
   `a` stale (`closingAnUnsavedDependencyTest`).
6. **Only open documents get diagnostics.** A close publishes an empty set at once, and nothing
   follows it until the next `didOpen`: replies still running are dropped. A disk write to a
   closed document makes stale only the open documents that read it
   (`closedDocumentGetsNothingTest`). Pinned by `ClosedPublishesNothing` and the controls
   `deliverIgnoringClose` and `didCloseWithoutClear`.
7. **A crash** loses the running jobs; a reply the worker wrote before it died is still read, and
   is published or dropped by the same rule. Every document whose current compile was lost is
   compiled again, on a worker spawned when a compile is pending (plan §3.4). Pinned by
   `EventuallyUpToDate` and its control `crashForgetting`.
8. **A timeout** compiles the document again and leaves the late job running; its reply is
   treated like any other (the scenario `timeoutLateReply`). Killing a hung worker is a crash.
9. **Nothing waits on the worker** (plan §3.2, ADR 0015): the coordinator's moves never block on
   a reply. Cancelling a superseded job is allowed and not required: it only removes a reply the
   rule would drop.

### What Claude Code sees

Claude Code reads pushed diagnostics after every edit. `FreshAfterEdit` is what it relies on:
diagnostics that arrive after an edit were computed for that edit or a newer one, whether or not
the client checks `version`. Until the first publication after its edit, a client still shows
the previous diagnostics, tagged with their own version; how long Claude Code waits for one is
E0's measurement ("diagnostics wait time", plan §9.2), and `jfl diag` waits for the current
version with a deadline (plan §3.2). An edit made only on disk (a shell edit) is seen by the
server when its watcher or stat sees it; until then nothing is stale to the server, which is the
views model's freshness, not this one's.

Diagnostics without a version (decision 4) have ranges into the disk's text, which an editor's
buffer may not hold: harmless for Claude Code, which reads the file again, but an editor would
misplace them. LSP-04's goldens pin those ranges.

"Only open documents get diagnostics" assumes Claude Code sends `didOpen` for the files it
edits. That is E0's open question (plan §9.2, §15); if it does not, Claude Code gets no
diagnostics at all, and the rule changes here first.

### LSP-04's obligations

These are the design above as LSP-04 must build it; the driver checks each against the model.

1. **The key is (document, open session, input epoch).** The epoch is bumped on every change of
   any input of the document's compile (decision 1) and on every `didOpen`. Never order or accept
   results by the LSP version.
2. **Each job records what javac actually read, and a change to any of it bumps the epoch:**
   - the source files read, with content hashes;
   - the negative lookups: the package directories listed (sources and class folders) for names
     it searched, so a file created or deleted there is a change; a coarser rule, any create or
     delete under a searched package directory, is acceptable;
   - the class folders of earlier builds it read (plan §3.1), as a change of any of them;
   - the classpath and the `EnvironmentKey`.

   The worker reads sources through a file manager the daemon controls (the view's pinned
   generation and the overlays) or records the hash of every file it read, so a disk change
   between the server's read and javac's is detected, never compiled unseen. The record comes
   from javac itself (a `JavaFileManager` wrapper, or a `TaskListener`), not from the imports.
3. **The publication rule:** a reply is published only when its document is open, in the same
   session, at the same epoch; otherwise it is dropped. Publish after every such reply, even when
   the diagnostics did not change: a client waits for a publication after its edit.
4. **The version tag:** the client's version whose text the compile read; none when the disk had
   overtaken the overlay.
5. **Close:** publish an empty set at once; drop every later reply for that session.
6. **A crash** re-queues every document whose current compile it lost, and a pending compile
   spawns the worker (with the backoff of plan §3.4). **A timeout** re-queues the document.
7. **The compile reads the winning text of every input,** overlays included, and the job records
   what it read; the scripted compiler echoes the inputs, the session, the epoch and the worker
   incarnation in the diagnostics it returns, so the client's observation decodes them.
8. **One publication is one epoch's whole set.** `publishDiagnostics` replaces a document's
   diagnostics, so when syntax diagnostics (LSP-04) and javac's (SEM-05) are combined, a
   publication combines results of one epoch only. Publishing them separately, or merging across
   epochs, changes this model first (ADR 0007).
9. **The driver can step the coordinator,** one model action at a time: start a job, read one
   reply, see one disk write.
10. **The observation is exact all the way down**, as for `views` (`deny_unknown_fields` and a
    derived `PartialEq` on every nested type).
11. **A re-sent document is never renumbered.** When a proxy re-sends its open documents to a
    restarted daemon (IDX-12), each keeps the client's version and text; the new daemon takes it
    as a `didOpen` at that version, and the client keeps the dead daemon's diagnostics until the
    new one publishes.
12. **`jfl diag` takes the views per-request stat and cookie first** (ADR 0037): of its file and
    of HEAD, so a shell edit the watcher has not delivered yet is pending before it waits for the
    current version's diagnostics with its deadline.

### Where the coordinator meets each obligation (LSP-04, first slice)

`jfl_application::diagnostics::Coordinator` is a core without I/O, stepped one model action at a
time by its shell (the driver, the tests, the daemon to come): obligations 1, 3, 4, 5, 6, 7, 9 and
10 are met by it and checked by the driver and `crates/jfl-application/tests/diagnostics_machine.rs`
(a reference machine with `EventuallyUpToDate`'s reduction). The overlay-or-disk choice compares
the caller's stamps (`Text`: the disk's mtime, an overlay's arrival stamped with the file's mtime
then, as LSP-02's overlays are); what a compile reads is `Coordinator::track`ed (the syntax pass:
the document alone). Left to the tasks that bring the rest: obligation 2's record of what javac
read and obligation 8's merge of syntax and javac results (SEM-05; the merge goes through the
model first), obligation 11 in the daemon's publication path and obligation 12 with `jfl diag`
(LSP-04's next slice and SEM-05).

The daemon's publication path (LSP-04, second slice: `jfl-composition`'s `daemon::diagnostics`)
is the coordinator's shell for the syntax pass. Obligation 7 holds there too: the worker compiles
the job's recorded input and never reads the disk
(`daemon::diagnostics::tests::should_compile_the_jobs_input_and_never_the_disk_when_the_disk_differs`).
Obligation 11 holds end to end: the proxy re-sends a document at the client's version and the
daemon passes it on (`crates/jfl-composition/tests/pushed_diagnostics.rs::should_publish_at_the_clients_version_when_a_restarted_daemon_gets_the_document_again`).
The stamps that order an overlay against the disk are a counter, one per observation, never mtimes
(two writes in one clock tick cannot tie), and the disk is compared by content. One step the
model does not have: a publication can race a newer change of the client's on its way, so it
carries the connection's count of document notifications when its compile took its inputs, and
the proxy drops it when it has sent a newer notification of that document since
(`FreshAfterEdit` kept past the daemon). The driver now also checks its observation of the worker
against the coordinator's own record of it (incarnation and jobs).

### Not modelled

- **Time:** deadlines, the worker's backoff and idle eviction, the pool's LRU cap (plan §7.4:
  component tests), debouncing (a pending compile may wait any number of steps), `jfl diag`'s wait.
- **Several environments and workers:** one worker whose replies come in any order stands for
  them.
- **Dynamic inputs beyond one negative lookup.** The model has one file that comes and goes
  (`c`). Not modelled, and covered by obligation 2: class folders a Gradle build rewrites,
  classpath and `EnvironmentKey` changes (a build-file edit), files created in other searched
  packages, generated sources, a dependency set that changes with the edit itself (a new
  import), and the gap between the server's read of a file and javac's.
- **The diagnostics' content.** A result is identified by the inputs it was computed from;
  positions and UTF-16 ranges are LSP-04's golden tests.
- **Annotation-processor suppression** (plan §3.3), and diagnostics of closed files (none are
  published; publishing them changes this model first).
- **How the server sees the disk:** `noticeDisk` stands for the view registry's watcher and stat
  (the `views` model); a dependency outside the worktree (a library) does not change.

### Stimuli: the driver's actions

`step` picks one of three equally likely groups, `clientStep`, `coordinatorStep` and `workerStep`
(faults, `faultStep`, are one branch of `workerStep`), then an enabled action in it. Each
parameter is a nondeterministic pick named as the parameter (`doc`, `job`); a pick the action did
not use is `None`.

| Action | Picks | What the driver does to the real code |
|:--|:--|:--|
| `init` | none | A fresh coordinator over a scripted disk holding `a` at text 1 and `b` at 2, and no `c`; `a` reads `b`, and `c` while it exists; all closed; no worker. |
| `didOpen` | `doc`, `version` | The client (or a proxy re-sending it) opens `a` or `b` with the disk's text, at `version`. |
| `didChange` | `doc` | The client sends the next version with a new text the disk does not have. |
| `toolEdit` | `doc` | Claude Code's Edit: the disk is written, then the same text is sent as the next version. |
| `diskWrite` | `doc` | The scripted disk is written (for `c`, perhaps created), and the coordinator is not told. |
| `diskDelete` | `doc` | The scripted disk deletes `c`, and the coordinator is not told. |
| `didClose` | `doc` | The client closes the document. |
| `noticeDisk` | `doc` | The coordinator is told the file's disk text (its watcher or stat); `doc` may be `c`. |
| `startJob` | `doc` | The coordinator sends the document's pending compile to the worker, in the first free slot (`j1`, `j2`, `j3`). |
| `deliver` | `job` | The coordinator reads the job's reply. |
| `complete` | `job` | The scripted worker finishes the job and writes its reply. |
| `restart` | none | The coordinator spawns the worker (lazily, or after a crash). |
| `crash` | none | The scripted worker dies; its running jobs are lost. |
| `timeout` | `job` | The job's deadline passes. |

Jobs take the first free slot, so an action has one outcome; the driver numbers its jobs the
same way.

### Observation: what the driver produces after every step

The model has eleven variables:

| Variable | Type | Observed from |
|:--|:--|:--|
| `disk` | file → int | The scripted disk's texts, 0 for an absent file. |
| `seenC` | int | The text of `c` the coordinator has seen, 0 while it has seen none (a separate number, not a third document: a third entry in every map made Apalache several times slower per step). |
| `docs` | doc → `{ open, session, version, overlay, seenDisk, epoch, dirty }` | The coordinator, per document: open or not, its open count, the client's latest version and its text, the disk text it has seen, its epoch, and whether a compile is pending. |
| `worker` | `{ up, inc, lost }` | The scripted worker: up or not, its incarnation, and the running jobs its crashes lost. |
| `jobs` | slot → `{ state, doc, session, version, epoch, inputs, inc, late }` | Each job: `free`, `running` or `done` (its reply written, not read); what it was started for; `inputs` the texts it read (`{ a, b, c }`, 0 for a text it does not read or a file it did not find); the incarnation; whether it timed out. |
| `client` | doc → `{ kind, version, session, epoch, inputs, inc }` | The scripted client's last notification per document: `none`, `cleared` (an empty set) or `diags`; its `version` (-1 for none); the rest decoded from the diagnostics the scripted compiler wrote. |
| `high` | doc → `{ epoch, version }` | Ghost: the highest epoch and version published before the latest in the current session (-1: none). The driver keeps them. |
| `sinceEdit` | doc → bool | Ghost: a publication arrived after the latest change of the document's inputs. |
| `sent` | set of `{ doc, session, version, text }` | Ghost: every text the client sent. |
| `nextText` | int | The scripted disk's and client's next text number. |
| `t` | int | The driver's step index (the scenarios' and the liveness simulation's clock). |

### Invariants

`val allInvariants` is the conjunction of these four. Apalache and the simulation check it.

| Invariant | States |
|:--|:--|
| `NeverOlderOverNewer` | Within an open session, each publication's epoch and version are at least those of every earlier publication of the document. |
| `PublishedMatchesVersion` | A publication that carries version `v` was computed from exactly the text the client sent as `v` in the document's current session. One without a version is not checked against the client's texts (decision 4 makes it disk content). |
| `ClosedPublishesNothing` | A closed document shows the empty set its close published (or nothing, never opened). |
| `FreshAfterEdit` | Diagnostics published after a change of an open document's inputs were computed for its current inputs: its current epoch in its current session, and the texts every input has now (so an input the epoch failed to track, such as a created file, is caught). |

The first three are plan §7.3's, stated on what the diagnostics were computed for: plan §7.3's
"version" is the input epoch, since the LSP version does not name a dependency's text (decision
1), and "never decreases" holds within an open session, since a client may number a reopened
document from 1 again (decision 2). `FreshAfterEdit` is plan §3.2's "older than the newest
pending" with every change pending at once (decision 3); `NeverOlderOverNewer` alone admits
diagnostics of a superseded text published after the edit that superseded it.

### Liveness: `EventuallyUpToDate`

Plan §3.2 and §3.4: a document whose edits stop eventually shows diagnostics of its current
inputs, through crashes and timeouts. Once the client, the agent and the faults stop, and under
weak fairness of the coordinator's and the worker's moves, that fails only by a deadlock (a lost
compile never started again) or a livelock. As for `daemon`, the model's `progressStep` takes 30
random steps of `step` (faults and clients included), then only system moves (`noticeDisk`,
`startJob`, `complete`, `deliver`, `restart`), each picked at random among the enabled ones;
`Progress` fails unless, 30 moves later, nothing is enabled and every open document is up to
date. `formal-verify diagnostics` runs it from the `progress` entry in `formal/models.toml`
(3000 samples of 62 steps of `progressStep` keeping `allProgress`), then the same of the
liveness control `progressStepForgetting` (a crash that does not re-queue what it lost), which
must report a violation. Every `check` runs the control's Quint test,
`crashForgettingNeverUpdatesTest`.

### Scenarios and regression traces

Each scenario is a scripted step whose branches are guarded by the step count `t`, so exactly one
action is enabled in each state. Each, run at seeds `0x1`, `0x3` and `0xdead`, gave one action
sequence. Its regression trace, at seed `0x3`, is in
`formal/traces/regression/manifest.toml`, and its test takes the same steps as named actions.

| Scenario (`--step`) | Steps | What it shows |
|:--|:--|:--|
| `twoQuickEditsReverseOrder` | 13 | Three versions compiled; the newest reply arrives first and is published, the two older ones after it and are dropped. |
| `crashMidCompile` | 13 | A crash loses version 2's compile; the restarted worker compiles it again; a reply the dead worker wrote for version 1 is read after the crash and dropped; the Edit tool's disk write, seen last, changes nothing. |
| `closeDuringCompile` | 15 | A close while a compile runs publishes an empty set and drops the reply; reopened twice at version 1 with the same text, session 2's reply is dropped and session 3's published. |
| `dependencyEditMakesStale` | 10 | An agent rewrites `b` on disk while `a` is open at version 1; `a`'s two replies, both at version 1, arrive in reverse order, and only the one that read the new `b` is published. |
| `diskWinsOverOverlay` | 16 | An unsaved change, then an agent's disk write the server sees: the disk wins, its diagnostics carry no version, and the overlay's older reply is dropped; Claude Code's Edit then sends version 3 with the disk's text. |
| `timeoutLateReply` | 9 | A compile times out and is compiled again; both replies, the late one last, are for the current inputs and both are published. |
| `createdFileMakesStale` | 10 | `a` is published while `c` is absent ("cannot find symbol `C`"); an agent creates `c`; once the server sees it, `a` is pending though no file it read changed, and its next diagnostics read `c`. The FORMAL-05 review's counterexample, with the fix. |

More tests without a trace: `deletedFileMakesStaleTest`, `reopenAtTheClientVersionTest`,
`closedDocumentGetsNothingTest`, `dependencyOverlayIsReadTest`
(an unsaved change of `b` is what `a` reads), `closingAnUnsavedDependencyTest`,
`newerOverlayWinsTest` (an overlay newer than a seen disk write wins) and `reopenReadsTheDiskTest`
(a document reopened after an unseen disk write is compiled from what the client read).

### Broken-model controls

Each control is an action variant with a Quint test that expects its invariant to be false:

| Control | Fault | Violates |
|:--|:--|:--|
| `deliverEveryResult` | Plan §7.3's control: publishes every reply as it arrives. | `NeverOlderOverNewer` |
| `deliverByVersion` | Accepts a reply at the document's current LSP version: a dependency's change does not move it, and a reopened document starts at 1 again. Two tests: the dependency case and the reopen case. | `NeverOlderOverNewer`, `PublishedMatchesVersion` |
| `deliverIgnoringClose` | Forgets the close: a reply of the last epoch is published after it. | `ClosedPublishesNothing` |
| `didCloseWithoutClear` | Closes without publishing the empty set. | `ClosedPublishesNothing` |
| `deliverNotOlderThanPublished` | Plan §3.2's rule without "pending": accepts a reply not older than the published one, though a newer compile is pending. Its test shows the other invariants hold. | `FreshAfterEdit` |
| `startJobTaggingClientVersion` | Tags a compile of disk content with the client's version. | `PublishedMatchesVersion` |
| `noticeDiskTrackingReads` | Tracks only the files a compile read: creating a file it looked up and did not find changes nothing tracked, and "cannot find symbol" stays current. Its test shows the other invariants hold. | `FreshAfterEdit` |
| `crashForgetting` | A crash that does not re-queue the compiles it lost. | `EventuallyUpToDate` (`Progress`) |

Each control is also found by random simulation, with its fault in place of the correct action
in `step` (20000 samples of 40 steps at seed `0x77`, a scratch module importing the model): every
one within 3 s; `deliverNotOlderThanPublished` breaks neither `NeverOlderOverNewer` nor
`PublishedMatchesVersion` in 20000 samples.

Plan §7.3 names LSP-04's own negative control: a coordinator that publishes every reply as it
arrives. It must diverge at the `deliver` of an older reply after a newer one was published:
step 12 of `twoQuickEditsReverseOrder`.

### Budget

Measured in the sandbox: 4 CPUs, Quint 0.33.1 on the TypeScript backend, Apalache 0.62.4 on the
Temurin JDK 21.

| Check | Budget | Time |
|:--|:--|:--|
| `formal-test` (23 diagnostics tests) | every `check` | under a second |
| `formal-freshness`: seven traces regenerated | every `check` | about 2 s per trace |
| `conformance-replay` | 10 traces of `sim_steps` (40), generated twice, every `check` | a few seconds; replayed through the coordinator since LSP-04. |
| `formal-verify diagnostics`: Apalache | `steps = 7` | 522 s |
| `formal-verify diagnostics`: simulation | 20000 samples of `sim_steps` (40) | 149 s |
| `formal-verify diagnostics`: liveness and its control | 3000 samples of 62 steps, each | 34 s, then the control fails within seconds |
| `formal-verify diagnostics` in all | weekly in `formal.yml` (60-minute job) | 725 s with xtask's build, another lane building beside it; perhaps twice that on a CI runner |

**Apalache's time grows steeply with the bound:** on round 1's model (no `c`, `didOpen` at 1)
18 s at length 4, 483 s at 7 and 1463 s at 8 (0.6 GB peak); on round 2's, 522 to 554 s at 7. So
the bound is 7, keeping the job well under its 60 minutes on a slower runner. With `c` as a third
document in every map, a step took about five times as long (length 5 reached in about 210 s
instead of 45), which is why the server's view of `c` is one number. Length 7 covers every interleaving of a first compile with one
change, a close, a crash and a restart, or a dependency's disk write and its notice: a stale
reply dropped after an edit takes 6 steps (open, spawn, start, change, finish, read), a close
during a compile 6, a crash mid-compile and the compile again 7, a published `a` made stale by
`c`'s creation 7. Two replies in reverse order
take 9 and are the simulation's. **The depth rests on the simulation**, run weekly in
`formal-verify` and at a fresh seed in every `conformance-replay`: 20000 samples of 40 steps in
`formal-verify`'s run above (seed `0x10023ab3705fa`) found no violation. Each of these witness states is
found by `quint run --invariant 'not(<witness>)' --max-steps 40 --max-samples 20000` at seed
`0x77`:

| Witness (`val`) | State | Found after |
|:--|:--|:--|
| `witnessRepublished` | diagnostics replaced earlier ones of the same session | under 1 s |
| `witnessReverseOrder` | a reply older than the published diagnostics is still to be read | under 1 s |
| `witnessCrashMidCompile` | a crash lost a running compile | under 1 s |
| `witnessCurrentAfterCrash` | after a lost compile, a later incarnation's diagnostics are up to date | under 1 s |
| `witnessCloseDuringCompile` | a document closed while its compile runs or its reply waits | under 1 s |
| `witnessSupersededVersion` | a reply for a version the client has superseded | under 1 s |
| `witnessDependencyStale` | a reply for `a` at its current version, stale only because `b` changed | under 1 s |
| `witnessReopenStale` | a reply of an earlier open session of a reopened document | under 1 s |
| `witnessDiskWins` | an open document's up-to-date diagnostics carry no version | under 1 s |
| `witnessLateReply` | a timed-out compile's reply arrives | under 1 s |
| `witnessBothUpToDate` | both documents up to date, `a`'s read from `b`'s unsaved overlay | 3 s |
| `witnessCreatedStale` | a reply for `a` that did not find `C`, read after the server saw `c` created | under 1 s |
| `witnessReadsCreated` | `a`'s up-to-date diagnostics, published again, read a created `c` | 1 s |

Every witness is reached within 3 s, so `sim_steps` is 40 for depth (a run holds several
open sessions, crashes and dependency changes), not for reach. Raise either number only with a
measurement, while `formal.yml`'s 60-minute job stays well under its limit.
