#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Applies the §22A-1c typed-omission and UI mutations. Every edit is restored
# byte-for-byte; compile failures and zero-test invocations are refused rather
# than credited as mutation kills.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
kernel="$root/crates/ferritecad-kernel/src/refusal.rs"
scene="$root/crates/ferritecad-scene/src/lib.rs"
ffi="$root/crates/ferritecad-occt/src/ffi.rs"
bridge="$root/crates/ferritecad-occt-bridge/src/bridge.cpp"
ui="$root/crates/ferritecad-ui/src/panels.rs"
app="$root/crates/ferritecad-app/src/main.rs"
snapshot="$root/crates/ferritecad-viewport/src/snapshot.rs"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-step-omission-mutations.XXXXXX")"
occt_dir="${OpenCASCADE_DIR:-/opt/homebrew/Cellar/opencascade/7.9.3/lib/cmake/opencascade}"
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

cargo_gate() {
  local gate="$1"
  local test_name="$2"
  local log="$temporary/${gate}.log"
  local status
  case "$gate" in
    kernel)
      cargo test -p ferritecad-kernel --lib "$test_name" -- --nocapture >"$log" 2>&1
      ;;
    scene)
      FERRITECAD_REQUIRE_OCCT=1 OpenCASCADE_DIR="$occt_dir" \
        cargo test -p ferritecad-scene --lib "$test_name" -- --nocapture >"$log" 2>&1
      ;;
    ffi)
      FERRITECAD_REQUIRE_OCCT=1 OpenCASCADE_DIR="$occt_dir" \
        cargo test -p ferritecad-occt --lib "$test_name" -- --nocapture >"$log" 2>&1
      ;;
    ui)
      cargo test -p ferritecad-ui --lib "$test_name" -- --nocapture >"$log" 2>&1
      ;;
    app)
      FERRITECAD_REQUIRE_OCCT=1 OpenCASCADE_DIR="$occt_dir" \
        cargo test -p ferritecad-app --bin ferritecad-viewer "$test_name" \
        -- --nocapture >"$log" 2>&1
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
    if rg -q 'could not compile|failed to run custom build command|error: building' "$log"; then
      return 20
    fi
    return 30
  fi
  if [ "$runs" -ne 1 ]; then
    return 30
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

expect_kill() {
  local name="$1"
  local gate="$2"
  local test_name="$3"
  set +e
  cargo_gate "$gate" "$test_name"
  local result=$?
  set -e
  case "$result" in
    10) echo "killed at runtime $gate gate: $name"; killed=$((killed + 1)) ;;
    20) echo "compile refusal (not a runtime kill): $name" >&2; exit 1 ;;
    30) echo "zero-test or malformed run refused: $name" >&2; exit 1 ;;
    0) echo "survived unexpectedly: $name" >&2; survived=$((survived + 1)); exit 1 ;;
    *) echo "harness refusal $result: $name" >&2; exit 1 ;;
  esac
}

baseline() {
  baseline_gate kernel only_a_typed_direct_kernel_source_is_a_tessellation_refusal
  baseline_gate scene the_old_phrase_in_an_untyped_kernel_error_does_not_permit_omission
  baseline_gate scene a_triangle_free_mesh_is_not_itself_evidence_of_an_omission
  baseline_gate scene omission_requires_the_same_persisted_and_fresh_validation_failure
  baseline_gate ffi incomplete_face_status_is_typed_and_an_ordinary_kernel_status_is_not
  baseline_gate ffi both_incomplete_face_bridge_branches_use_the_dedicated_status
  baseline_gate ui an_omitted_import_is_visibly_marked_in_the_definitions_list_only
  baseline_gate ui an_omitted_import_inspector_explains_both_observations_in_portable_terms
  baseline_gate app describe_carries_an_imported_geometry_omission_into_the_ui_model
  baseline_gate app a_chosen_definition_that_draws_nothing_cannot_offer_hide
}

no_stale_backups "$root"

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
cargo_gate kernel __ferritecad_zero_test_control__
zero_result=$?
set -e
if [ "$zero_result" -ne 30 ]; then
  echo "zero-test control was not refused, result $zero_result" >&2
  exit 1
fi
echo "harness control: an actual zero-test run was refused"

