# §31H — Launch journal of ferritecad-viewer

Implemented on branch `viewer-launch-journal`; independent review is in progress.
The author ran no window, viewer, bundle, CUA or GPU. Base:
`062e467c82a55619026cad4fb4ddc87ef9274e4d` (merge of
PR #111, tree `b49fc746…`; HEAD = main = origin/main after a fresh fetch, clean tree;
the foreign worktree `a200` untouched). The plan entry is [§31H](implementation-plan.md).
Goal: after a start from Finder, with no terminal, a person can find a local file that
says what the window said. Not in scope and not touched: telemetry, a memory profiler,
a settings UI, the OOM investigation.

## Policy (fixed before the code)

**Who writes it.** Only the window route of `ferritecad-viewer`. `main` reads the command
line exactly as before; for a window request it runs `journal::launch(|| window(document))`:
the journal is opened (with the panic hook) **before** the event loop is asked for, so a
window that cannot be made is written too, and how the route ended is written after it.
The document, kernel, solver, jobs and CLI crates write nothing; `ferritecad` (the CLI),
`ferritecad-viewer --solver-info` and every usage refusal open no journal, read no
journal folder, need no writable `HOME`, and keep their stdout, stderr and exit codes byte
for byte.

**Where.** One per-user folder outside the checkout, the bundle and every model's folder:

| Platform | Folder |
| --- | --- |
| macOS (arm64) | `~/Library/Logs/FerriteCAD` (the per-user folder macOS keeps application logs in) |
| Windows | `%LOCALAPPDATA%\FerriteCAD\Logs` |
| Linux and other Unix | `$XDG_STATE_HOME/ferritecad/logs`, by default `~/.local/state/ferritecad/logs` |

