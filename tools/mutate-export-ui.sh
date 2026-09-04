#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Applies the §22B-1d window-export mutations. Every edit is restored
# byte-for-byte; compile failures, zero-test invocations and gates that skipped
# themselves are refused rather than credited as mutation kills.
#
# What is under test is the window's export: that it is offered only for a
# document that was accepted, that it writes out the document on screen rather
# than the last one somebody asked for, that a closed dialog does exactly
# nothing, that a file already at the destination is replaced only after this
# application has asked and only the file it asked about, that the work happens
# on a worker rather than in the event handler, that a withdrawn request
# publishes nothing and a late answer changes nothing, that closing the window
# stops and waits for every worker, and that what is shown afterwards is the
# writer's own record — every omission, with its source-qualified identity, its
# persisted finding, its typed refusal and every placement — rather than a
# second opinion, a debugging aid or a warning about a file that is whole.
#
# It also gates what the window must never become: a second exporter, a second
# publication, a reader of the picture, or a program that starts the command
# line.
#
# No Open CASCADE is needed for the window's own gates: they drive the real
# route — document, scene, writer, publication — with a kernel that needs none.
# The command-line gates alongside them do need one, and a gate that skipped
# itself measured nothing.
#
# Run from the repository root:
#   tools/mutate-export-ui.sh

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
window="$root/crates/ferritecad-app/src/exports.rs"
viewer="$root/crates/ferritecad-app/src/main.rs"
panels="$root/crates/ferritecad-ui/src/panels.rs"
job="$root/crates/ferritecad-jobs/src/fbx.rs"
publish="$root/crates/ferritecad-jobs/src/publish.rs"
main="$root/crates/ferritecad-cli/src/main.rs"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-export-ui-mutations.XXXXXX")"
mutation_files=()
mutation_hashes=()
mutation_mtimes=()
killed=0
survived=0

digest() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

mtime() {
  if stat -f %m "$1" >/dev/null 2>&1; then
    stat -f %m "$1"
  else
    stat -c %Y "$1"
  fi
}

no_stale_backups() {
  local directory="$1"
  local stale
  if [ "$directory" = "$root" ]; then
    stale="$({
      find "$root" -maxdepth 1 \( -name '*.bak' -o -name '*.mutbak' \) -print
      find "$root/crates" "$root/tools" "$root/docs" "$root/fixtures" \
        "$root/.github" \( -name '*.bak' -o -name '*.mutbak' \) -print
    } | head -1)"
  else
    stale="$(find "$directory" \( -name '*.bak' -o -name '*.mutbak' \) -print -quit)"
  fi
  if [ -n "$stale" ]; then
    echo "stale mutation backup: $stale" >&2
    return 1
  fi
}

restore_mutation() {
  local count="${#mutation_files[@]}"
  if [ "$count" -eq 0 ]; then
    return 0
  fi

  local index
  for ((index = 0; index < count; ++index)); do
    cp "${mutation_files[index]}.mutbak" "${mutation_files[index]}"
    rm "${mutation_files[index]}.mutbak"
  done
  sleep 1
  touch "${mutation_files[@]}"

  for ((index = 0; index < count; ++index)); do
    if [ "$(digest "${mutation_files[index]}")" != "${mutation_hashes[index]}" ]; then
      echo "restoration changed ${mutation_files[index]}" >&2
      return 1
    fi
    if [ "$(mtime "${mutation_files[index]}")" -le "${mutation_mtimes[index]}" ]; then
      echo "restoration did not advance mtime for ${mutation_files[index]}" >&2
      return 1
    fi
  done
  mutation_files=()
  mutation_hashes=()
  mutation_mtimes=()
}

cleanup() {
  local status=$?
  set +e
  restore_mutation
  rm -rf "$temporary"
  exit "$status"
}
trap cleanup EXIT INT TERM

begin_mutation() {
  if [ "${#mutation_files[@]}" -ne 0 ]; then
    echo "a mutation backup is already active" >&2
    exit 1
  fi
  local file
  for file in "$@"; do
    if [ -e "$file.mutbak" ]; then
      echo "stale mutation backup: $file.mutbak" >&2
      exit 1
    fi
    mutation_files+=("$file")
    mutation_hashes+=("$(digest "$file")")
    mutation_mtimes+=("$(mtime "$file")")
    cp -p "$file" "$file.mutbak"
  done
}