begin_mutation "$kernel"
replace_once "$kernel" \
  'pub enum TessellationRefusal {' \
  $'pub enum TessellationRefusal {\n    this is not Rust,'
set +e
cargo_gate kernel only_a_typed_direct_kernel_source_is_a_tessellation_refusal
compile_result=$?
set -e
if [ "$compile_result" -ne 20 ]; then
  echo "non-compiling control was not classified as compile refusal" >&2
  exit 1
fi
echo "harness control: non-compiling mutant refused before runtime"
restore_mutation

classification='    TessellationRefusal::of(reason) == Some(&TessellationRefusal::IncompleteFace)'

begin_mutation "$scene"
replace_once "$scene" "$classification" \
  $'    TessellationRefusal::of(reason) == Some(&TessellationRefusal::IncompleteFace)\n        || reason.to_string().contains("Open CASCADE could not tessellate every face")'
expect_kill old_phrase_accepted_as_untyped_kernel scene \
  the_old_phrase_in_an_untyped_kernel_error_does_not_permit_omission
restore_mutation

begin_mutation "$scene"
replace_once "$scene" "$classification" \
  '    reason.to_string().contains("Open CASCADE could not tessellate every face")'
expect_kill typed_source_ignored_and_display_parsed scene \
  human_wording_does_not_change_a_typed_partial_import
restore_mutation

begin_mutation "$scene"
replace_once "$scene" "$classification" \
  '    reason.kind() == ferritecad_types::ErrorKind::Kernel'
expect_kill every_kernel_failure_accepted scene \
  the_old_phrase_in_an_untyped_kernel_error_does_not_permit_omission
restore_mutation

begin_mutation "$scene"
replace_once "$scene" \
  '                Err(reason) if is_face_tessellation_refusal(&reason) => {' \
  '                Err(reason) if from.omittable.contains_key(&definition.key) => {'
expect_kill persisted_diagnostic_accepted_without_current_typed_refusal scene \
  the_old_phrase_in_an_untyped_kernel_error_does_not_permit_omission
restore_mutation

begin_mutation "$scene"
replace_once "$scene" \
  $'                        .filter(|diagnostic| {\n                            is_topology_failure(diagnostic)\n                                && reopened.diagnostics_now.iter().any(|current| {\n                                    is_topology_failure(current)\n                                        && current.entity == diagnostic.entity\n                                })\n                        })' \
  $'                        .filter(|diagnostic| is_topology_failure(diagnostic))'
expect_kill persisted_diagnostic_accepted_without_fresh_validation scene \
  omission_requires_the_same_persisted_and_fresh_validation_failure
restore_mutation

begin_mutation "$scene"
replace_once "$scene" \
  $'                    let Some(diagnostic) = from.omittable.get(&definition.key) else {\n                        return Err(reason);\n                    };' \
  $'                    let diagnostic = from.omittable.get(&definition.key).cloned().unwrap_or(\n                        Diagnostic {\n                            stage: Stage::Validation,\n                            severity: Severity::Fail,\n                            entity: definition.key.clone(),\n                            message: "inferred from tessellation".to_owned(),\n                        },\n                    );'
replace_once "$scene" \
  '                        diagnostic: diagnostic.clone(),' \
  '                        diagnostic,'
expect_kill typed_refusal_accepted_without_persisted_or_fresh_validation scene \
  a_typed_refusal_without_matching_validation_finding_refuses_the_whole_load
restore_mutation

begin_mutation "$bridge"
replace_once "$bridge" \
  $'      write_error(out_error, "Open CASCADE could not tessellate every face; status " +\n                                 std::to_string(mesh_status));\n      return FC_OCCT_INCOMPLETE_FACE_TESSELLATION;' \
  $'      write_error(out_error, "Open CASCADE could not tessellate every face; status " +\n                                 std::to_string(mesh_status));\n      return FC_OCCT_KERNEL;'
expect_kill mesher_status_branch_returns_generic_kernel ffi \
  both_incomplete_face_bridge_branches_use_the_dedicated_status
restore_mutation

begin_mutation "$bridge"
replace_once "$bridge" \
  $'        write_error(out_error,\n                    "Open CASCADE produced no triangles for one of the shape\x27s faces");\n        return FC_OCCT_INCOMPLETE_FACE_TESSELLATION;' \
  $'        write_error(out_error,\n                    "Open CASCADE produced no triangles for one of the shape\x27s faces");\n        return FC_OCCT_KERNEL;'
