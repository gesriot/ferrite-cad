# §30R — reopen the saved files of the last window

Markers `FCAD_30R_*`. Builds on [§30Q](open-new-recover-with-drafts.md),
[§30P](tab-edit-drafts.md), [§30O document tabs](document-tabs.md),
[§30M crash recovery](document-crash-recovery.md) and
[ADR 0005, §30R](decisions/0005-document-session.md#30r-reopen-the-saved-files-of-the-last-window).
This one file is the contract and the verification record of the slice.

## What a person can do

Open A, B and C, work, save what matters and Quit. On the next start the window
offers **Reopen saved files** with the names of the files that were open. One press
opens them again, in the old order, through the ordinary Open, and shows the tab that
was shown, when it is still there. What opens is each file **as it is saved on disk
now**: changes that were discarded or never saved are not part of it, and an Untitled
document that was never saved is not in the list. **Not now** opens nothing and changes
nothing. A crash copy is still offered separately under *Recover unsaved work*; the two
offers never decide for each other.

## Decided before implementation: events and owners

**Policy.** One descriptor file, `last-window`, describes the set of saved files of the
last window that went through Quit to the end. It is replaced whole by an atomic
rename: the next start reads either the previous complete list or the new complete
one. With two independent windows (two processes) the last one to finish its Quit
publishes its own whole list; lists are never merged. Each publication writes its own
uniquely named temporary file (`.last-window.<uuid>.partial`, created exclusively), so
no process removes or overwrites another's. No workspace, journal or synchronisation.

| Event | Owner of the decision | Descriptor |
|---|---|---|
| Quit pass reaches its end: every unsaved tab saved or answered Discard, no form (shown or set aside for New), no worker | `Tabs::quit_step` = `Exit`; `main::end_window_quit` takes the set from `Tabs::saved_set` (row order, each tab's `Sessions::logical_path`, the shown tab) and publishes it through `last_tabs::Folder::publish` **before** `decide_exit` and `event_loop.exit()` | replaced by the new whole list (also an empty one) |
| Quit refused or stopped: Cancel at any tab, a cancelled or failed Save, a form open or set aside, a worker, a tab that could not be shown | `begin_window_quit`, `continue_quit` (`Tabs::abort_quit`) | untouched |
| `end_window_quit` reached while no pass is at `Exit` (a late continuation) | `end_window_quit` asks `Tabs::quitting` and `quit_step` itself | untouched |
| Publication fails (folder, permissions, a destination that is not ours, a path the format cannot hold) | `end_window_quit` | old list whole; Quit stops **once** with the reason, nothing closed; the next Quit tries again and ends the window even if it fails again (stderr) |
| No folder known at all (no home, no `LOCALAPPDATA`) | `Restores::new` | nothing written; the window says reopening is off; Quit is not held |
| Window ends without a decision (a frame or surface that failed, a crash, a kill) | `App::exiting` | untouched: the previous list stays offered |
| Start | `App::resumed` reads the descriptor on a thread → `Restores::listed` | read only; no folder is made, nothing written |
| **Reopen saved files** | `can_restore` (one predicate, button and handler) → `Restores::begin` → `restore_step` → `App::start_reading` (the existing `Loads` + `open_for_view` worker) → `Bind::Open` in `present` | read only |
| A reading's answer | `Loaded` handler → `Restores::answered` (only the generation it waits for, only when `Loads` accepted it) → next `restore_step` | read only |
| Cancel of the reading in flight | `cancel_load` + `Restores::cancel` | read only |
| **Not now** | `Restores::later` | nothing |

**What is in the list.** Tabs in row order, each tab's logical path (absolute, as the
session holds it); Untitled tabs are skipped; the shown tab's index, or none when the
shown tab is Untitled or there is none. A named tab answered Discard is listed: what
reopens is its saved file. An Untitled tab saved through Save As during the pass is
listed under the path that save published. Tabs closed before Quit are gone.

**Restore.** A queue owned by `restores::Restores`, driven one file at a time through
the one foreground route (`Loads`/`open_for_view`/`Bind::Open`): no second session
type, Open, parser or GPU scene. A file a tab already names (by any name) is *already
open* and not read again; two physical copies with one `DocumentId` are two tabs. The
eight-tab limit refuses the rest of the queue in words. Each new tab is inserted after
the shown one, so successes keep the list's order; the shown tab's forms stay with it
(`Tabs::open`). At the end the window shows the old shown file's tab when it is open;
otherwise the first file of the list that is open; otherwise nothing changes. While the
queue runs, Open, New, Recover, tab changes and Quit wait (a reading is in flight); the
Open button and its handler ask `can_open`, which also waits for the queue.

## Rules

1. **What is kept.** `last_tabs::LastTabs`: the paths and the shown index — no model,
   document Undo, checkpoint state, form draft, camera, selection or copy of any file.
   It is not a document operation: it dirties nothing, is not in Undo, checkpoints or a
   crash copy, and no `.fcad`, schema, capability, JSON v1 or CLI command changes.
2. **Folder.** `~/Library/Application Support/FerriteCAD/Tabs` (macOS),
   `%LOCALAPPDATA%\FerriteCAD\Tabs` (Windows), `$XDG_STATE_HOME/ferritecad/tabs` or
   `~/.local/state/ferritecad/tabs` (Linux, other Unix) — the recovery folder's own
   platform rule (`ferritecad_jobs::per_user_state_folder`, also used by
   `default_recovery_root`, whose words are unchanged). `FERRITECAD_TABS_DIR` names
   another folder (tests, the window recipe). Never the system temporary directory, a
   checkout or a document's folder. Made `0700` at the first publication; the
   descriptor is `0600`. Reading never makes the folder.
3. **Format v1.** Text lines, each ending in a newline, nothing after the last:
   `FERRITECAD-LAST-TABS 1`, `platform unix|windows`, `count <n>` (0–8, no sign, no
   leading zero), `active <i>|none` (`i < n`), then exactly `n` lines `path <hex>`: the
   path's native units in lowercase hexadecimal (Unix bytes; Windows UTF-16 units), at
   most 128 Ki digits, decoded to an absolute path with no NUL unit. The whole file is
   at most `MAX_BYTES` (1 048 688) and is refused from its size before it is read. Any
   other first line is not a list; another version number is `unknown-version`; any
   other deviation (cut short, extra lines, another platform, a relative or malformed
   path, too many files) is `damaged`.
4. **Native paths.** Paths are compared and opened exactly as stored: Unicode, spaces,
   a newline in a name and names that are not UTF-8 (Unix) or not valid UTF-16
   (Windows) round-trip unit for unit. Names are shown to a person lossily (display
   only); the path opened is the exact one. A path is handed only to Open, which reads
   a `.fcad` through `DocumentSession::open_in`: no command, URL, import or migration of
   the user's file runs (a newer-schema migration happens only in the private copy, as
   for any Open).
