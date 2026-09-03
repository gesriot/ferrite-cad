#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Applies the §22B-1c `export-fbx` mutations. Every edit is restored
# byte-for-byte; compile failures, zero-test invocations and gates that skipped
# themselves are refused rather than credited as mutation kills.
#
# What is under test is the shipped command: that it exists and goes to the FBX
# route rather than the STL one beside it, that it is handed the hierarchical
# `ExportScene` and not a flattened picture, that it reads the document once and
# calls the writer once, that nothing is written to the destination and nothing
# is published until the writer has finished, that `--force` is the only thing
# that replaces and that it always does, that the document can never be its own
# output, that no scratch file survives an early or a late failure, that a
# partial export is a published file with an exit code of its own rather than a
# success or a refusal, and that the report beside it keeps every omission with
# its source-qualified identity, its persisted finding, its typed refusal and
# every affected placement.
#
# Since §22B-1d the work itself lives in `ferritecad-jobs` and this command is
# a thin adapter over it, so the mutations that are about the work are applied
# there. What is being gated is unchanged: the same properties, at the place
# they are now decided, and the command's own bytes, text and exit codes.
#
# Open CASCADE is needed: five of the gates run the real command against a real
# document, and a gate that skipped itself measured nothing. The complex
# assembly gate takes about two minutes at baseline.
#
# Run from the repository root:
#   tools/mutate-export-fbx.sh

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
route="$root/crates/ferritecad-cli/src/export_fbx.rs"
main="$root/crates/ferritecad-cli/src/main.rs"
job="$root/crates/ferritecad-jobs/src/fbx.rs"
publish="$root/crates/ferritecad-jobs/src/publish.rs"
builder="$root/crates/ferritecad-scene/src/export.rs"
prepare="$root/crates/ferritecad-scene/src/prepare.rs"
writer="$root/crates/ferritecad-export/src/fbx/mod.rs"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-export-fbx-mutations.XXXXXX")"
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
    # count. It still has to compile: a command that does not build is not a
    # command that was caught.
    if ! cargo build -p ferritecad-cli --bin ferritecad >"$log" 2>&1; then
      return 20
    fi
    if ! cargo build -p ferritecad-jobs >>"$log" 2>&1; then
      return 20
    fi
    if tools/check-export-boundary.sh >>"$log" 2>&1; then
      return 0
    fi
    return 10
  fi

  case "$gate" in
    unit)
      cargo test -p ferritecad-cli --bin ferritecad "$test_name" -- --nocapture >"$log" 2>&1
      ;;
    jobs)
      cargo test -p ferritecad-jobs --lib "$test_name" -- --nocapture >"$log" 2>&1
      ;;
    cli)
      cargo test -p ferritecad-cli --test export_fbx "$test_name" -- --nocapture >"$log" 2>&1
      ;;
    complex)
      cargo test -p ferritecad-cli --test export_fbx_complex "$test_name" -- --nocapture \
        >"$log" 2>&1
      ;;
    scene)
      cargo test -p ferritecad-scene --test export_scene "$test_name" -- --nocapture >"$log" 2>&1
      ;;
    fbx)
      cargo test -p ferritecad-export --test fbx_ascii "$test_name" -- --nocapture >"$log" 2>&1
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
  baseline_gate unit a_complete_export_is_a_plain_success
  baseline_gate unit a_partial_export_is_not_a_success_and_is_not_a_failure
  baseline_gate jobs a_writer_that_fails_after_it_has_started_publishes_nothing
  baseline_gate jobs a_colour_the_format_cannot_record_is_refused_before_anything_is_published
  baseline_gate jobs a_destination_that_appears_while_the_writer_works_is_not_overwritten
  baseline_gate jobs replacing_a_destination_leaves_none_of_the_old_file_behind
  baseline_gate jobs a_partial_export_is_published_and_says_it_is_not_the_whole_document
  baseline_gate unit every_omission_is_reported_in_a_stable_order
  baseline_gate unit two_sources_with_one_local_key_are_two_entries_in_the_report
  baseline_gate unit the_report_carries_the_typed_refusal_and_not_a_rendering_of_it
  baseline_gate cli the_command_exists_and_writes_an_fbx_the_format_recognises
  baseline_gate cli the_same_document_exports_to_the_same_bytes_and_the_same_report
  baseline_gate cli a_nested_assembly_keeps_its_hierarchy_and_shares_one_geometry
  baseline_gate cli the_document_cannot_be_its_own_output
  baseline_gate scene a_structural_definition_is_not_reported_as_an_omission
  baseline_gate scene one_native_body_is_one_definition_one_node_and_one_mesh
  baseline_gate scene cancelling_produces_no_partial_scene_and_leaks_no_shapes
  baseline_gate fbx an_omitted_definition_is_a_node_with_no_geometry_and_says_why
  baseline_gate boundary -
  baseline_gate complex \
    the_complex_assembly_becomes_one_fbx_that_keeps_every_definition_and_says_what_is_missing
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
cargo_gate unit __ferritecad_zero_test_control__
zero_result=$?
set -e
if [ "$zero_result" -ne 30 ]; then
  echo "zero-test control was not refused, result $zero_result" >&2
  exit 1