expect_kill missing_triangulation_branch_returns_generic_kernel ffi \
  both_incomplete_face_bridge_branches_use_the_dedicated_status
restore_mutation

begin_mutation "$ffi"
replace_once "$ffi" \
  $'        STATUS_INCOMPLETE_FACE_TESSELLATION => Err(CadError::kernel_because(\n            format!("{what}: {}", error.text()),\n            TessellationRefusal::IncompleteFace,\n        )),' \
  $'        STATUS_INCOMPLETE_FACE_TESSELLATION => {\n            Err(CadError::kernel(format!("{what}: {}", error.text())))\n        }'
expect_kill dedicated_abi_status_mapped_to_plain_kernel ffi \
  incomplete_face_status_is_typed_and_an_ordinary_kernel_status_is_not
restore_mutation

begin_mutation "$scene"
replace_once "$scene" \
  '                Ok(mesh) => mesh,' \
  $'                Ok(mesh) if mesh.indices.is_empty() => {\n                    omission = Some(GeometryOmission {\n                        diagnostic: from.omittable.get(&definition.key).cloned().unwrap_or(\n                            Diagnostic {\n                                stage: Stage::Validation,\n                                severity: Severity::Fail,\n                                entity: definition.key.clone(),\n                                message: "inferred from an empty mesh".to_owned(),\n                            },\n                        ),\n                        reason: "inferred from an empty mesh".to_owned(),\n                    });\n                    mesh\n                }\n                Ok(mesh) => mesh,'
expect_kill every_triangle_free_definition_inferred_as_omitted scene \
  a_triangle_free_mesh_is_not_itself_evidence_of_an_omission
restore_mutation

begin_mutation "$ui"
replace_once "$ui" \
  '                if geometry_unavailable.is_some() {' \
  '                if geometry_unavailable.is_none() {'
expect_kill omission_hidden_from_definitions_list ui \
  an_omitted_import_is_visibly_marked_in_the_definitions_list_only
restore_mutation

inspector_block=$'                if let Some(unavailable) = geometry_unavailable {\n                    rows.push(("Geometry", "Imported geometry unavailable".to_owned()));\n                    rows.push(("Finding entity", unavailable.finding_entity.to_owned()));\n                    rows.push(("Validation", unavailable.validation.to_owned()));\n                    rows.push(("Tessellation", unavailable.tessellation.to_owned()));\n                }'

begin_mutation "$ui"
replace_once "$ui" "$inspector_block" \
  '                let _ = geometry_unavailable;'
expect_kill omission_hidden_from_selection_inspector ui \
  an_omitted_import_inspector_explains_both_observations_in_portable_terms
restore_mutation

begin_mutation "$ui"
replace_once "$ui" \
  '                if geometry_unavailable.is_some() {' \
  '                if geometry_unavailable.is_none() || geometry_unavailable.is_some() {'
expect_kill every_definition_without_typed_omission_marked_unavailable ui \
  an_omitted_import_is_visibly_marked_in_the_definitions_list_only
restore_mutation

begin_mutation "$ui"
replace_once "$ui" "$inspector_block" \
  $'                if let Some(unavailable) = geometry_unavailable {\n                    rows.push(("Geometry", "Imported geometry unavailable".to_owned()));\n                    rows.push(("Finding entity", unavailable.finding_entity.to_owned()));\n                    rows.push(("Validation", unavailable.validation.to_owned()));\n                    rows.push(("Tessellation", unavailable.tessellation.to_owned()));\n                    rows.push(("Debug", format!("{geometry_unavailable:?}")));\n                }'
expect_kill debug_or_transient_representation_leaked ui \
  an_omitted_import_inspector_explains_both_observations_in_portable_terms
restore_mutation

begin_mutation "$snapshot"
replace_once "$snapshot" \
  $'        if mesh.indices.is_empty() {\n            return;\n        }' \
  ''
expect_kill omission_creates_bounds_or_visibility app \
  a_chosen_definition_that_draws_nothing_cannot_offer_hide
restore_mutation

baseline
no_stale_backups "$root"

echo "mutation campaign: $killed runtime mutants killed"
echo "mutation campaign: $survived unexpected survivors"
echo "mutation campaign: compile refusal and zero-test controls were not credited"