5. **Reading.** At start, on a thread, `Folder::read`: no folder or no descriptor is no
   offer; an empty list is no offer. A link or a non-file in the descriptor's (or the
   folder's) place is `foreign` and is not followed. The entry is looked at again after
   it is opened: the file read must be the plain file the name holds; a list replaced
   by another window's publication in between is read again (at most three times).
   Unreadable is `io`. Every refusal is shown in words beside the offer area (*was not
   used (kind): … Nothing was opened or changed.*); the file is left as it is and no
   list is invented.
6. **Publication.** `Folder::publish`: the list is checked first (absolute, at most 8,
   index in range, path length) and refused before any I/O; a link or non-file at the
   destination, or a folder that is a link or a file, is refused and left alone. An
   existing descriptor must also pass the same bounded format reader: unknown versions,
   foreign text, damaged and unreadable lists are kept rather than overwritten at Quit. Then
   `.last-window.<uuid>.partial` is created exclusively, written, synced and renamed
   over `last-window`, and the folder is synced (Unix). A failure removes only this
   publication's own temporary file (a removal that fails is added to the error); other
   files, other processes' temporary files and leftovers of an interrupted publication
   are never touched or read. A failed folder sync after the rename is said on stderr:
   the list is in place, and power-loss durability is not claimed.