fi
echo "harness control: an actual zero-test run was refused"

begin_mutation "$route"
replace_once "$route" \
  'fn exit_code(report: &FbxWriteReport) -> u8 {' \
  $'fn exit_code(report: &FbxWriteReport) -> u8 {\nthis is not Rust,'
set +e
cargo_gate unit a_complete_export_is_a_plain_success
compile_result=$?
set -e
if [ "$compile_result" -ne 20 ]; then
  echo "non-compiling control was not classified as compile refusal" >&2
  exit 1
fi
echo "harness control: non-compiling mutant refused before runtime"
restore_mutation

# ------------------------------------------------------------------- the route

# 1. There is no route at all: the subcommand parses and does nothing.
begin_mutation "$main"
replace_once "$main" \
  '        Command::ExportFbx(args) => export_fbx::export_fbx(args),' \
  '        Command::ExportFbx(_) => Ok(ExitCode::SUCCESS),'
expect_kill route_absent cli the_command_exists_and_writes_an_fbx_the_format_recognises
restore_mutation

# 2. The route goes to the STL export beside it, which also writes a file to
#    the path the user asked for.
begin_mutation "$main"
replace_once "$main" \
  '        Command::ExportFbx(args) => export_fbx::export_fbx(args),' \
  $'        Command::ExportFbx(args) => export::export_stl(ExportStlArgs {\n            path: args.path,\n            output: args.output,\n            solid: None,\n            linear_deflection: TessellationParams::DEFAULT_LINEAR,\n            angular_deflection: TessellationParams::DEFAULT_ANGULAR,\n            force: args.force,\n        }),'
expect_kill route_goes_to_export_stl cli the_command_exists_and_writes_an_fbx_the_format_recognises
restore_mutation

# 3. The scene is flattened before it is written: every placement becomes a
#    root, which is exactly what a picture would have handed the writer.
begin_mutation "$job"
replace_once "$job" \
  '    let report = write_and_publish(&request, &scene, context.cancel())?;' \
  $'    let scene = {\n        let mut builder = ferritecad_export::ExportSceneBuilder::new();\n        let mut ids = Vec::new();\n        for definition in scene.definitions() {\n            ids.push(builder.definition(\n                definition.source.clone(),\n                definition.display_name.clone(),\n                definition.provenance.clone(),\n                definition.geometry.clone(),\n            )?);\n        }\n        for node in scene.nodes() {\n            builder.node(\n                None,\n                ids[node.definition.index()],\n                node.local_transform,\n                node.display_name.clone(),\n                node.colour_override,\n            )?;\n        }\n        builder.finish()?\n    };\n    let report = write_and_publish(&request, &scene, context.cancel())?;'
expect_kill hierarchy_flattened_before_writing cli \
  a_nested_assembly_keeps_its_hierarchy_and_shares_one_geometry
restore_mutation

# 4. The route reaches for a picture. Nothing here uses it yet, which is the
#    point: the mechanical gate must catch the dependency before the flattening
#    that follows it is written.
begin_mutation "$route"
replace_once "$route" \
  '    let mut kernel = OcctKernel::new()?;' \
  $'    let picture: Option<&ferritecad_scene::LoadedScene> = None;\n    let _snapshot = picture.map(|loaded| &loaded.snapshot);\n    let mut kernel = OcctKernel::new()?;'
expect_kill route_names_a_picture boundary -
restore_mutation

# 5. A second read of the document the export has already read once.
begin_mutation "$route"
replace_once "$route" \
  '    let mut kernel = OcctKernel::new()?;' \
  $'    let _again = ferritecad_document::Document::open_read_only(&args.path)?;\n    let mut kernel = OcctKernel::new()?;'