replace_once() {
  local file="$1"
  local old="$2"
  local new="$3"
  local count
  count="$(FCAD_OLD="$old" perl -0ne '
    $old = $ENV{FCAD_OLD}; $at = 0; $count = 0;
    while (($found = index($_, $old, $at)) >= 0) {
      ++$count; $at = $found + length($old);
    }
    print $count;
  ' "$file")"
  if [ "$count" -ne 1 ]; then
    echo "anchor in $file matched $count times, expected once" >&2
    return 1
  fi
  FCAD_OLD="$old" FCAD_NEW="$new" perl -0pi -e '
    $old = $ENV{FCAD_OLD}; $new = $ENV{FCAD_NEW};
    $at = index($_, $old); substr($_, $at, length($old), $new);
  ' "$file"
}

# 0 survived, 10 killed, 20 compile refusal, 30 zero-test or malformed run,
# 31 the gate skipped itself and measured nothing.
cargo_gate() {
  local gate="$1"
  local test_name="$2"
  local log="$temporary/${gate}.log"
  local status

  if [ "$gate" = "boundary" ]; then
    # A mechanical check over the sources, so a mutation it catches has no test
    # count. It still has to compile: a window that does not build is not a
    # window that was caught.
    if ! cargo build -p ferritecad-app --bin ferritecad-viewer >"$log" 2>&1; then
      return 20
    fi
    if ! cargo build -p ferritecad-cli --bin ferritecad >>"$log" 2>&1; then
      return 20
    fi
    if tools/check-export-boundary.sh >>"$log" 2>&1; then
      return 0
    fi
    return 10
  fi

  case "$gate" in
    app)
      cargo test -p ferritecad-app --bin ferritecad-viewer "$test_name" -- --nocapture \
        >"$log" 2>&1
      ;;
    ui)
      cargo test -p ferritecad-ui --lib "$test_name" -- --nocapture >"$log" 2>&1
      ;;
    jobs)
      cargo test -p ferritecad-jobs --lib "$test_name" -- --nocapture >"$log" 2>&1
      ;;
    cli)
      cargo test -p ferritecad-cli --test export_fbx "$test_name" -- --nocapture >"$log" 2>&1
      ;;
    *)
      echo "unknown mutation gate: $gate" >&2
      return 40
      ;;
  esac
  status=$?

  local runs
  runs="$(sed -n 's/^running \([0-9][0-9]*\) tests*$/\1/p' "$log" | tail -1)"
  if [ "${runs:-0}" -eq 0 ]; then
    if grep -qE 'could not compile|failed to run custom build command|error: building' "$log"; then
      return 20
    fi
    return 30
  fi
  if [ "$runs" -ne 1 ]; then
    return 30
  fi
  # A gate that returned early because this build has no kernel proved nothing
  # about the mutation, and must never be counted either way.
  if grep -q 'skipped: this build has no Open CASCADE' "$log"; then
    return 31
  fi
  if [ "$status" -eq 0 ]; then
    return 0
  fi
  return 10
}

baseline_gate() {
  local gate="$1"
  local test_name="$2"
  set +e
  cargo_gate "$gate" "$test_name"
  local result=$?
  set -e
  if [ "$result" -ne 0 ]; then
    echo "baseline $gate gate failed with result $result: $test_name" >&2
    cat "$temporary/${gate}.log" >&2
    exit 1
  fi
  echo "baseline $gate gate passed: $test_name"
}

expect_survivor() {
  local name="$1"
  local gate="$2"
  local test_name="$3"
  set +e
  cargo_gate "$gate" "$test_name"
  local result=$?
  set -e
  case "$result" in
    0) echo "survived as required (observationally equivalent): $name" ;;
    10) echo "an equivalent change was killed, so a gate depends on it: $name" >&2; exit 1 ;;
    20) echo "compile refusal (not a runtime result): $name" >&2; exit 1 ;;
    30) echo "zero-test or malformed run refused: $name" >&2; exit 1 ;;
    31) echo "the gate skipped itself; Open CASCADE is required: $name" >&2; exit 1 ;;
    *) echo "harness refusal $result: $name" >&2; exit 1 ;;
  esac
}