7. **Quit.** As decided above: `end_window_quit` publishes only at the real end of the
   pass, then decides every tab's end (crash copies retire as before). A list that
   cannot be published stops the pass once with *"The list of open files for the next
   start could not be kept: … Nothing was closed. Quit again to end the window anyway;
   the list is tried once more."*; the earlier list stays whole. The macOS Quit hook
   and window close reach the same pass. The recovery lease protocol, PR #96's explicit
   unlock and PR #97's cleanup skip of published records are untouched.
8. **Reopen.** Offered with each file's name, folder and the shown mark, and the words
   *"Reopening opens each file as it is saved on disk now. Changes that were discarded
   or never saved are not part of it; work a crash interrupted is offered under Recover
   unsaved work."* **Reopen saved files** asks `can_restore` (= `can_recover` + an offer
   and no Reopen running) in the button and the handler. Per file: `Opened`,
   `AlreadyOpen` (a tab names it by any name: not read again), `Failed(why)` (unreadable,
   damaged, unsupported or newer document, kernel or device refusal of its picture,
   refusal at the bind), `NotOpened(why)` (the window full — the rest of the queue too —
   or cancelled). The account — *"k of n saved files are open, each as it is saved on
   disk now."* and one line per file not opened — is kept until **Not now** or the next
   Reopen; a file that opens never hides one that did not. Reopen again opens only what
   is not open. **Not now** hides the offer for this run and is not offered while a
   Reopen runs (Cancel stops that).
9. **Order and the shown tab.** Each opened file becomes a tab after the tab shown at
   that moment, so one Reopen keeps the list's order; at the end the old shown file's
   tab is shown when it is open (by a switch through the existing worker), otherwise the
   first listed file that is open, otherwise nothing moves. Cancel shows nothing more.
10. **Concurrency.** One foreground reading at a time: the Reopen waits for its own
    generation; `Loads` rejects anything else as before, and `Restores::answered` takes
    only the generation it waits for — an answer `Loads` did not accept (cancelled or
    replaced) ends the Reopen. While the queue runs, `can_open` refuses Open (a newer
    Open would replace its reading); New, Recover, tab changes, Close and Quit already
    wait for the reading in flight. Existing tabs, their forms and drafts (§30P/§30Q),
    dirty state, Undo, the 8-tab limit, alias deduplication and generations are
    unchanged.

## Found defects

* Independent review reproduced two publication bugs with executed failing assertions:
  a rejected descriptor (including foreign text and unknown versions) was overwritten
  by Quit, and a failed exclusive temporary-file creation removed the pre-existing
  file at that name. Publication now validates the existing descriptor before writing;
  cleanup starts only after exclusive creation succeeds and closes the handle first.
  The existing refusal gate checks both reading and publishing, and the publication
  gate exercises a temporary-name collision plus a rename failure after an owned write.
  Previous descriptor bytes, foreign bytes and unrelated files remain intact.

* **A reading refused a whole list during another window's publication** (found while
  verifying, fixed). The first reader compared the entry looked at *before* opening with
  the file opened; a rename landing between the two made it say *"changed while it was
  being opened"* (`foreign`) about a perfectly whole newer list. It showed in the full
  stub app suite (the two-process test, run in parallel with the other tests) and in 1
  of 5 isolated runs of that test; raising its rounds to 600 did not make it
  deterministic (4 of 4 passed on the old code), so there is no deterministic failing
  regression for it — the evidence is that log, kept, and 10 of 10 isolated runs plus the
  full parallel stub and native suites after the fix. The fix compares the opened file
  with the entry looked at *after* opening (a link put in its place is still refused) and
  reads again, at most three times, when another publication replaced it.