expect_kill second_document_read boundary -
restore_mutation

# 6. A second rebuild of what the export already rebuilt.
begin_mutation "$route"
replace_once "$route" \
  '    let mut kernel = OcctKernel::new()?;' \
  $'    let mut spare = OcctKernel::new()?;\n    let _again = ferritecad_eval::rebuild_cold(\n        &ferritecad_document::Document::open_read_only(&args.path)?,\n        &mut spare,\n        &OperationContext::default(),\n    )?;\n    let mut kernel = OcctKernel::new()?;'
expect_kill second_rebuild boundary -
restore_mutation

# 7. The external STEP file is read again, so the export stops being a function
#    of the document alone.
begin_mutation "$route"
replace_once "$route" \
  '    let mut kernel = OcctKernel::new()?;' \
  $'    let _source = std::fs::read(args.path.with_extension("stp"))\n        .map_err(|e| CadError::io("reading the source STEP again", e))?;\n    let mut kernel = OcctKernel::new()?;'
expect_kill external_step_read_again complex \
  the_complex_assembly_becomes_one_fbx_that_keeps_every_definition_and_says_what_is_missing
restore_mutation

# 8. The writer is called twice into the same sink.
begin_mutation "$job"
replace_once "$job" \
  '    let report = write_scene(scene, &mut sink, cancel)?;' \
  $'    let _first = write_scene(scene, &mut sink, cancel)?;\n    let report = write_scene(scene, &mut sink, cancel)?;'
expect_kill writer_called_twice cli a_nested_assembly_keeps_its_hierarchy_and_shares_one_geometry
restore_mutation

# ---------------------------------------------------------------- publication

# 9. The destination itself is opened and written into.
begin_mutation "$job"
replace_once "$job" '        .open(temporary.path())' '        .open(request.destination)'
expect_kill destination_written_directly jobs \
  a_writer_that_fails_after_it_has_started_publishes_nothing
restore_mutation

# 10. The file appears at the destination before the writer has finished with
#     it, so a reader can see a prefix of an export.
begin_mutation "$job"
replace_once "$job" \
  '    let mut sink = std::io::BufWriter::with_capacity(1 << 20, file);' \
  $'    std::fs::hard_link(temporary.path(), request.destination).map_err(|e| {\n        CadError::io(format!("publishing {}", request.destination.display()), e)\n    })?;\n    let mut sink = std::io::BufWriter::with_capacity(1 << 20, file);'
replace_once "$job" \
  '    publish_if_still_wanted(temporary, request.destination, request.existing, cancel)?;' \
  $'    drop(temporary);'
expect_kill published_before_the_writer_finished jobs \
  a_writer_that_fails_after_it_has_started_publishes_nothing
restore_mutation

# 11. Publication replaces whatever is there, whether or not it was allowed to.
#
#     Both halves, because only both together are the defect a user would see:
#     the early check is a courtesy — the survivor at the end of this file says
#     so — and on its own it would hide this behind a refusal that happens to
#     come first. What is being gated is the publication.
begin_mutation "$main" "$route"
replace_once "$main" \
  $'    if force {\n        Existing::Replace\n    } else {\n        Existing::Keep {\n            advice: REPLACE_ADVICE,\n        }\n    }' \
  $'    let _ = force;\n    Existing::Replace'
replace_once "$route" '    if !args.force && path_entry_exists(&args.output)? {' \
  '    if false && !args.force && path_entry_exists(&args.output)? {'
expect_kill overwrite_without_force cli \
  an_existing_file_is_not_replaced_without_being_asked
restore_mutation

# 12. `--force` stops replacing anything.
begin_mutation "$main"
replace_once "$main" \
  $'    if force {\n        Existing::Replace\n    } else {\n        Existing::Keep {\n            advice: REPLACE_ADVICE,\n        }\n    }' \
  $'    let _ = force;\n    Existing::Keep {\n        advice: REPLACE_ADVICE,\n    }'
expect_kill force_does_not_replace cli force_replaces_the_destination_completely
restore_mutation

# 13. The document is allowed to be its own output.
begin_mutation "$route" "$job"
replace_once "$route" \
  '    refuse_source_as_destination(&args.path, &args.output, SOURCE_IS_DESTINATION)?;' \
  '    let _ = SOURCE_IS_DESTINATION;'