expect_kill() {
  local name="$1"
  local gate="$2"
  local test_name="$3"
  set +e
  cargo_gate "$gate" "$test_name"
  local result=$?
  set -e
  case "$result" in
    10) echo "killed at $gate gate: $name"; killed=$((killed + 1)) ;;
    20) echo "compile refusal (not a runtime kill): $name" >&2; exit 1 ;;
    30) echo "zero-test or malformed run refused: $name" >&2; exit 1 ;;
    31) echo "the gate skipped itself; Open CASCADE is required: $name" >&2; exit 1 ;;
    0) echo "survived unexpectedly: $name" >&2; survived=$((survived + 1)); exit 1 ;;
    *) echo "harness refusal $result: $name" >&2; exit 1 ;;
  esac
}

baseline() {
  baseline_gate app nothing_can_be_exported_until_a_document_is_on_screen
  baseline_gate app a_failed_open_does_not_change_what_an_export_would_read
  baseline_gate app an_abandoned_open_does_not_change_what_an_export_would_read
  baseline_gate app a_stale_answer_does_not_change_what_an_export_would_read
  baseline_gate app asking_for_an_export_changes_nothing_about_the_picture
  baseline_gate app an_export_with_no_document_on_screen_starts_nothing
  baseline_gate app a_closed_dialog_is_an_exact_no_op
  baseline_gate app the_document_itself_is_refused_as_its_own_output
  baseline_gate app confirming_does_not_make_the_document_its_own_output
  baseline_gate app an_existing_destination_is_asked_about_before_anything_is_written
  baseline_gate app cancelling_the_confirmation_keeps_the_destination
  baseline_gate app a_question_nobody_answers_writes_nothing
  baseline_gate app confirming_replaces_the_whole_file
  baseline_gate app a_confirmation_applies_to_the_file_it_named
  baseline_gate app a_question_about_the_last_document_does_not_survive_a_new_open
  baseline_gate app the_work_happens_on_the_worker_and_not_in_the_call
  baseline_gate app the_export_does_not_happen_in_the_event_loop
  baseline_gate app an_export_writes_the_file_the_user_chose
  baseline_gate app the_window_and_the_shared_job_write_the_same_bytes
  baseline_gate app a_new_export_cancels_the_old_one_whose_answer_then_counts_for_nothing
  baseline_gate app an_export_given_up_on_publishes_nothing_and_is_not_a_failure
  baseline_gate app shutting_down_cancels_and_joins_every_export
  baseline_gate app a_failed_export_is_reported_as_one
  baseline_gate app a_partial_export_says_so_and_reports_every_omission
  baseline_gate app two_sources_with_one_local_key_stay_two_omissions
  baseline_gate app a_complete_export_carries_no_omissions_and_no_warning_words
  baseline_gate ui the_way_to_export_is_offered_only_with_a_document_on_screen
  baseline_gate ui an_export_can_be_given_up_on_only_while_one_is_running
  baseline_gate jobs a_cancellation_that_arrives_before_publication_publishes_nothing
  baseline_gate jobs a_write_given_up_on_part_way_through_stops_where_it_was
  baseline_gate jobs a_withdrawn_request_is_refused_with_a_kind_nothing_retries
  baseline_gate jobs a_request_still_wanted_publishes
  baseline_gate cli an_existing_file_is_not_replaced_without_being_asked
  baseline_gate boundary -
}

cd "$root"
no_stale_backups "$root"

# ------------------------------------------------------------ harness controls

printf 'one\n' >"$temporary/one.txt"
if replace_once "$temporary/one.txt" missing x >/dev/null 2>&1; then
  echo "anchor-miss control was accepted" >&2
  exit 1
fi
printf 'twice twice\n' >"$temporary/twice.txt"
if replace_once "$temporary/twice.txt" twice x >/dev/null 2>&1; then
  echo "multiple-anchor control was accepted" >&2
  exit 1
fi
mkdir "$temporary/stale"
printf 'stale\n' >"$temporary/stale/control.mutbak"
if no_stale_backups "$temporary/stale" >/dev/null 2>&1; then
  echo "stale-backup control was accepted" >&2
  exit 1
fi
echo "harness controls: anchor miss, multiple matches and stale backup refused"

baseline

set +e
cargo_gate app __ferritecad_zero_test_control__
zero_result=$?
set -e
if [ "$zero_result" -ne 30 ]; then
  echo "zero-test control was not refused, result $zero_result" >&2
  exit 1