`FERRITECAD_LOG_DIR` (not empty; a relative value is made absolute against the working
directory) names another folder, for tests and for the window recipe; `HOME` is never
changed for it. Windows and Linux reuse `ferritecad_jobs::per_user_state_folder` (the
recovery and tab-list folders' rule); macOS takes the conventional log folder instead of
`Application Support`. A missing folder is made (`0700` where the platform has modes); a
folder that is a link or not a folder is refused and never followed. Journal files are
created `0600` (Unix). A refusal, a folder that cannot be written, a full disk or a
failed write never stops the window: the journal says once on stderr why it is not kept
(`ferritecad: no launch journal: …` at the start, or `ferritecad: the launch journal
stopped: …` later) and writes nothing more for that process. No retry, no queue, no
second attempt later, no recursion (those two lines go straight to stderr, never through
the journal).

**Files of one instance.** Each process that opens a journal is one *instance*, named by
a fresh UUIDv7. A PID alone is not unique: it is reused, and two journals in one process
share it. Every file of an instance starts with
`viewer-<UTC start YYYYMMDDTHHMMSSZ>-<pid>-<uuid>`:

* `….lock` — created exclusively, locked exclusively with `File::try_lock` for the whole
  life of the process, then given one line `FERRITECAD-LAUNCH-JOURNAL-LOCK 1 pid=<pid>
  instance=<uuid>`. The operating system lets go of the lock when the process ends however
  it ends; that is the only liveness signal (no PID, age or host name is consulted), as for
  the recovery leases. Segments are not locked, so they can be read while the window runs,
  also on Windows where locks are mandatory.
* `….<n>.log` — segment `n` (1, 2, 3, …), created exclusively, whose first line is the
  header `FERRITECAD-LAUNCH-JOURNAL 1 segment=<n> ferritecad-viewer <version> os=<os>
  arch=<arch> pid=<pid> instance=<uuid> started=<UTC>`.

**Size.** A segment holds at most **16 MiB = 16 777 216 bytes, newlines included**. A
record is written only when it fits whole (`length + record ≤ 16 MiB`); otherwise the next
segment is opened first. A record is one line of at most **16 KiB** (16 384 bytes,
newline included): `<UTC time> <kind> <message>\n`. Control characters other than tab are
escaped (`\n`, `\r`, `\u{1b}`), so a record is always one line. A longer message is cut at
a character boundary — never inside a UTF-8 sequence or an escape — and ends with
` [truncated]`; the time, kind and message take at most 16 371 bytes so that the marker and
the newline always fit. Formatting stops at the limit (the formatter is refused further
text), so a message of any size costs at most 16 KiB of memory and work per writing
thread. Why 16 KiB: far above any message this application writes (an error naming two
long paths is a few KiB), a thousandth of a segment (a segment always holds more than a
thousand records), and small enough to be one buffer per thread.

**Rotation inside one instance: the first segment and the current one (two segments,
32 MiB).** The first segment holds the start and the earliest errors — what a launch
problem is usually about — and is kept for the instance's whole life. When a later segment
is full, the next one is created and headed first, then the full one is removed (segment 1
is never removed by rotation). Each header repeats the instance's facts and its number, so
a gap in the numbers says how many were removed. If the next segment cannot be created or
the full one cannot be removed, the journal stops (stderr says so once): an instance never
grows past three segments (48 MiB) even when rotation or removal is impossible, and
never past 16 MiB in any one file.

**Retention of finished instances: the four newest.** When a window starts, after its own
files exist, it looks once at the folder. An instance is *finished* when its lock file can
be locked (`try_lock`, never a waiting lock) and carries the lock line that names it; one
whose lock is held is running and is not touched, not rotated and not waited for; one that
cannot be judged (no lock file, a lock file without that line, a lock that cannot be
tried, two names for one UUID) is left alone. Finished instances are ordered by their
UUIDv7 (creation time). The files of all but the newest **four** are removed: each segment
only if it is a plain file (not a link) whose first bytes are that segment's header, the
lock file last, only after all readable journal segments were removed. If reading a
segment header or removing a segment fails, the lock stays so a later launch can retry;
an already absent segment is harmless. Unrecognised files and links stay untouched.
An interrupted removal is still a finished instance next time. A finished
lock file without segments holds nothing and is removed without counting. Nothing that does
not match the exact name grammar is read, followed or removed; links are never followed, so
no link target is removed.

**What that bounds, honestly.** Per instance, not globally: each running viewer keeps at
most 32 MiB (48 MiB when rotation broke), and the four kept finished instances at most
4 × 32 MiB (4 × 48 MiB in the worst case) beside them. Several viewers running at once each
add their own bound; there is no single cap for the folder. A crash in the microseconds
between creating a file and writing its first line can leave an empty file that no
instance removes. Ordering by UUIDv7 follows the clock: a clock set back can make an older
instance look newer.
Retention cannot enforce the completed-instance count while filesystem access is denied;
it preserves the lock and retries on a later launch after access is restored.

**Threads and panics.** One writer per process behind a mutex; no queue, no background
thread. An ordinary caller waits for one record at most (one `write` of ≤ 16 KiB). The
panic hook never waits for the mutex: it tries it 100 times, 1 ms apart (about a tenth of a
second), and then leaves the journal alone; this also covers a panic on the thread that is
writing. The previous hook always runs after it (stderr unchanged), and unwinding or abort
is the runtime's, unchanged. Neither the hook nor the writer panics; a poisoned mutex still
guards a whole journal (nothing panics while holding it).

## What is written, and what cannot be

Written, and nothing else:

| Kind | When | Content |
| --- | --- | --- |
| header (line 1 of each segment) | the window route starts; each new segment | app name and version (`CARGO_PKG_VERSION`), OS family and architecture (`std::env::consts`), PID, instance UUID, UTC start, segment number |
| `error` | an application diagnostic that is printed on stderr | the same text as the stderr line (`ferritecad: …`) |
| `panic` | a panic after the hook is installed | location and message (no backtrace, no thread name) |
| `exit` | the window route returned | `the event loop ended`, or `the window failed: <error>: <source>: …` (the error and its source chain — the event-loop refusal's reason is only in the source) |

Not written: frames, pointer or keyboard events, model or document contents, geometry,
request contents, the environment, the OS version (deliberately not collected here),
output of Open CASCADE or other libraries (no fd 1/2 redirect), the peer CLI's output.
Nothing is sent anywhere.

Cannot be written, and not claimed: a kill by the system (`SIGKILL`, jetsam/OOM), a
native crash (`SIGSEGV`/`SIGBUS` in Open CASCADE, a driver or Rust code), an abort that
does not go through the Rust panic hook (`std::process::abort`, an allocation failure, a
stack overflow), a failure of the loader before `main` (a missing dylib), anything before
the window route opened the journal (the command line, usage), and anything after a
journal stopped. Records written before such an end are on disk already (each record is
one unbuffered `write`; no `fsync` is claimed, so a power cut may lose the tail). The
earlier OOM is not diagnosed by this slice.

### Inventory of the diagnostic route

One small route, `journal::report(format_args!(…))`: it writes the exact bytes
`eprintln!` wrote to stderr first, then the journal; a journal failure never costs the
stderr line. Unlike `eprintln!` it does not panic when stderr cannot be written, and
(being a direct stderr write) libtest no longer captures these lines in unit tests.

| Site (`crates/ferritecad-app/src/…`) | What it says | Route |
| --- | --- | --- |
| `main.rs` `main` usage refusal | `invalid input: usage: …` | **unchanged `eprintln!`** — before the window route, never journaled |
| `main.rs` `solver_info` | `sketch solver: …`, `provenance: …` on stdout | **unchanged `println!`** — never journaled |
| `main.rs` `end_window_quit` | the list of open files was not kept (second failure at Quit) | `report` |
| `main.rs` `resumed` | the window/surface could not be made (then exits) | `report` |
| `main.rs` `AppEvent::SceneStaged`, `TabShown`, `Recovered`, `Created`, `Loaded` | a picture/document that could not be shown or read (the `Loaded` answer includes a file that cannot be opened) | `report` (5 sites) |
| `main.rs` `WindowEvent::Resized`, `RedrawRequested` | surface resize or frame failure (then exits) | `report` (2 sites) |
| `main.rs` magnify gesture, `close_now`, `keep_picture`, `choose_at`, `frame_selection`, `frame_whole_scene` | a failed gesture, tab close, re-pointed picture, pick or framing | `report` (6 sites) |
| `main.rs` `point_at` | a failed hover pick — printed once per pointer move while it keeps failing; bounded by rotation | `report` |
| `last_tabs.rs` `Folder::publish` | the tab list's folder could not be synced | `report` |
| about 90 `eprintln!("skipped: …")` and `println!("\nFCAD_…")` in test modules | test skips and markers | unchanged, not the application's journal |

Every `eprintln!`/`println!` in the other crates the window links (jobs, document, eval,
sketch-solver, viewport-gpu, …) is inside a test module.

## Code

* `crates/ferritecad-app/src/journal.rs` (new): `launch`, `report`, the panic hook,
  `Journal` (open, append, rotate, stop), `record` (bounded line), the name grammar and
  the sweep. No new dependency: std file locks (`File::try_lock`, Rust 1.89, within the
  1.90 MSRV), `ferritecad_types::ObjectId` (UUIDv7), `ferritecad_jobs::format_utc` and
  `per_user_state_folder`. `Cargo.toml`/`Cargo.lock`, notices, SBOMs and inventories are
  unchanged.
* `crates/ferritecad-app/src/main.rs`: `mod journal`; `main` hands the window route to
  `journal::launch`; the window code moved unchanged into `fn window`; 16 `eprintln!`
  diagnostics → `journal::report`.
* `crates/ferritecad-app/src/last_tabs.rs`: its one diagnostic → `journal::report`.
* Tests: `crates/ferritecad-app/src/journal/tests.rs` (new; 9 gates + 1 child),
  `crates/ferritecad-app/src/sessions/tests/tabs/restore.rs` (the route gate + its child),
  `crates/ferritecad-app/tests/solver_info.rs` (journal override on every run, the exact
  `--solver-info`/usage gate, the Linux window-route gate).
* `.github/workflows/ci.yml`: one step in the existing `test` job (three OSes), "Keep the
  viewer launch journal bounded and per instance": 10 exact-name gates with
  `FCAD_LAUNCH_JOURNAL_…` markers and the `solver_info` process test (its Linux-only marker
  on Linux). No new workflow, no STEP/FBX/GPU corpus.

## Executed (author, local)

All builds sequential (`CARGO_BUILD_JOBS=1`, `nice`), existing targets and pinned
libraries, OCCT/PlaneGCS not rebuilt. Every test executable ran under
`tools/watch-viewer-memory.py --limit-mib 1536` with a finite limit and its own log
(including each mutation); none is a widget, window or GPU test. Logs: the ignored
`target/launch-journal-check/`. Sub-second runs get one 0.5 s sample, so their peaks are
coarse.

| Check | Result |
| --- | --- |
| Baseline freeze | viewer built at the clean base before any edit: native (`--features planegcs`, 50 OCCT + 1 planegcs links) and true stub (0 libTK/planegcs, build warning "Open CASCADE was not usable"); copied read-only to `baseline/` |
| `--solver-info` and usage, real binaries (`cli-matrix.py`: `--solver-info`; `--solver-info part.fcad`; `--solver-info --solver-info`; `--solver-info ''`; `--help`; `-x`; `a.fcad b.fcad`) | after vs baseline: stdout, stderr and exit codes **byte-identical** in both shapes (native 0/2×6, stub 3/2×6); the `FERRITECAD_LOG_DIR` folder given to each run was never made; `~/Library/Logs/FerriteCAD` never existed on this Mac during the work; the new binaries contain the journal (strings), the baselines do not |
| Writer gates (`journal::tests`, 9 + 1 child), native | 9 passed, 0 failed, 1 ignored (the child); markers `FCAD_LAUNCH_JOURNAL_{EXACT_LIMIT,LARGE_RECORD,UNICODE,ROTATION,RETENTION,FAILURES,THREADS,BUSY_PANIC,INSTANCES}_EXECUTED`; pressure 1, swap 0 |
| Route gate (`sessions::tests::tabs::restore::the_window_route_journals_its_start_a_real_error_a_panic_and_its_end`, 4 child processes) | passed, `FCAD_LAUNCH_JOURNAL_ROUTE_EXECUTED`; a manual panic-mode child's journal read by eye: header (`0.0.1 os=macos arch=aarch64 pid=<child>`), the real `end_window_quit` error, `panic at …restore.rs:…: FCAD_LAUNCH_JOURNAL a panic in the window`; stderr kept the error line and the default hook's message |
| `tests/solver_info.rs` (real viewer: `--solver-info`, usage; no window on macOS) | 4 passed; `FCAD_LAUNCH_JOURNAL_SOLVER_INFO_EXECUTED` |
| ci.yml step (the real `run:` body cut by indentation, each `cargo test` replaced by the same filter on the built executable under the watchdog, `make-ci-step.py`) | **true stub** and **native**: exit 0, 10 × `1 passed; 0 failed` + `solver_info` 4 passed, 11 markers, no `skipped:` |
| Mutations (copy of the existing harness, below) | 3 of 3 compiled and failed on executed assertions; files restored byte for byte; positive repeats green |
| fmt; `clippy --workspace --all-targets --all-features -D warnings` | pass on the final tree; the Linux-only gate was also compiled and clippy'd through a temporary `cfg(any(linux, macos))` swap, restored byte for byte (sha256 compared) |
| actionlint `ci.yml`; `check-actions-run-size.py`; licence headers; solver ownership; export boundary; STEP corpus; planegcs pins; notice ownership; `git diff --check` | pass (run-size: 211 blocks, largest 20 432 of 21 000, not this step); no manifest, lock, deny, notice, SBOM or inventory change |
| Whole true-stub viewer suite under the watchdog | **not completed**: aborted by the watchdog after 10 s ("system memory pressure": the system went to level 2; the test process peaked at 186.7 MiB, swap 0). Pressure then stayed at 2 for more than 70 minutes because of other applications; the watchdog refuses to start below level 1 and it was not bypassed |
| Final-tree repeat | **pending** for three small edits made after the runs above: the route gate's `error` mode now carries a source and expects `…: no display in this child` in the `exit` record; one assertion in `solver_info.rs` moved above an existing comment; two unused derives removed in `journal.rs`. All compile and pass clippy; they were not executed. Commands below |

Pending, at pressure 1 (from the checkout; `G=target/launch-journal-check`):

```sh
source $G/guard.sh $G/native-env.sh $PWD/target/debug/deps/ferritecad_viewer-24959a0b09ee0124 final-native 900 journal::tests:: --nocapture --test-threads=1
source $G/guard.sh $G/native-env.sh $PWD/target/debug/deps/ferritecad_viewer-24959a0b09ee0124 final-route 900 sessions::tests::tabs::restore::the_window_route_journals_its_start_a_real_error_a_panic_and_its_end --exact --nocapture --test-threads=1
source $G/guard.sh $G/native-env.sh $PWD/target/debug/deps/solver_info-641892f5b6180c72 final-solver-info 300 --nocapture --test-threads=1
source $G/guard.sh $G/stub-env.sh /private/tmp/ferrite-25j-stub-target/debug/deps/ferritecad_viewer-98d3567b5f805b3f final-stub-whole 1800
```

### Mutations (copy of `target/31h-check/mutate.py` as `target/launch-journal-check/mutate.py`; only its list changed)

| Mutation (`journal.rs`) | Gate(s) | Failed at |
| --- | --- | --- |
| size checked after the write (`self.len > SEGMENT_LIMIT` instead of `self.len + bytes > …`) | exact-limit | "segment 1 changed: 16777218 bytes" |
| a held lock taken for a finished instance (`WouldBlock` no longer means running) | instances; retention | the child process's active files were gone (`left: {}`); the running in-process instance's files were gone |
| the hook drops the previous one (`previous(info)` not called) | route | the child's stderr had no `panicked at` |

The first mutation run exposed two defects of the tests themselves, both fixed before the
recorded run: an `assert_eq!` on two 16 MiB byte vectors wrote 134 MB of Debug output on
failure (now `assert!(a == b)`), and a parent that failed early left its `instance_child`
waiting, so the watchdog rightly refused the next run (the child is now killed and reaped
by a guard, and stops when the parent's folder is gone). That orphan was this session's own
process; it was stopped by its PID after checking its command line, and its 134 MB log was
deleted.

### Found while building, and changed

* `PanicHookInfo::payload_as_str` is Rust 1.91, above the 1.90 MSRV (clippy
  `incompatible_msrv`): the payload is downcast instead.
* `CadError::Io` shows only its context, so the operating system's reason is put into the
  context (`creating …: Permission denied (os error 13)`).
* On Windows, locks are mandatory: the tests list a folder without reading files, because a
  running instance's lock file cannot be read there.
* The `exit` record carries the error's source chain; the winit refusal's reason is only in
  the source.

### Design choices flagged for review

* macOS folder `~/Library/Logs/FerriteCAD` rather than the existing state helper's
  `Application Support/FerriteCAD/Logs` (Windows and Linux reuse that helper).
* First + current segment per instance (launch problems are at the start) rather than the
  newest two; four finished instances kept.
* 16 KiB per record; ~100 ms bounded try in the panic hook, then the journal is skipped.
* Two new stderr lines exist only on failure (`no launch journal`, `launch journal
  stopped`); a broken stderr no longer panics the diagnostic sites (`eprintln!` did).
* No OS version in the header (only OS family and architecture).
* The route gate drives one real production diagnostic (`end_window_quit`) inside
  `journal::launch`; the real `window()` is entered headless only on Linux in CI.

### N/A, with reasons

* Window, viewer, bundle, CUA, GPU: not run by the author (the reviewer's part).
* Native release whole suite, mixed (OCCT without solver) whole suite, STEP/FBX/Unity
  corpus, SQL/STL/FBX comparisons, ufbx: no subject route (document, kernel, solver, jobs,
  export) is touched.
* Linux gate executed: impossible on this Mac; compiled and linted only.

## Author handoff: not done, not claimed

* No GUI pass: the author ran no window, viewer, bundle, CUA or GPU work. The independent
  recipe below is the window part.
* The Linux real-process gate (`a_window_that_cannot_open_is_kept_in_the_launch_journal`,
  `FCAD_LAUNCH_JOURNAL_WINDOW_FAILURE_EXECUTED`) cannot run on this Mac: it was
  compiled and linted here through a temporary local `cfg` swap (restored byte for byte),
  not executed; it runs only in CI on Linux.
* The base's CI is the base's: nothing here has CI of its own.
* The whole true-stub suite and the final-tree repeat are pending (pressure, above).
* §30, Milestone 5C, the product and the cause of the earlier OOM stay open.

## Independent review

The first review found that a failed segment read or deletion still removed its lock
file. That made the leftover segment permanently unrecognisable to retention. Cleanup
now keeps the lock on an I/O refusal, and the existing retention gate also checks a
retry after access is restored: Unix denies reading the segment header; Windows makes
the segment read-only so removal fails. Unrelated links and directories remain untouched.

On the review tree, fmt, workspace clippy with all targets/features and `-D warnings`,
licence headers (505 files), export boundary, actionlint and whitespace checks passed.
The initial attempt to execute the retention regression was refused before starting a
process: system memory pressure was 2. It is not a test pass. Final local execution,
the window recipe and remote CI are pending; review evidence goes in the ignored
`target/launch-journal-review/`, separate from the author's logs.

## Independent macOS window recipe (the author does not run it)

The reviewer's own fresh arm64 bundle `APP`, staged by `tools/stage-runtime-layout.sh`
and accepted by the readers in `tools/macos-bundle.sh`, as in earlier recipes. Memory
pressure normal (`sysctl kern.memorystatus_vm_pressure_level` = 1) before each run; every
viewer run only through `tools/watch-viewer-memory.py --limit-mib 1536 --seconds <limit>`
(the watchdog owns one process, so instances are run one after another here; concurrent
instances are covered by the process gates). Record the window, stderr and the journal as
they are; nothing below predicts what the window shows.

```sh
source <the native env of earlier recipes>
ROOT="$(mktemp -d "${TMPDIR:-/tmp}/ferrite-31h-journal.XXXXXX")"
mkdir -p "$ROOT/run"
"$APP/Contents/MacOS/ferritecad" create "$ROOT/run/plate.fcad" --sample --size 40 20 5
export FERRITECAD_RECOVERY_DIR="$ROOT/recovery" FERRITECAD_TABS_DIR="$ROOT/tabs"
export FERRITECAD_LOG_DIR="$ROOT/logs"
```

1. **Missing input file.** `python3 tools/watch-viewer-memory.py --log "$ROOT/a.jsonl"
   --limit-mib 1536 --seconds 600 -- "$APP/Contents/MacOS/ferritecad-viewer"
   "$ROOT/run/missing.fcad"` (the file does not exist). Note what the window shows, then
   Quit (⌘Q). Keep `a.stdout`, `a.stderr`, the exit status and `ls -l "$ROOT/logs"`.
2. **A model that opens.** The same with `b.jsonl` and `"$ROOT/run/plate.fcad"`; orbit
   or click once, then Quit. Keep the same evidence.
3. **The default folder.** `unset FERRITECAD_LOG_DIR`, run once more with no document
   (`c.jsonl`), Quit, and list `~/Library/Logs/FerriteCAD` (Console → Log Reports may also
   show it). The new instance's files there are the reviewer's own and may be removed
   afterwards.
4. **Check against the contract.** Per run: exactly one new instance (`….lock` + `….1.log`,
   `0600`, folder `0700` if it was made); the header names the bundle's version,
   `os=macos arch=aarch64` and the PID the watchdog recorded; each `ferritecad: …` line of
   that run's stderr appears once as an `error` record with the same text, and there is no
   other `error` record; after Quit an `exit` record; no model contents, geometry, frames,
   pointer events or environment; run 1's files are still present after run 2 (four
   finished instances are kept). Note the exit status, peak footprint, pressure and swap
   from each watchdog log. Do not call the OOM investigation closed.

## Manifest

Branch `viewer-launch-journal` at base `062e467c…` (no commits; index empty; no stash).

Modified: `.github/workflows/ci.yml` (+36), `crates/ferritecad-app/src/last_tabs.rs` (±2),
`crates/ferritecad-app/src/main.rs` (+26/−16), `crates/ferritecad-app/src/sessions/tests/tabs/restore.rs`
(+197), `crates/ferritecad-app/tests/solver_info.rs` (+125), `docs/implementation-plan.md` (+24/−1).
Untracked: `crates/ferritecad-app/src/journal.rs`, `crates/ferritecad-app/src/journal/tests.rs`,
`docs/viewer-launch-journal.md`.