replace_once "$job" \
  '    refuse_source_as_destination(request.document, request.destination, SOURCE_IS_DESTINATION)?;' \
  '    let _ = SOURCE_IS_DESTINATION;'
expect_kill source_allowed_as_destination cli the_document_cannot_be_its_own_output
restore_mutation

# 14. Scratch space survives a failure that happened before any byte was
#     written.
begin_mutation "$publish"
replace_once "$publish" \
  $'    fn drop(&mut self) {\n        // A failure here is not worth reporting over whatever error is already\n        // on its way out, and there is nothing useful to do about it.\n        self.clean();\n    }' \
  $'    fn drop(&mut self) {\n        let _ = &self.directory;\n    }'
expect_kill scratch_survives_an_early_failure jobs \
  a_colour_the_format_cannot_record_is_refused_before_anything_is_published
restore_mutation

# 15. And a failure that happened with half a file already on the disk.
begin_mutation "$publish"
replace_once "$publish" \
  $'    fn drop(&mut self) {\n        // A failure here is not worth reporting over whatever error is already\n        // on its way out, and there is nothing useful to do about it.\n        self.clean();\n    }' \
  $'    fn drop(&mut self) {\n        let _ = &self.directory;\n    }'
expect_kill scratch_survives_a_late_failure jobs \
  a_writer_that_fails_after_it_has_started_publishes_nothing
restore_mutation

# ---------------------------------------------------- what a partial export is

# 16. A partial export becomes a plain success.
begin_mutation "$route"
replace_once "$route" \
  $'    if report.is_complete() {\n        0\n    } else {\n        EXIT_PARTIAL\n    }' \
  $'    let _ = report;\n    0'
expect_kill partial_export_returns_zero unit \
  a_partial_export_is_not_a_success_and_is_not_a_failure
restore_mutation

# 17. A partial export becomes a refusal with no file at all, which throws away
#     every definition that was fine.
begin_mutation "$job"
replace_once "$job" \
  '    publish_if_still_wanted(temporary, request.destination, request.existing, cancel)?;' \
  $'    if !report.is_complete() {\n        return Err(CadError::input("this document cannot be exported completely"));\n    }\n    publish_if_still_wanted(temporary, request.destination, request.existing, cancel)?;'
expect_kill partial_export_refuses_entirely jobs \
  a_partial_export_is_published_and_says_it_is_not_the_whole_document
restore_mutation

# ------------------------------------------------------------- what it reports

# 18. Only the first omission is reported.
begin_mutation "$route"
replace_once "$route" '    for (index, report) in omissions.iter().enumerate() {' \
  '    for (index, report) in omissions.iter().take(1).enumerate() {'
expect_kill report_stops_at_the_first_omission unit every_omission_is_reported_in_a_stable_order
restore_mutation

# 19. An imported key travels without the identity of the file it belongs to,
#     so two definitions from two sources read as one.
begin_mutation "$route"
replace_once "$route" \
  $'        } => format!("imported source {source}  key {definition_key}"),' \
  $'        } => {\n            let _ = source;\n            format!("key {definition_key}")\n        }'
expect_kill source_identity_dropped unit \
  two_sources_with_one_local_key_are_two_entries_in_the_report
restore_mutation

# 20. The typed refusal is replaced by the words shown to a person.
begin_mutation "$route"
replace_once "$route" '            report.omission.refusal.stable_name()' \
  '            report.omission.refusal'
expect_kill refusal_rendered_for_a_person unit \
  the_report_carries_the_typed_refusal_and_not_a_rendering_of_it
restore_mutation

# 21. The persisted finding is replaced by a Debug rendering of it.
begin_mutation "$route"
replace_once "$route" \
  '        writeln!(out, "    finding     {}", report.omission.finding).expect("cannot fail");' \
  $'        writeln!(out, "    finding     {:?}", report.omission.finding).expect("cannot fail");'
expect_kill finding_rendered_with_debug unit \
  the_report_carries_the_typed_refusal_and_not_a_rendering_of_it
restore_mutation

# 22. Only the first placement of an omitted definition is named.
begin_mutation "$route"
replace_once "$route" \
  $'    nodes\n        .iter()\n        .map(|node| format!("node/{}", node.index()))' \
  $'    nodes\n        .iter()\n        .take(1)\n        .map(|node| format!("node/{}", node.index()))'