fi
echo "harness control: an actual zero-test run was refused"

begin_mutation "$window"
replace_once "$window" \
  'fn display(path: &Path) -> String {' \
  $'fn display(path: &Path) -> String {\nthis is not Rust,'
set +e
cargo_gate app a_closed_dialog_is_an_exact_no_op
compile_result=$?
set -e
if [ "$compile_result" -ne 20 ]; then
  echo "non-compiling control was not classified as compile refusal" >&2
  exit 1
fi
echo "harness control: non-compiling mutant refused before runtime"
restore_mutation

# ------------------------------------------------------- what is offered

# 1. The action is offered whether or not a document was ever accepted, so it
#    can be pressed while the window is still empty or still reading.
begin_mutation "$viewer"
replace_once "$viewer" \
  $'fn can_export<P>(scene: &LiveScene<P>) -> bool {\n    scene.document.is_some()\n}' \
  $'fn can_export<P>(scene: &LiveScene<P>) -> bool {\n    let _ = scene;\n    true\n}'
expect_kill export_offered_without_a_document app \
  nothing_can_be_exported_until_a_document_is_on_screen
restore_mutation

# 2. And the button ignores the answer, which is the same defect one layer up.
begin_mutation "$panels"
replace_once "$panels" \
  '            .add_enabled(activity.can_export, egui::Button::new(EXPORT_FBX))' \
  '            .add_enabled(true, egui::Button::new(EXPORT_FBX))'
expect_kill export_button_always_enabled ui \
  the_way_to_export_is_offered_only_with_a_document_on_screen
restore_mutation

# --------------------------------------------- which document is written out

# 3. The export reads the path the file dialog remembers, which is already the
#    document being opened rather than the one on screen.
begin_mutation "$viewer"
replace_once "$viewer" \
  $'        let document = self\n            .live\n            .as_ref()\n            .and_then(|live| live.scene.document.clone());\n        let proxy = self.proxy.clone();\n        exports::begin_export(\n            &mut self.exports,' \
  $'        let document = Some(self.document.clone());\n        let proxy = self.proxy.clone();\n        exports::begin_export(\n            &mut self.exports,'
expect_kill exports_the_path_the_dialog_remembers boundary -
restore_mutation

# 4. A reading that failed renames the document an export would read.
begin_mutation "$viewer"
replace_once "$viewer" \
  $'    let next = next?;\n    *scene = LiveScene::new(' \
  $'    let next = match next {\n        Ok(next) => next,\n        Err(error) => {\n            scene.document = Some(std::path::PathBuf::from("b.fcad"));\n            return Err(error);\n        }\n    };\n    *scene = LiveScene::new('
expect_kill a_failed_open_renames_the_export_source app \
  a_failed_open_does_not_change_what_an_export_would_read
restore_mutation

# 5. An answer nobody is waiting for still names the document on screen. The
#    request that was given up on is the case: it is no longer current, and
#    the newest path recorded is still its own.
begin_mutation "$viewer"
replace_once "$viewer" \
  $'        if !self.accepts(generation) {\n            return None;\n        }\n        self.requested' \
  $'        self.requested'
expect_kill a_reading_given_up_on_still_arrives app \
  an_abandoned_open_does_not_change_what_an_export_would_read
restore_mutation

# 6. Beginning an Open no longer stops the export of the document being left
#    behind, so an answer about the old one can arrive describing the new one.
begin_mutation "$viewer"
replace_once "$viewer" \
  '        exports::cancel_export(&mut self.exports, &mut self.input);
        self.document = path;' \
  '        self.document = path;'
expect_kill an_open_leaves_the_old_export_running boundary -
restore_mutation

# ------------------------------------------------------------- the dialog

# 7. A dialog the user closed exports anyway, to a name nobody chose.
begin_mutation "$window"
replace_once "$window" \
  $'    let (Some(source), Some(destination)) = (source, chosen) else {\n        return Ok(ExportRequest::Nothing);\n    };' \
  $'    let Some(source) = source else {\n        return Ok(ExportRequest::Nothing);\n    };\n    let destination = chosen.unwrap_or_else(|| source.with_extension("fbx"));'
expect_kill a_closed_dialog_starts_an_export app a_closed_dialog_is_an_exact_no_op
restore_mutation