* Test-only: a first comparator compared listed paths with the copied root of a
  negative control; lists are now compared by name and one folder, and the folder is
  checked once on the root the windows ran in.

## Not here

Form drafts, Undo or camera across a restart; restoring after a crash or a kill (the
previous list stays offered, crash copies are §30M's); a set per window when several
windows run (the last to quit wins); a workspace database, synchronisation or journal; a
CLI for window tabs. §30, Milestone 5C and the product remain open; nothing here closes
the earlier OOM investigation; no next slice has started.

## Verification

### What is checked, and where

**Kernel-free, every OS (`ci.yml`, step *Open, edit, Undo, Redo and Save one document
without native geometry*, exact name, `--exact`, no `skipped:`, one marker line each):**

* `last_tabs::tests::a_published_list_reads_back_exactly_and_reading_writes_nothing`
  (`FCAD_30R_DESCRIPTOR_ROUND_TRIP_EXECUTED`) — no folder: no list and no folder made;
  three paths (Cyrillic, spaces, two `a.fcad` in two folders) back exactly; modes 0700 /
  0600; three readings change no byte, name or time; a new list replaces the old whole;
  an empty list is a list, byte-exact; no temporary file left.
* `last_tabs::tests::damaged_foreign_or_unknown_lists_are_refused_and_left_as_they_are`
  (`FCAD_30R_DESCRIPTOR_REFUSALS_EXECUTED`) — 18 byte-level cases (empty, cut in the
  magic, version 2, version `x`, foreign text, no last newline, a path line missing, an
  extra line, nine files, index out of range, leading zero, upper-case and odd digits, a
  relative path, a NUL unit, another platform, not UTF-8, an over-long path) each with
  its kind and the folder unchanged; a sparse file one byte over the limit refused from
  its size; a folder in the descriptor's place; (Unix) a link to a user's file refused
  for reading and publishing with the file unchanged, a linked folder refused with
  nothing written there, an unreadable list (`io`, file kept; N/A where modes are not
  enforced, e.g. root).
* `last_tabs::tests::a_failed_publication_keeps_the_previous_list_whole_and_touches_nothing_else`
  (`FCAD_30R_FAILED_PUBLICATION_EXECUTED`) — a relative, an over-long, nine, and an
  out-of-range-index list each refused before I/O with the old list byte for byte and
  no temporary file; a folder that is a file and a folder under a file; (Unix) a folder
  that cannot be written (N/A when modes are not enforced); another process's or an
  interrupted publication's `.partial` and an unrelated file never read or touched, also
  by a later successful publication.
* `last_tabs::tests::paths_keep_their_native_units_exactly`
  (`FCAD_30R_NATIVE_PATHS_EXECUTED`) — spaces, Japanese, a newline (Unix), bytes not
  UTF-8 (Unix) or an unpaired surrogate (Windows) round-trip unit for unit; real files
  with the Unicode names are found by the paths read back. A non-UTF-8 name on disk is
  N/A on APFS (`Illegal byte sequence`), printed as `FCAD_30R_NATIVE_NAME_ON_DISK_N/A`.
* `last_tabs::tests::two_processes_publish_whole_lists_and_one_of_them_stays`
  (`FCAD_30R_TWO_PROCESSES_EXECUTED`) — this test binary re-run twice as children
  (`publisher_child`, ignored unless run so) publishing 200 times each, an eight-path
  and a three-path list of ~1 KiB paths, into one folder while the parent reads in a
  loop: every reading is no list yet or one of the two whole lists; the last one stays;
  no temporary file is left. On Windows a rename refused by a concurrent rename or a
  read refused by a pending rename is counted, never a mixed list.
* `sessions::tests::tabs::restore::quit_keeps_the_saved_files_only_when_the_window_really_ends`
  (`FCAD_30R_QUIT_PUBLICATION_EXECUTED`) — through `begin_window_quit`, `Tabs`,
  `Sessions`, `present`, `end_window_quit`: an earlier window's list stays byte for byte
  through a pass given up at a hidden tab's form *with the form since closed and every
  tab clean* (M1's target), a running worker, a form on the shown tab, a pass stopped at
  a hidden form after a Discard, Cancel, a refused Save As, New's set-aside forms; the
  real end (Discard Untitled U2, Discard named A after its 14 mm Apply, Save As U1 during
  the pass, C closed before Quit) publishes `[a, b, u1]`, U1 shown; a repeated end
  publishes nothing; every input byte unchanged; the next lifecycle's offer is exactly
  that list and Reopen gives A at 12 mm (its saved file), clean.
* `…::restore::an_empty_end_clears_the_offer_and_a_list_that_cannot_be_kept_stops_quit_once`
  (`FCAD_30R_EMPTY_AND_FAILED_LIST_EXECUTED`) — an Untitled-only window publishes an
  empty list and no offer remains; a folder that is a file: the first end stays with the
  words, nothing closed or decided, the pass stopped; the next Quit ends the window.
* `…::restore::reopen_restores_saved_files_beside_open_tabs_in_order_and_shows_the_last_shown`
  (`FCAD_30R_REOPEN_EXECUTED`) — X open, dirty, with the literal height `2..6`; list
  `[a, missing, x-alias (hard link), broken, d, d copy]`, d shown: while each file is
  read, Open, Reopen, New/Recover/tab row wait; A and D open after X in order, missing
  and broken said by name, the alias is X (not read again), the device refuses D copy's
  picture; D shown; X keeps `2..6`, dirty, 11 mm; Reopen again opens only D copy (one
  `DocumentId`, its own tab, after the tab shown) and shows D; a third Reopen adds
  nothing; inputs and the list unchanged; Not now hides everything and removes nothing.
* `…::restore::cancel_late_answers_and_a_full_window_never_revive_or_mix_a_reopen`
  (`FCAD_30R_CANCEL_LATE_FULL_EXECUTED`) — Cancel while B is read: A stays, B and C *not
  opened: cancelled*, Open free again; a new Reopen reads B again and the cancelled
  reading's late answer arrives: no tab, the new Reopen still waits for its own (M2's
  target); a foreign generation changes nothing; B shown at the end; with seven tabs
  open one file fits and the rest are refused with the 8-tab words; the shown file did
  not open, so the first listed open file stays shown.
* `…::restore::stub_reopen_is_refused_at_the_picture_and_keeps_the_open_tab`
  (`FCAD_30R_STUB_REOPEN_EXECUTED`, executes only without a kernel) — keeping the list
  needs no kernel; the production `open_for_view` refuses each file; the open tab and
  its form stay; each file is said.
* `…::restore::reopen_waits_for_a_gesture_and_is_offered_only_with_a_list` (no marker)
  and the real-egui `reopen::tests::reopen_asks_nothing_while_the_window_says_it_cannot_and_says_what_opens`
  (`ferritecad-ui`): the predicate and the panel's buttons; a refused list is said.

**No solver (`runtime-layout.yml`, OCCT without PlaneGCS):**
`…::restore::mixed_reopen_with_occt_and_no_solver` (`FCAD_30R_MIXED_REOPEN_EXECUTED`).

**Native (`runtime-layout.yml`, OCCT + PlaneGCS):**
`…::restore::native_reopen_saved_tabs_then_apply_save_and_compare_with_the_command_line`
(`FCAD_30R_NATIVE_REOPEN_EXECUTED volume_a=83200.000 volume_b=…`,
`FCAD_30R_SESSION_FILES_COMPARE_OK negative_controls=8 all_SQL_cells=true`) runs the
window recipe below on the owners with the production workers — **not window
evidence** — and the recipe's comparator: both lists by name, one folder, the shown
index (and, on the root the windows ran in, that the folder is the root); `a.fcad`
equals the CLI's `edit-extrude 26` and `b.fcad` the CLI's `edit-circle` (−3.5, 4.25,
r 8) in every SQL cell but `meta.modified_at` (the one stamp every writer refreshes — the
existing `all_sql` allowlist), ids and references equal the inputs'; `b-unsaved.stl/.fbx`
byte-equal the CLI's exports of that copy and the saved `b.fcad` exports the same bytes;
A 80 × 40 × 26 = 83 200 mm³ and B round (centre, radius, height 15.25) by an independent
STL reading; the recovery folder is empty. Eight controls on copies, each refused for
its own reason: a missing output (no peer job ran), `a.fcad` or `b.fcad` never saved, the
two documents swapped, a hand-made first list, the first list left as the last one, a
truncated last list, swapped exports. `reopen-b` (the window's unsaved export) and
`reopen-a` (the CLI's export of the saved A) go to the pinned ufbx block of
`tools/check-fbx-complex.sh` (`FCAD_RESTORE_SAVED_TABS_UFBX_EXECUTED`).
`…::restore::native_compare_real_reopen_gui_artifacts_with_negative_controls` runs the
same comparator on real window outputs in `FCAD_30R_GUI_DIR`; without the variable it
does nothing.

### Execution record (author, macOS arm64, 2026-10-09)

Base: `main` = `origin/main` = `20c6f7fe4d50716ca616e5d5fd44620e0e671f03` (PR #97 merged
17:56:08 UTC). At the start of this slice its post-merge `rust sbom` and `product sbom`
had succeeded and CI, rust notices, planegcs pin and combined runtime layout were in
progress; when this record was written CI, planegcs pin, rust notices, rust sbom and
product sbom had succeeded on that commit and **combined runtime layout was still in
progress — not counted as passed**. Branch `restore-saved-tabs`, uncommitted; **no
remote CI has run for this diff.** `rustc 1.96.0`, `CARGO_BUILD_JOBS=2`. Vendor OCCT and
PlaneGCS were not rebuilt. No viewer, bundle, GPU window, `osascript` or browser was
run. Memory pressure stayed normal (55–61 % free), swap unchanged at 1018.56 MiB,
142 GiB disk free.

* **Native** (`source /private/tmp/ferrite-pr93-review/env.sh`, target
  `/private/tmp/ferrite-24b-native-target`): `cargo fmt --all -- --check` clean; `cargo
  clippy --workspace --all-targets --all-features -- -D warnings` clean;
  `ferritecad-app --features planegcs` **586 passed, 2 ignored** (the pre-existing one and
  `publisher_child`, which only its parent runs) + 3 integration, 0 `skipped:`;
  `ferritecad-jobs --features planegcs` 120 passed, 1 ignored (pre-existing);
  `ferritecad-ui` 109 passed; `ferritecad-cli --test recovery` 4 passed. In that build
  the stub-only and no-solver-only gates and the env-gated GUI comparator return
  without executing: N/A, not passes.
* **Runtime-layout, exact lines** (extracted from the edited YAML, run by `/bin/bash`
  after `source env.sh`; only `RUNNER_TEMP`, `GITHUB_ENV` and the matrix name supplied;
  release viewer and peer CLI built first from this tree): the native tabs block exit 0
  with every §30O/§30P/§30Q marker and `FCAD_30R_NATIVE_REOPEN_EXECUTED
  volume_a=83200.000 volume_b=3063.648`, `FCAD_30R_SESSION_FILES_COMPARE_OK
  negative_controls=8 all_SQL_cells=true`, 18 artifacts, four `GITHUB_ENV` lines; the
  §30R no-solver lines (`FERRITECAD_REQUIRE_PLANEGCS=0`, no `planegcs` feature) exit 0
  with `FCAD_30R_MIXED_REOPEN_EXECUTED`.
* **Pinned ufbx:** the §30R block of `tools/check-fbx-complex.sh` with the script's
  preamble and reader build unchanged (ufbx 0.23.0 `fcc5d6b…`, cached, strict; only
  `root` pointed at the checkout because the composed script ran from an ignored
  folder): `checks=6 failures=0` for `reopen-b` (STL↔FBX 352 triangles) and `reopen-a`
  (12), `FCAD_RESTORE_SAVED_TABS_UFBX_EXECUTED`. The script's complex-STEP part was not
  rerun.
* **True stub** (target `/private/tmp/ferrite-25j-stub-target`, `env -i`: no `DYLD_*`,
  no `FCAD_PLANEGCS_DIR`, `OpenCASCADE_DIR` and `CMAKE_PREFIX_PATH` =
  `/private/tmp/absent-occt-true-stub`, `FERRITECAD_REQUIRE_OCCT=0`,
  `FERRITECAD_REQUIRE_PLANEGCS=0`): bridge directory `ferritecad-occt-bd6e1e14777ad9eb`,
  whose cache holds `CMAKE_DISABLE_FIND_PACKAGE_OpenCASCADE=TRUE`, warning *"Open
  CASCADE was not usable … configuring the bridge failed"*; `otool -L` of the app test
  binary shows no `libTK*` and no PlaneGCS.
  * The exact `run:` block of the edited `ci.yml` step, extracted and run with
    `RUNNER_OS=macOS` from an ignored directory, after the last code change: exit 0,
    214 `test result: ok` lines (202 before this slice + 12), 0 `skipped:`, all ten §30R
    markers with the §30O/§30P/§30Q ones; `FCAD_30R_NATIVE_NAME_ON_DISK_N/A` (APFS).
  * Stub `ferritecad-app` suite (after the reader fix): 586 passed, 2 ignored, 3
    integration, 0 `skipped:`.
  * Stub `cargo clippy --workspace --all-targets -- -D warnings`: clean.
* `tools/check-export-boundary.sh` passes; `tools/check-licence-headers.sh`: 464 tracked
  files, all MIT (it reads `git ls-files`, so the six new code files were checked by
  hand: all carry the MIT header); `actionlint` (with shellcheck) on `ci.yml` and
  `runtime-layout.yml` clean; `shellcheck tools/check-fbx-complex.sh` clean; `git diff
  --check` clean, no trailing whitespace in new files.
* `tools/restore-saved-tabs-gui.py`: a root inside the checkout is refused
  (`FCAD_30R_GUI_FIXTURE_REFUSED`, nothing made); a root outside made only `a.fcad`,
  `b.fcad`, `inputs/`, an empty `recovery/` and `facts.json` (no `tabs/`); `--between`
  refused before a first window had saved A; `--compare` refused before any build on
  missing outputs. On a separate probe root (removed afterwards; a hand-written list
  standing for a window's, to exercise the tool only) `--between` accepted and copied
  once and refused a second run.

**Directed mutations** (native debug, on the formatted source; both compile and fail an
executed assertion; restored byte for byte — `main.rs` `7332652f…103a4acd`, `restores.rs`
`4e598d3e…127443` before and after — then the nine restore tests passed again):

* **M1 — publication after a given-up Quit.** `end_window_quit` no longer asks whether a
  Quit pass is under way (`!tabs.quitting() ||` removed). `quit_keeps_…` fails at its
  first executed check of that case: after a pass stopped at B's form, the form closed
  and every tab clean, a late end returned `Exit` (it published and decided the tabs)
  instead of `NotEnded`.
* **M2 — a late answer accepted after Cancel.** `Restores::answered` takes any
  generation (`take_if(… == generation)` → `take()`). `cancel_late_…` fails: the
  cancelled reading's late answer ended the new Reopen (*"the late answer did not end
  this Reopen"*).

Logs are in the author's checkout under the ignored `target/30r-logs`,
`target/30r-ci` and `target/30r-mutations`, not in the repository.

### Real window recipe (macOS)

Not run by the author. One freshly staged arm64 bundle `APP`, run twice in turn, each
time under the 1536 MiB watchdog, from the checkout in the native environment
(`source <env.sh>`: `--compare` runs the Rust comparator). A new root outside any
checkout; the recovery and tabs folders are the root's own — the person's own folders
are never touched. The list between the two launches is the first window's own: never
written by hand. Never call `getApp`/`getAX` after a Quit (it may start the viewer
outside the watchdog); check only that the owned PID is gone.

```sh
ROOT=/private/tmp/ferrite-30r-window-review
FERRITECAD="$APP/Contents/MacOS/ferritecad" python3 tools/restore-saved-tabs-gui.py "$ROOT"
export FERRITECAD_RECOVERY_DIR="$ROOT/recovery" FERRITECAD_TABS_DIR="$ROOT/tabs"
python3 tools/watch-viewer-memory.py --log "$ROOT/watch-1.jsonl" --limit-mib 1536 \
  -- "$APP/Contents/MacOS/ferritecad-viewer" "$ROOT/a.fcad"
```

First launch:

1. `a.fcad` opens. No *Files open when FerriteCAD was last quit* section and no recovery
   list.
2. **Open…** `$ROOT/b.fcad`: a second tab.
3. Press `a.fcad`. **Edit extrusion…** → choose the extrusion → `26` → **Apply**
   (`*a.fcad`). **Save** (`Cmd+S`).
4. Quit (`Cmd+Q`) with `a.fcad` shown: nothing is asked; the window ends; check the owned
   PID is gone.

```sh
python3 tools/restore-saved-tabs-gui.py --between "$ROOT"     # FCAD_30R_GUI_BETWEEN_OK
python3 tools/watch-viewer-memory.py --log "$ROOT/watch-2.jsonl" --limit-mib 1536 \
  -- "$APP/Contents/MacOS/ferritecad-viewer"
```

Second launch (no document named):

5. An empty window with the section *Files open when FerriteCAD was last quit*:
   `a.fcad (shown) — $ROOT`, `b.fcad — $ROOT`, the sentence that files open as saved on
   disk now, **Reopen saved files** and **Not now**. No recovery list.
6. **Reopen saved files**: `a.fcad` then `b.fcad` open in that order and `a.fcad` is
   shown; *2 of 2 saved files are open, each as it is saved on disk now.* A is 26 mm.
7. **Reopen saved files** again: no new tab; still 2 of 2.
8. Press `b.fcad`. **Edit circle …** → centre −3.5, 4.25, radius 8 → **Apply circle**
   (`*b.fcad`).
9. **Export STL…** → `$ROOT/b-unsaved.stl`; **Export FBX…** → `$ROOT/b-unsaved.fbx`.
   **Save** (`Cmd+S`).
10. Quit (`Cmd+Q`) with `b.fcad` shown: nothing is asked; check the owned PID is gone.

```sh
python3 tools/restore-saved-tabs-gui.py --compare "$ROOT"
```

`--between` refuses (`FCAD_30R_GUI_BETWEEN_REFUSED`) unless the first window saved A, left
B unwritten and kept `[a.fcad (shown), b.fcad]`; it copies that list to `first-window`
once. `--compare` refuses (`FCAD_30R_GUI_COMPARE_REFUSED`) before any build or peer job
when an input changed, an output is missing or a document was never saved; the Rust
comparator refuses a missing output again before its first peer job, checks that both
lists name files in the root, and requires `FCAD_30R_GUI_COMPARE_OK negative_controls=8
all_SQL_cells=true`. Files alone cannot prove that step 6 used Reopen rather than two
Opens; the reviewer's eyes on steps 5–7 are that part of the evidence.

### Limits

* No real window was run by the author; the owners' scenario and the headless widget
  test are not window evidence. Windows/Linux window interaction is untested; their
  descriptor, owner and process gates run in CI only after this diff is pushed.
* Publication runs on the event loop at the end of Quit (a few hundred bytes, one
  `fsync`, one rename); reading runs on a thread at start.
* Ownership of the folder is not checked by user id (no new dependency); it is created
  `0700`, and links and non-files are refused.
* The race found above has probabilistic, not deterministic, failing evidence.
* With two windows, the one that finishes its Quit last decides the next offer; the
  other's set is not kept.