expect_kill affected_placements_lost unit every_omission_is_reported_in_a_stable_order
restore_mutation

# 23. The report stops being a function of the export.
begin_mutation "$route"
replace_once "$route" \
  '    writeln!(out, "  FBX 7.4.0 ASCII, {} byte(s)", report.bytes()).expect("cannot fail");' \
  $'    writeln!(\n        out,\n        "  FBX 7.4.0 ASCII, {} byte(s) at {:?}",\n        report.bytes(),\n        std::time::SystemTime::now()\n    )\n    .expect("cannot fail");'
expect_kill report_is_not_deterministic cli \
  the_same_document_exports_to_the_same_bytes_and_the_same_report
restore_mutation

# ------------------------------------- the two halves this route is handed

# 24. An assembly frame with no geometry of its own is declared a missing part.
begin_mutation "$builder"
replace_once "$builder" '            return Ok(ExportGeometry::Structural);' \
  $'            return Ok(ExportGeometry::Omitted(ExportOmission::new(\n                ferritecad_exchange::Diagnostic {\n                    stage: ferritecad_exchange::Stage::Validation,\n                    severity: ferritecad_exchange::Severity::Warning,\n                    entity: String::new(),\n                    message: "an assembly frame".to_owned(),\n                },\n                TessellationRefusal::IncompleteFace,\n            )));'
expect_kill structural_declared_an_omission scene a_structural_definition_is_not_reported_as_an_omission
restore_mutation

# 25. The node of a definition with no triangles is dropped from the file, so
#     the part silently stops existing rather than being visibly missing.
begin_mutation "$writer"
replace_once "$writer" \
  $'        for node in self.scene.nodes() {\n            self.model(ascii, node)?;\n        }' \
  $'        for node in self.scene.nodes() {\n            let definition = self.scene.definition(node.definition);\n            if definition.is_some_and(|d| d.geometry.omission().is_some()) {\n                continue;\n            }\n            self.model(ascii, node)?;\n        }'
expect_kill omitted_node_removed_from_the_file fbx \
  an_omitted_definition_is_a_node_with_no_geometry_and_says_why
restore_mutation

# 26. The shapes a successful export built are never given back.
begin_mutation "$prepare"
replace_once "$prepare" $'    built.release_all(kernel);\n    output' \
  $'    let _ = &built;\n    output'
expect_kill success_leaks_kernel_shapes scene \
  one_native_body_is_one_definition_one_node_and_one_mesh
restore_mutation

# 27. And the shapes a cancelled one built are not given back either.
begin_mutation "$prepare"
replace_once "$prepare" \
  $'    for shape in imported.into_iter().rev() {\n        kernel.release(shape);\n    }' \
  $'    for shape in imported.into_iter().rev() {\n        let _ = shape;\n    }'
expect_kill cancellation_leaks_kernel_shapes scene \
  cancelling_produces_no_partial_scene_and_leaks_no_shapes
restore_mutation

# ---------------------------------------------------- what must not be a kill

# The early no-clobber check is a courtesy, not the decision. Removing it must
# change nothing a user can see: publication is where a destination is refused,
# and it refuses the same thing with the same message. A gate this killed would
# be a gate that had mistaken the courtesy for the contract.
begin_mutation "$route"
replace_once "$route" '    if !args.force && path_entry_exists(&args.output)? {' \
  '    if false && !args.force && path_entry_exists(&args.output)? {'
expect_survivor early_no_clobber_check_removed cli \
  an_existing_file_is_not_replaced_without_being_asked
restore_mutation

# `TessellationRefusal` has exactly one variant today, and its `Debug` spelling
# happens to be the same string as its stable name. So swapping one for the
# other is invisible now and would stop being invisible the moment a variant
# gained a field. Recorded as a survivor rather than dressed up as a kill.
begin_mutation "$route"
replace_once "$route" '            report.omission.refusal.stable_name()' \
  $'            format!("{:?}", report.omission.refusal)'
expect_survivor refusal_rendered_with_debug unit \
  the_report_carries_the_typed_refusal_and_not_a_rendering_of_it
restore_mutation

baseline
no_stale_backups "$root"

echo "mutation campaign: $killed mutants killed"
echo "mutation campaign: $survived unexpected survivors"
echo "mutation campaign: compile refusal, zero-test and skipped-gate controls were not credited"