# 8. The document may be its own output after all.
begin_mutation "$window"
replace_once "$window" \
  $'    if is_same_entry(source, &destination)? {\n        return Ok(ExportRequest::RefusedSource);\n    }' \
  '    let _ = is_same_entry(source, &destination)?;'
expect_kill source_allowed_as_destination app \
  the_document_itself_is_refused_as_its_own_output
restore_mutation

# ------------------------------------------------------- the confirmation

# 9. A file already at the destination is replaced without anybody being asked.
begin_mutation "$window"
replace_once "$window" \
  $'        Ok(ExportRequest::Confirm(destination)) => {\n            exports.ask(destination);\n            input.request_redraw();\n            None\n        }' \
  $'        Ok(ExportRequest::Confirm(destination)) => {\n            let generation = exports.start(&destination, true, spawn);\n            input.request_redraw();\n            Some(generation)\n        }'
expect_kill overwrite_before_confirmation app \
  an_existing_destination_is_asked_about_before_anything_is_written
restore_mutation

# 10. The question is about the first file chosen rather than the last, so
#     pressing Replace replaces something the user was not asked about.
begin_mutation "$window"
replace_once "$window" \
  $'    fn ask(&mut self, destination: PathBuf) {\n        self.pending = Some(destination);\n    }' \
  $'    fn ask(&mut self, destination: PathBuf) {\n        self.pending.get_or_insert(destination);\n    }'
expect_kill confirmation_applies_to_another_path app \
  a_confirmation_applies_to_the_file_it_named
restore_mutation

# 11. A question left over from the document that has been replaced survives,
#     so Replace exports the new document over the old document's destination.
begin_mutation "$window"
replace_once "$window" \
  '        let asked = self.dismiss();' \
  $'        let asked = false;'
expect_kill confirmation_applies_to_a_stale_path app \
  a_question_about_the_last_document_does_not_survive_a_new_open
restore_mutation

# 12. Saying no exports anyway.
begin_mutation "$window"
replace_once "$window" \
  $'        ReplaceChoice::Cancel => {\n            if exports.dismiss() {\n                input.request_redraw();\n            }\n            None\n        }' \
  $'        ReplaceChoice::Cancel => {\n            confirm_export(exports, input, source, ReplaceChoice::Replace, spawn)\n        }'
expect_kill saying_no_still_exports app cancelling_the_confirmation_keeps_the_destination
restore_mutation

# 13. A frame in which nothing was pressed is taken for a yes.
begin_mutation "$window"
replace_once "$window" \
  '        ReplaceChoice::Waiting => None,' \
  $'        ReplaceChoice::Waiting => {\n            confirm_export(exports, input, source, ReplaceChoice::Replace, spawn)\n        }'
expect_kill an_unanswered_question_is_taken_for_a_yes app \
  a_question_nobody_answers_writes_nothing
restore_mutation

# 14. A confirmed replacement is published no-clobber, so the export the user
#     authorised fails instead of replacing the file.
begin_mutation "$window"
replace_once "$window" \
  '                    let generation = exports.start(&destination, true, spawn);' \
  '                    let generation = exports.start(&destination, false, spawn);'
expect_kill confirmation_does_not_authorise_a_replacement app confirming_replaces_the_whole_file
restore_mutation

