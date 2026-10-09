# §30S — reading the recovery folder is not owning a record

Marker `FCAD_30S_RECOVERY_INSPECTION`. Builds on
[document-crash-recovery.md](document-crash-recovery.md) (§30M, rule 6 and 12),
[open-new-recover-with-drafts.md](open-new-recover-with-drafts.md) (§30Q, the cleanup
sweep) and [ADR 0005, §30S](decisions/0005-document-session.md#30s-reading-the-recovery-folder-is-not-owning-a-record).
This one file is the contract and its verification; nothing else was split off.

## The defect, as found on the base

Base: `main` = merge of PR #98, `513d9758bbcc8b70b5244f99e4e86bef0ebd9c41`, tree
`5aecbe626eb310b4fb7858f4ac779ce1c44c652e` (the tree of the reviewed PR head
`2d944ac5`).

`RecoveryStore::list` called `inspect`, and `inspect` is what `claim` calls: it takes
the record's **exclusive** `lease` lock and verifies the manifest, the copy's length and
BLAKE3 and the SQLite document *under it*. So for as long as one record was being
read for a listing, a Recover, an `extract-recovery`, a Delete, or another listing of
the same record found the lease held and answered `Active` — "belongs to a FerriteCAD
window that is still running" — although the only other party was somebody reading.

The cleanup sweep had the same shape and was fixed in PR #97 (it no longer locks a
record that has a manifest). That stays. The explicit paths — `list`, `claim`,
`delete` — still shared one lock mode for owning and for looking.

## Owners and locks

The `lease` file's advisory lock is the only ownership primitive (no PID, host, age,
registry or daemon). Before and after this slice:

| Operation | Takes on `lease` | Held for | A refusal means | Retry |
|---|---|---|---|---|
| **owner** (`create_record` → `RecoveryRecord`) | exclusive, at creation | the record's life; the OS lets go at process end | a new name is tried (3×) when a reader sneaks onto the brand-new file | none |
| **list / probe** — before | exclusive | the whole verification of one record | `active` was counted although only a reader held it | — |
| **list / probe** — now | **shared** | the verification of that one record, released before the next | exclusive holder proven (the shared lock was refused): counted `active` | none; advisory, a new listing re-reads |
| **claim / extract** — before | exclusive (try) | until dropped, finished or adopted | `active` for anybody, readers included | — |
| **claim / extract** — now | exclusive (try); on refusal a *shared* try tells readers from owners | until dropped, finished or adopted | `active`: an exclusive holder (window, claim, removal) was proven. `busy`: only readers, still there after the bound. `cancelled`: the caller stopped waiting | readers only: poll every 5 ms for at most 5 s, stops at once when cancelled; verification is repeated under the exclusive lock |
| **adoption** (`into_record`) | the claim's exclusive lock, kept | the adopting session's life | — | none |
| **delete** | exclusive (try), same readers rule as claim | until the record is gone | as claim | as claim |
| **cleanup sweep** (PR #97) | nothing for a record with a manifest; exclusive (try) on a record without one, re-checked under it | the removal | anything unavailable counts as kept | none |
| **cancel / drop / failure** | releases only the lock its own value holds, by explicit `unlock` (PR #96), also through a duplicated descriptor | — | — | — |

Why a shared lock for reading, and not a lock-free read: a shared lock cannot be
granted while an exclusive holder exists, so a listing that holds it *knows* there is
no owner, claim or removal for as long as it reads, and nobody can obtain one until it
lets go. The bytes it verifies cannot be mid-publication, mid-adoption or removed under
it, so a record is never listed as damaged because it changed hands, and a live record
is never mistaken for an orphan. Two listings do not exclude each other.

What is **not** known: when an exclusive lock refuses, the primitive cannot say whether
the holder is a window, a claim or a removal. The text says exactly that; it no longer
says "a window that is still running". Old FerriteCAD builds list with an exclusive
lock; against them a claim sees an exclusive holder and answers `active`, as before
(the fix applies between builds that contain it).

## Rules

1. `list` takes a shared lock per record, only while verifying that record; it never
   writes, renames, or changes a mode or timestamp, and holds nothing after it returns.
2. `claim` and `delete` still take the exclusive lock; **a reader is waited for, a
   holder is not**. The wait is bounded (5 s, polled every 5 ms), is off the event
   loop (the Recover worker, the CLI's main thread), and ends at once when the caller
   cancels.
3. Whatever a listing showed, a claim verifies the record again under its own exclusive
   lock before anything is restored, extracted or adopted.
4. `active`, `busy` and `cancelled` are different answers; `busy` and `cancelled` are
   new and additive (`recovery_refusal` in the CLI's JSON; the exit code is the
   existing refusal's). Prose is not a contract.
5. The window: Recover is still one worker, one generation, `Bind::Open`,
   `DocumentSession`. Cancel and Quit now also tell that worker to stop waiting; a
   late or foreign answer still opens nothing, frees no newer slot and deletes
   nothing.

## Compatibility

* Library: `RecoveryStore::{list, claim, delete}` keep their signatures. New:
  `claim_cancellable(record, &CancelToken)`, `RefusalKind::{Busy, Cancelled}` and
  `#[non_exhaustive]` on `RefusalKind` (it gained variants; the workspace matches it
  nowhere exhaustively). No on-disk change: the manifest, the `lease` header, the
  record layout and the 32-record limit are as in §30M.
* CLI: `list-recovery` JSON v1 is unchanged field for field (`active` keeps its name
  and meaning "held exclusively"); the text line for `active` is reworded. A successful
  `extract-recovery` is the same JSON, exit 0 and no-clobber publication. A reader
  that stays past the bound is `error.recovery_refusal: "busy"` with the exit code of
  every refusal (2) — where the base said `active` for the same situation. A script
  that treated `active` as "retry later" should treat `busy` the same; one that matched
  the sentence "still running" was reading prose, which is not a contract. Losing
  stdout after a published extraction is still exit 7 with no rollback or retry.
* Mixed builds: an older FerriteCAD lists with an exclusive lock. A new claim then finds
  an exclusive holder and answers `active`, exactly as before; the fix is complete
  between builds that contain it.

## Gates

Every gate runs with `--exact` and fails on `skipped:`; the three that print a marker
are checked by their `test NAME ... ` line, `test result: ok. 1 passed` and the marker
on its own line.

**Kernel-free, every platform (existing `ci.yml` stub step):**

* `ferritecad-jobs --lib`, `recovery::tests::…` (seven; the Unix-only duplicated-descriptor
  test of PR #96 is a Unix-only gate beside them):
  * `a_listing_in_progress_neither_hides_the_record_nor_looks_like_its_owner` — a
    thread held *inside* the verification of an orphan; a second listing finds it
    recoverable (`active` 0), a claim is `busy` and not `active`, the held listing
    still finishes with the record, nothing was written, and the record is
    claimable afterwards.
  * `processes_that_read_claim_and_remove_an_orphan_exclude_only_the_owners` — the same
    with a **child process** as the held reader (the test binary, started by name, ready
    when it says `READING`), then two claims, a claim and a removal across processes:
    one wins, the others are `active` with the record's files unchanged; letting go
    frees it for exactly one more; removal is exclusive and final. Children are killed
    and reaped if an assertion fails.
  * `a_claim_waits_for_readers_alone_within_its_bound_and_stops_when_cancelled` — `busy`
    only after the whole bound, for claim and removal; cancelled mid-wait (30 s bound,
    answered promptly); cancelled before it began takes nothing; a holder is answered
    `active` at once whatever the bound; the refusals release nothing of the holder's.
  * `a_stale_listing_is_verified_again_by_the_claim` — a bit flipped after the listing:
    the claim says `mismatch`, a listing says the same, nothing stays locked.
  * `listings_racing_an_owner_never_see_a_record_change_under_them` — three threads
    list in a loop while an adopted claim publishes eight copies; only whole copies of
    versions that were published, or a count, are ever seen — never a refusal, never a
    mixed one.
  * `readers_share_and_are_told_from_holders_and_unlock_through_a_duplicate` — readers
    coexist; an exclusive try beside readers is `Readers`, beside an owner `Active`; on
    Unix ending a read releases it through a duplicated descriptor.
  * `cleanup_never_holds_a_published_records_lease` (PR #97) and, on Unix,
    `dropping_a_lease_unlocks_before_a_duplicated_descriptor_closes` (PR #96) — now exact
    gates, because this slice sits beside both.
* `ferritecad-cli --test recovery`: `an_extraction_waits_for_a_reader_and_says_busy_only_when_the_reader_stays`
  (the real binary; a foreign reader holds the lease shared: released after 300 ms the
  extraction succeeds with every identity of a real edited source; held past the bound
  it is `recovery_refusal: "busy"`, exit 2, no file, the record's bytes and mtimes and
  the source unchanged) and
  `listings_in_other_processes_never_stop_an_extraction_or_change_anything` (six rounds
  of eight real `list-recovery` processes around every extraction; each listing says the
  record whole or held, never anything else; every extraction exit 0 with the recorded
  document id, content version — every SQL cell, row id and reference — and model
  version; the record and the source unchanged).
* The existing `a_live_lease_is_never_offered_claimed_or_deleted_and_death_releases_it`
  (jobs integration) is the active-owner-process gate: a real child owns a record,
  listing counts it, claim and delete are `active`, killing the child frees it.

**With Open CASCADE (existing `runtime-layout.yml` no-solver step, release):** the same
owner gate below, so Recover goes through `recover_for_view`'s production claim.

**Window owners (existing `ci.yml` marker loop, and the step above):**
`recover_waiting_for_a_reader_obeys_cancel_late_answers_and_quit_beside_forms_in_two_tabs`
(`FCAD_30S_RECOVER_BESIDE_READER_EXECUTED`): two tabs each holding a typed form, a real
worker thread with the token `Recoveries::begin` hands it, a foreign reader holding the
record. Cancel makes the worker answer in under 2 s (the bound is 5 s); that late
answer opens nothing and frees no slot; a second Recover's slot is not released by the
first one's late answer; after the reader leaves the second succeeds as a new tab (21.5
high) and the forms come back as typed on switching; an owner-held record is refused
at once with nothing opened and the form intact; `Recoveries::stop_all` (Quit) returns in
under 3 s with a worker still waiting; no input file or record byte changed.

## Reproduction on the base, before any change

The production code was the base's; the only additions were a `cfg(test)` shim that can
hold a listing inside `verify` (compiled into no build but the unit tests) and the tests.

```text
a_listing_in_progress_…  (thread)      "a second listing found recoverable=0 active=1"
                                       "a claim beside a reader was refused as active:
                                        invalid input: recovery record 01a12284-… belongs to
                                        a FerriteCAD window that is still running"
processes_that_read_…    (process)     "a listing beside a reading process found
                                        recoverable=0 active=1"
                                       "a claim beside a reading process was refused as active"
ferritecad-cli --test recovery         extract-recovery (the real binary) exit 2, message "…belongs to
  (base jobs, 2 of 6 failed)           a FerriteCAD window that is still running",
                                       recovery_refusal "active" — with a foreign shared
                                       reader held, and in round 0 of the process overlap
```

Each failed at an executed assertion on the state, not at compile time or with zero
tests. The CLI run used the base's `recovery.rs` placed back for that run and restored
byte-for-byte afterwards (SHA-256 checked). The overlap test failed on its first
round, but that is chance: a race has probabilistic evidence, and the deterministic
one is the held reader.

## Directed mutations

Both compile (also under `RUSTFLAGS=-D warnings`); the file was saved first, restored
byte-for-byte (SHA-256 `3d8f941c…d155`) and the positive gates rerun green.

* **M30S-1, a claim without ownership:** `inspect` with `Hold::Own` takes the shared
  lease instead of the exclusive one. Killed by
  `a_claim_waits_for_readers_alone_…` (the claim beside a reader is granted),
  `a_listing_in_progress_…` and `processes_that_read_…` ("a claim was granted beside a
  reader"), the jobs integration `a_live_lease_is_never_offered_claimed_…` (a second
  claim succeeded) and, in the CLI suite,
  `active_damaged_and_unknown_records_are_refused_as_data` and
  `an_extraction_waits_for_a_reader_…`.
* **M30S-2, lost cancellation:** the waiting loop no longer looks at the token
  (`if false && cancel.is_cancelled()`). Killed by
  `a_claim_waits_for_readers_alone_…` (`Busy` after the full 30 s bound instead of
  `Cancelled`) and by the window gate (`cancel ended the wait long before the 5 s
  bound: Timeout`).
* **Not distinguished by these gates, not counted as killed:** `READER_POLL` 5 ms ↔ 1–10 ms; `<` ↔ `<=` on the
  deadline; removing the `unlock` inside `only_readers` (its `File` is dropped on the
  next line, so only a descriptor duplicated in that instant could tell, which no
  deterministic test builds); opening a read lease read-write (no test uses a
  read-only lease).

## Limits, honestly

* **What a refused exclusive lock means.** `active` is proven to be an exclusive holder
  (the shared try was refused too), not proven to be a window: a claim in another
  process and a removal look the same, which is why the text says so.
* **Reader storms.** A claim waits for readers, so listings that follow one another
  without a gap can keep it waiting until the bound (`busy`). Listings are human-scale
  (start-up, after a Delete, a command); the test that races an owner starts its readers
  after the claim for that reason. A reader that hangs while holding the lock holds off
  claims and removals of that record for as long as it hangs; the bound turns that into
  an answer.
* **Cooperative locking only.** The lock is advisory and per record; a process that
  ignores it can still write the folder, as in §30M. A filesystem without locks refuses
  (`unlockable`) as before. Network filesystems' lock semantics are theirs.
* **Windows.** The locks are `LockFileEx`, shared and exclusive, mandatory on the
  locked range. The Windows legs are compiled and run by CI only; this slice has no
  Windows or Linux run. The Windows-only branches it adds are the `cfg(not(unix))`
  arm of the lock test and a metadata-only look at the lease in a test helper (a locked
  file cannot be read there).
* **Power loss** and the rest of §30M's limits are unchanged and not claimed.
* The window does not change what a person sees except in words and in time: a Recover
  that meets a reader says *Recovering…* a little longer, can be cancelled, and says
  *still being read by another FerriteCAD process; try again* if the reader stays.

## Real window scenario (macOS, for the reviewer who runs it)

Not run by the author: no viewer, browser, `osascript` or CUA was started. The window
changes only in words and in time, and only when a reader holds the copy, so the
scenario is the §30M recipe
([document-crash-recovery-verification.md](document-crash-recovery-verification.md),
"Real window recipe") with **one reader process** inserted between its steps 4 and 6.
Use the existing generator and controlled crash (it writes only `plate.fcad`,
`occupied.fcad`, `inputs/`, `facts.json` and the empty `recovery/`; nothing positive),
one freshly staged bundle, **one viewer at a time under
`tools/watch-viewer-memory.py --limit-mib 1536`**, which owns it and writes its PID to
`<log>.pid`; private roots only:

```sh
ROOT=/private/tmp/ferrite-30s-window-review
export FERRITECAD_RECOVERY_DIR="$ROOT/recovery" FERRITECAD_TABS_DIR="$ROOT/tabs"
# watch() and crash() exactly as in the §30M recipe.
reader() {   # a foreign reader: the only record's lease, shared, until $ROOT/release
  python3 - "$ROOT" <<'PY' &
import fcntl, glob, os, sys, time
root = sys.argv[1]
leases = glob.glob(os.path.join(root, "recovery", "r-*", "lease"))
assert len(leases) == 1, leases
fd = os.open(leases[0], os.O_RDONLY)
fcntl.flock(fd, fcntl.LOCK_SH)
print("HELD", flush=True)
end = time.time() + 120          # bounded: it never outlives the review
while time.time() < end and not os.path.exists(os.path.join(root, "release")):
    time.sleep(0.2)
PY
  echo $! > "$ROOT/reader.pid"
}
```

1. §30M steps 1–4: `watch a plate.fcad`, Apply height 21.5, wait for
   `Recovery copy written <time> UTC.`, export `accepted.stl`/`accepted.fbx`, `crash a`.
2. `reader` (prints `HELD`), then `watch b`. **Expect:** the start-up list still offers
   `plate.fcad — copy written <the same time> UTC`. (On the base a listing and a
   claim exclude each other; here a reader does not hide the copy.)
3. **Recover**, and wait six seconds without touching anything. **Expect:**
   *Recovering…* the whole time; then *Could not recover the copy; it is still listed and
   nothing else changed: … is still being read by another FerriteCAD process; try
   again …*; no new tab; the copy still listed.
4. **Recover**, then at once **Cancel operation**. **Expect:** *Recovery cancelled; the
   copy is kept.*, the button usable again within a moment (not after five seconds),
   no new tab, the copy still listed.
5. **Recover**, and within four seconds `touch "$ROOT/release"`. **Expect:** after a
   moment `*plate.fcad (recovered) — FerriteCAD`, 21.5 high, the copy gone from the list.
6. §30M steps 7–9 (exports `recovered.stl`/`recovered.fbx`, Save to `occupied.fcad`
   refused, Save to `recovered.fcad`, Quit). Check that only your own PIDs are gone
   (`watch-*.pid`, `reader.pid`); after Quit do not call `getApp`/`getAX`: it can
   relaunch the viewer outside the watchdog.
7. Follow §30M steps 10–13 with the current tab behavior: create/crash Untitled,
   launch again, create a second Untitled, then Recover opens the orphan in a separate
   tab (there is no longer a Discard-before-Recover question). Save the recovered tab
   as `empty-recovered.fcad`, Quit, and Discard only the other test Untitled. Then run
   the existing comparator, whose result and negative controls check the saved result:

```sh
python3 tools/document-crash-recovery-gui.py --compare "$ROOT"
```

A watchdog exit status for the two deliberately killed viewers is not success; the
`watch-*.jsonl` logs carry the peaks. A headless widget test is not a window.

## Execution record (local, macOS arm64, rustc 1.96.0)

Primary checkout only; the foreign worktree `a200` was not touched. Sequential builds,
`CARGO_BUILD_JOBS=2`, the two existing targets (`/private/tmp/ferrite-25j-stub-target`,
`/private/tmp/ferrite-24b-native-target`), pinned OCCT and PlaneGCS not rebuilt
(`vendor/install/lib`, `vendor/planegcs` checked present), no third target.
`FCAD_ALLOW_LOADER_FAILURE_PROBES` unset; no viewer, browser, `osascript` or CUA.
Memory stayed at "normal" (free 48–61 %), swap unchanged (1018.56 MiB used of 2048
throughout), disk 140 GiB free. The stub target grew from 5.7 GiB to 6.8 GiB (one
`RUSTFLAGS=-D warnings` mutation build, two refused cross-target attempts). **Stub**
means the true stub: the bridge's configure failed (`CMAKE_DISABLE_FIND_PACKAGE_OpenCASCADE`,
absent OCCT directory), printed on every build, so Homebrew's OCCT was not linked.
The base's tree was clean; the branch is `recovery-inspection-contention`.

* **Base.** `origin/main` = `main` = `513d9758bbcc8b70b5244f99e4e86bef0ebd9c41`, tree
  `5aecbe626eb310b4fb7858f4ac779ce1c44c652e`, after a fresh fetch; PR #98 merged at
  `2026-10-09T20:55:01Z`; its tree equals the reviewed PR head `2d944ac5` (7/7) and
  the code/workflow head `0a51886a` ran 15/15. **Post-merge CI of the merge SHA was
  not finished when this slice started and is not counted as passed:** at the last read,
  `CI` 37990151624 had `lint`, `sbom`, `notices` and `test` on all three OSes `success`
  and `supply-chain` `failure` — the log shows `Build EmbarkStudios/cargo-deny-action`
  refused by Docker Hub (`429 Too Many Requests … unauthenticated pull rate limit`), an
  infrastructure failure, not code, on a tree whose PR run passed; `rust notices`,
  `product sbom`, `rust sbom` and `planegcs pin` `success`; `combined runtime layout`
  37990151658 still `in_progress` (linux `success`, macOS and Windows pending).
* **Reproduction on the base:** above (three failures, all on an executed assertion).
* **Stub, kernel-free, whole suites:** `ferritecad-jobs` — lib 67 (12 recovery, 1 child
  entry ignored), `--test recovery` 17 (+1 ignored child), checkpoints 5, save 21,
  session 12, unnamed 4: all ok. `ferritecad-app` — 587 + `solver_info` 3 (2 ignored): ok.
  `ferritecad-cli` — 348 tests in 31 binaries: ok (6 in `recovery`). `ferritecad-ui`
  109: ok.
* **Packed argv, executed** (the added blocks cut out of `ci.yml` and
  `runtime-layout.yml` by text and run as `bash -eo pipefail`, `RUNNER_OS=macOS`, from an
  ignored directory; the native one sourced in the same zsh as `env.sh`, because bash
  drops `DYLD_*`): jobs-lib gates exit 0 with eight exact `test … ok` lines (the
  Unix descriptor gate included); CLI loop exit 0 with six; the app marker gate exit 0
  with `FCAD_30S_RECOVER_BESIDE_READER_EXECUTED`; the runtime-layout block
  (`cargo test --release`, Open CASCADE, no solver) exit 0 with the marker, no `skipped:`.
* **Native, debug, OCCT + PlaneGCS, scoped to recovery** (not the STEP/FBX corpus):
  17 passed, 1 ignored child entry — the §30M crash matrix and comparator self-check
  (`native_a_killed_window_recovers_…`, `native_recovery_window_scenario_…`), §30Q's
  Recover beside forms (stub, mixed, native, and the refusal test), the new two-tab gate.
  `native_compare_real_recovery_gui_artifacts_…` and
  `native_compare_real_open_new_recover_gui_artifacts_…` are no-ops without
  `FCAD_30M_GUI_DIR` / `FCAD_30Q_GUI_DIR` and are **not** counted as passes.
* **Static:** `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets
  --all-features -- -D warnings` (stub target); `actionlint` (with `shellcheck`) on both
  workflows; `tools/check-licence-headers.sh` (470 files, all MIT);
  `tools/check-export-boundary.sh`; `tools/check-step-corpus.sh`; `git diff --check`.
  `shellcheck` on the cut blocks: clean except SC2043 on the one-entry app loop, an
  artifact of cutting the real many-entry loop down.
* **Windows and Linux: not run.** A cross `cargo check` is not possible here (blake3's
  assembler needs `ml64.exe`, SQLite's `cl.exe`/`x86_64-linux-gnu-gcc`): N/A, not a pass.
  The slice adds no `cfg(windows)` code; its only platform arms are a `cfg(unix)` /
  `cfg(not(unix))` pair in one test and the Unix-only exact gate guarded by
  `RUNNER_OS != Windows`. CI's three legs are the first compile and run there.
* **Cost of the bound:** a listing of the sample plate copy (SQLite open + BLAKE3) takes
  milliseconds; six rounds of eight real `list-recovery` processes around an extraction
  finished in 0.4 s. The 5 s bound is generous by three orders of magnitude, and is
  only paid when a reader is genuinely stuck.

### Found and fixed during the slice

* The first racing test started its readers before the claim and could have starved it
  (three listings in a tight loop leave the lock free only in the gaps): it now starts
  them after the adoption, and the starvation is recorded as a limit above rather than
  hidden by a longer bound.
* A test helper read the lease file's bytes while a claim held it, which a Windows
  exclusive lock forbids: it compares the lease by length and modification time.
* `clippy::panic` in the new test code: the test module carries the repository's usual
  `allow(..., reason = "a gate that cannot fail is not a gate")`.

## Heads and CI

Starting HEAD `513d9758bbcc8b70b5244f99e4e86bef0ebd9c41`; the branch is
`recovery-inspection-contention`; **nothing is staged or committed**, so the final
HEAD is the starting one. The diff is the working tree. **No remote CI has run on it.**
It is not claimed closed: §30, Milestone 5C, the product and the earlier OOM
investigation remain open, and no next slice has started.


## Independent review

The shared-lock probe originally reduced every failure to `active`. A refused lock
(`WouldBlock`) proves contention; an I/O error or an unsupported lock does not. Review
keeps those outcomes separate and also propagates an error releasing the probe. The
existing exact lock gate now feeds the OS-result boundary with I/O, unsupported,
would-block and unlock failures; it does not claim a real damaged filesystem was used.
The regression failed before the change. After correcting the assertion to inspect the
nested I/O cause (the outer Display only prints context), all 12 recovery library gates,
six real CLI recovery processes and the two-tab cancellation owner gate passed in the
true stub build. The ignored library entry is the child-process test harness.

Review logs: `/private/tmp/ferrite-30s-review/`. Native OCCT+PlaneGCS recovery
integration: 17 passed (one child-process harness ignored); the new two-tab owner
scenario passed with its execution marker. Workspace clippy all-targets/all-features,
fmt, actionlint and diff whitespace passed. An initial jobs command omitted
`--all-features` while requiring PlaneGCS and was rejected by build.rs; it was corrected,
not counted as a test failure or a pass.

### macOS window evidence

A fresh release CLI/viewer was staged with the existing pinned native libraries and
verified by the bundled `--solver-info`. Private roots and outputs:
`/private/tmp/ferrite-30s-window-review/`; one viewer at a time under the unchanged
1536 MiB watchdog. No positive output was generated outside the window.

* Applied height 21.5 without Save, observed the recovery publication, exported
  `accepted.stl`/`accepted.fbx`, then killed only watchdog-owned PID 50382.
* A separate Python process held the record's shared lease. The new window still
  offered it. Recover showed `Recovering…`, then the bounded "still being read"
  refusal; no tab appeared and the copy stayed. Another Recover followed immediately
  by Cancel returned "Recovery cancelled; the copy is kept" within the 1.57 s tool
  interaction, before the five-second bound.
* The first manual release was too late for that particular five-second attempt and
  correctly produced another busy refusal. A subsequent bounded-reader attempt
  recovered successfully. The window result proves recovery after release; exact
  release-during-wait timing is established by the deterministic owner/CLI gates,
  not claimed from automation scheduling.
* Exported the recovered model, tried Save to the occupied test file (the app refused
  even after the OS Replace confirmation), then saved `recovered.fcad` and Quit.
  Accepted/recovered STL and FBX are byte-identical. Both actual FBX files passed
  pinned ufbx 0.23.0: six checks each, no failures.
* Created and crashed Untitled; the last window created another Untitled, recovered
  the orphan into its own tab, saved `empty-recovered.fcad`, then Quit/Discard applied
  only to the other test Untitled. The recovery folder ended empty.

Watchdog logs `watch-a`, `watch-b2`, `watch-c2`, `watch-d2`: peak footprints
201.329, 203.439, 189.251, 194.845 MiB. A/C ended by the deliberate SIGKILL; B/D exited
0. Pressure stayed normal while each viewer ran; sampled swap stayed 1471283200 bytes.
Three preflights (`watch-b`, `watch-c`, `watch-d`) refused at system pressure 2 before
launching a viewer, then were retried after pressure returned to 1. No watchdog limit
was weakened. No dead app was queried with CUA after Quit/crash. This is not evidence
that the historical OOM cause has been fixed.

The existing real-window comparator passed on these outputs:
`FCAD_30M_GUI_COMPARE_OK negative_controls=7 all_SQL_cells=true`, one executed test,
no skips. It checked every SQL cell with its existing narrow allowlist, UUIDs, source
and occupied-file preservation, both exports against the actual CLI, the recovered
empty model, and the empty recovery directory.

### CI revisions

Reviewed production code/workflow: `c990770c36e462a04c810ae3f64dcf0a53d8e782`,
[PR #99](https://github.com/gesriot/ferrite-cad/pull/99). The ordinary
[CI run](https://github.com/gesriot/ferrite-cad/actions/runs/37995709932) completed
7/7 jobs successfully, including the added recovery gates on Linux, macOS and Windows.
The [PlaneGCS run](https://github.com/gesriot/ferrite-cad/actions/runs/37995706295)
completed all four jobs successfully. The
[combined native runtime run](https://github.com/gesriot/ferrite-cad/actions/runs/37995706316)
is the required native/mixed evidence for that same code head and must finish before
merge. Final results are attached to the PR checks; these documentation edits change
no code or workflow. Local audit downloads each OS job log and requires the exact new
gate lines and `FCAD_30S_RECOVER_BESIDE_READER_EXECUTED`, rather than counting an
unexecuted test as passed.