# 15. And a confirmation makes the document an acceptable output for itself.
begin_mutation "$window"
replace_once "$window" \
  '            match is_same_entry(source, &destination) {
                Ok(false) => {' \
  '            match is_same_entry(source, &destination) {
                Ok(_) => {'
expect_kill confirmation_allows_the_document_as_its_own_output app \
  confirming_does_not_make_the_document_its_own_output
restore_mutation

# ------------------------------------------------------ worker and lifecycle

# 16. The export happens in the call that starts it, which is the event
#     handler: the window freezes for exactly as long as the export takes.
begin_mutation "$window"
replace_once "$window" \
  '    std::thread::spawn(move || deliver(export()))' \
  $'    deliver(export());\n    std::thread::spawn(|| {})'
expect_kill export_runs_in_the_event_handler app \
  the_work_happens_on_the_worker_and_not_in_the_call
restore_mutation

# 17. A new export leaves the one before it running.
begin_mutation "$window"
replace_once "$window" \
  $'        for exporting in &self.running {\n            exporting.cancel.cancel();\n        }\n        // A question about some other file is over' \
  $'        // A question about some other file is over'
expect_kill a_new_export_leaves_the_old_one_running app \
  a_new_export_cancels_the_old_one_whose_answer_then_counts_for_nothing
restore_mutation

# 18. An answer to a request that has been replaced overwrites what the window
#     is saying about the one that replaced it.
begin_mutation "$window"
replace_once "$window" \
  '            } if *waiting == generation => {' \
  '            } if *waiting == generation || true => {'
expect_kill stale_generation_overwrites_the_status app \
  a_new_export_cancels_the_old_one_whose_answer_then_counts_for_nothing
restore_mutation

# 19. Giving up is reported as a failure, which complains about something the
#     user asked for.
begin_mutation "$window"
replace_once "$window" \
  '                    Err(error) if error.kind() == ErrorKind::Cancellation => {' \
  '                    Err(error) if error.kind() == ErrorKind::Cancellation && false => {'
expect_kill cancellation_reported_as_a_failure app \
  an_export_given_up_on_publishes_nothing_and_is_not_a_failure
restore_mutation

# 20. Closing the window does not stop the exports.
begin_mutation "$window"
replace_once "$window" \
  $'        for exporting in &self.running {\n            exporting.cancel.cancel();\n        }\n        // Cancelled first, all of them, and only then waited for.' \
  $'        // Cancelled first, all of them, and only then waited for.'
expect_kill shutdown_does_not_cancel app shutting_down_cancels_and_joins_every_export
restore_mutation

# 21. And it detaches them instead of waiting: the process ends with kernel
#     sessions still open and nobody to end them.
begin_mutation "$window"
replace_once "$window" \
  $'        for exporting in self.running.drain(..) {\n            let _ = exporting.worker.join();\n        }' \
  $'        for exporting in self.running.drain(..) {\n            drop(exporting.worker);\n        }'
expect_kill export_worker_detached_at_shutdown app \
  shutting_down_cancels_and_joins_every_export
restore_mutation

# ------------------------------------------------ cancellation and publication

# 22. The last check before publication is gone, so an export that was given up
#     on publishes its file anyway.
begin_mutation "$job"
replace_once "$job" \
  $'    cancel.check()?;\n    temporary.publish(destination, existing)' \
  $'    let _ = cancel;\n    temporary.publish(destination, existing)'
expect_kill no_cancellation_check_before_publication jobs \
  a_cancellation_that_arrives_before_publication_publishes_nothing
restore_mutation

# 23. Serialisation cannot be given up on: the sink accepts everything, so a
#     cancelled export writes the whole file before anybody notices.
begin_mutation "$job"
replace_once "$job" \
  $'        if self.cancel.is_cancelled() {\n            // Deliberately not `ErrorKind::Interrupted`' \
  $'        if false && self.cancel.is_cancelled() {\n            // Deliberately not `ErrorKind::Interrupted`'
expect_kill serialisation_ignores_cancellation jobs \
  a_write_given_up_on_part_way_through_stops_where_it_was
restore_mutation

# 23b. And it refuses with the one kind that means "try again", so a cancelled
#      export never stops. This is the defect this slice actually had.
begin_mutation "$job"
replace_once "$job" \
  '            return Err(std::io::Error::other("the export was cancelled"));' \
  $'            return Err(std::io::Error::new(\n                std::io::ErrorKind::Interrupted,\n                "the export was cancelled",\n            ));'
expect_kill cancellation_refused_with_a_retried_kind jobs \
  a_withdrawn_request_is_refused_with_a_kind_nothing_retries
restore_mutation

# 24. Scratch space survives a publication that did not happen.
begin_mutation "$publish"
replace_once "$publish" \
  $'    fn drop(&mut self) {\n        // A failure here is not worth reporting over whatever error is already\n        // on its way out, and there is nothing useful to do about it.\n        self.clean();\n    }' \
  $'    fn drop(&mut self) {\n        let _ = &self.directory;\n    }'
expect_kill scratch_survives_a_publication_failure jobs \
  a_cancellation_that_arrives_before_publication_publishes_nothing
restore_mutation

# ------------------------------------------------------------ what is shown

# 25. A whole export is described as a partial one, so a window says something
#     is missing from a file that has everything in it.
begin_mutation "$window"
replace_once "$window" \
  '            } if omissions.is_empty() => format!("{EXPORTED} {destination}"),' \
  '            } if omissions.is_empty() && false => format!("{EXPORTED} {destination}"),'
expect_kill complete_result_displayed_as_partial app an_export_writes_the_file_the_user_chose
restore_mutation

# 26. And a partial export is described as a whole one.
begin_mutation "$window"
replace_once "$window" \
  '            } if omissions.is_empty() => format!("{EXPORTED} {destination}"),' \
  '            } if omissions.is_empty() || true => format!("{EXPORTED} {destination}"),'
expect_kill partial_result_displayed_as_complete app \
  a_partial_export_says_so_and_reports_every_omission
restore_mutation

# 27. Only the first omission is kept, so a window describes a smaller problem
#     than the one the user has.
begin_mutation "$window"
replace_once "$window" \
  '            omissions: report.omissions().iter().map(OmittedWords::of).collect(),' \
  '            omissions: report.omissions().iter().take(1).map(OmittedWords::of).collect(),'
expect_kill report_stops_at_the_first_omission app \
  a_partial_export_says_so_and_reports_every_omission
restore_mutation

# 28. An imported key travels without the identity of the file it belongs to,
#     and `#2583` names something different in every STEP file there is.
begin_mutation "$window"
replace_once "$window" \
  '        } => format!("imported source {source}  key {definition_key}"),' \
  $'        } => {\n            let _ = source;\n            format!("key {definition_key}")\n        }'
expect_kill source_identity_dropped app two_sources_with_one_local_key_stay_two_omissions
restore_mutation

# 29. The persisted finding is replaced by a debugging aid.
begin_mutation "$window"
replace_once "$window" \
  '            finding: report.omission.finding.to_string(),' \
  '            finding: format!("{:?}", report.omission.finding),'
expect_kill finding_rendered_with_debug app \
  a_partial_export_says_so_and_reports_every_omission
restore_mutation

# 30. The typed refusal is replaced by the sentence written for a person, which
#     is free to be rewritten and is not a fact.
begin_mutation "$window"
replace_once "$window" \
  '            refusal: report.omission.refusal.stable_name().to_owned(),' \
  '            refusal: report.omission.refusal.to_string(),'
expect_kill refusal_rendered_for_a_person app \
  a_partial_export_says_so_and_reports_every_omission
restore_mutation

# 31. Only the first placement of an omitted definition is named, so a person
#     looking for the other five never finds them.
begin_mutation "$window"
replace_once "$window" \
  '            placements: report.nodes.iter().map(node_key).collect(),' \
  '            placements: report.nodes.iter().take(1).map(node_key).collect(),'
expect_kill affected_placements_lost app a_partial_export_says_so_and_reports_every_omission
restore_mutation

# 32. The window works out for itself what is missing, from the scene rather
#     than from the record of the file that was published.
begin_mutation "$window"
replace_once "$window" \
  '    fn of(destination: String, report: &FbxWriteReport) -> Self {' \
  $'    #[allow(dead_code)]\n    fn second_opinion(scene: &ferritecad_export::ExportScene) -> usize {\n        scene.completeness().omissions().len()\n    }\n\n    fn of(destination: String, report: &FbxWriteReport) -> Self {'
expect_kill window_works_out_what_is_missing_itself boundary -
restore_mutation

# ------------------------------------------- what a window must never become

# 33. The window starts the command line instead of doing the work.
begin_mutation "$viewer"
replace_once "$viewer" \
  '    fn export_to(&mut self, chosen: Option<PathBuf>) {' \
  $'    fn export_to(&mut self, chosen: Option<PathBuf>) {\n        let _ = std::process::Command::new("ferritecad");'
expect_kill window_starts_the_command_line boundary -
restore_mutation

# 34. The window exports the picture rather than the stored document.
begin_mutation "$window"
replace_once "$window" \
  '/// A path as a person reads it: whole, because where a file went is the' \
  $'/// What a picture would be handed to an exporter as.\n#[allow(dead_code)]\nfn from_the_picture(snapshot: &ferritecad_viewport::RenderSnapshot) -> usize {\n    snapshot.draws().len()\n}\n\n/// A path as a person reads it: whole, because where a file went is the'
expect_kill window_exports_the_picture boundary -
restore_mutation

# 35. A second writer in the window, which is a second file format that agrees
#     with the first until the day it does not.
begin_mutation "$window"
replace_once "$window" \
  '/// A path as a person reads it: whole, because where a file went is the' \
  $'/// A second way to write an FBX.\n#[allow(dead_code)]\nfn write_it_here(scene: &ferritecad_export::ExportScene) -> Result<()> {\n    let mut sink = Vec::new();\n    ferritecad_export::write_fbx_ascii_7400(scene, &mut sink)?;\n    Ok(())\n}\n\n/// A path as a person reads it: whole, because where a file went is the'
expect_kill second_writer_in_the_window boundary -
restore_mutation

# 36. And a second publication, which is a second set of rules about what
#     replaces what.
begin_mutation "$window"
replace_once "$window" \
  '/// A path as a person reads it: whole, because where a file went is the' \
  $'/// A second way to put a file where the user asked for it.\n#[allow(dead_code)]\nfn publish_it_here(from: &Path, to: &Path) -> std::io::Result<()> {\n    std::fs::rename(from, to)\n}\n\n/// A path as a person reads it: whole, because where a file went is the'
expect_kill second_publication_in_the_window boundary -
restore_mutation

# 37. A second scene built in the window, which is a second export.
begin_mutation "$window"
replace_once "$window" \
  '/// A path as a person reads it: whole, because where a file went is the' \
  $'/// A second way to build the scene an export writes.\n#[allow(dead_code)]\nfn build_it_here<K: GeometryKernel + ?Sized>(\n    kernel: &mut K,\n    document: &Path,\n) -> Result<ferritecad_export::ExportScene> {\n    ferritecad_scene::export_scene(\n        document,\n        kernel,\n        |_, _| Err(ferritecad_types::CadError::unsupported("no imports")),\n        &TessellationParams::default(),\n        &OperationContext::default(),\n    )\n}\n\n/// A path as a person reads it: whole, because where a file went is the'
expect_kill second_export_scene_in_the_window boundary -
restore_mutation

# 38. The export moves the camera, which is a window rearranging itself around
#     a file it is writing.
begin_mutation "$window"
replace_once "$window" \
  '        Ok(ExportRequest::Start(destination)) => {
            let generation = exports.start(&destination, false, spawn);' \
  '        Ok(ExportRequest::Start(destination)) => {
            input.handle(
                ferritecad_ui::ViewportEvent::Look(ferritecad_viewport::StandardView::Top),
                false,
            );
            let generation = exports.start(&destination, false, spawn);'
expect_kill export_moves_the_camera app asking_for_an_export_changes_nothing_about_the_picture
restore_mutation

# 39. And it forgets the click that was already in flight, which is a question
#     about a frame the user asked about being answered against another one.
begin_mutation "$window"
replace_once "$window" \
  '        Ok(ExportRequest::Start(destination)) => {
            let generation = exports.start(&destination, false, spawn);' \
  '        Ok(ExportRequest::Start(destination)) => {
            input.forget_pending();
            let generation = exports.start(&destination, false, spawn);'
expect_kill export_forgets_a_click_in_flight app \
  asking_for_an_export_changes_nothing_about_the_picture
restore_mutation

# ---------------------------------------------- the command line beside it

# 40. The extraction drifts: the command line stops naming the flag that would
#     have let it replace the file, so its message becomes a window's.
begin_mutation "$main"
replace_once "$main" \
  'const REPLACE_ADVICE: &str = "pass --force to replace it";' \
  'const REPLACE_ADVICE: &str = "it was created while the export was being written";'
expect_kill command_line_message_drift cli an_existing_file_is_not_replaced_without_being_asked
restore_mutation

# 41. And the window borrows the command line's vocabulary, telling somebody
#     looking at a window to pass a flag.
begin_mutation "$window"
replace_once "$window" \
  'const APPEARED: &str = "it was created while the export was being written";' \
  'const APPEARED: &str = "pass --force to replace it";'
expect_kill window_prints_a_command_line_flag app the_window_speaks_for_itself
restore_mutation

baseline
no_stale_backups "$root"

echo "mutation campaign: $killed mutants killed"
echo "mutation campaign: $survived unexpected survivors"
echo "mutation campaign: compile refusal, zero-test and skipped-gate controls were not credited"
