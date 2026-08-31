#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Applies the §22A-1b complex partial-import mutations to the real CLI,
# document reopen, scene loader and GPU path. Every edit is restored exactly.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
test_file="$root/crates/ferritecad-cli/tests/complex_step_pixels.rs"
cli_main="$root/crates/ferritecad-cli/src/main.rs"
cli_import="$root/crates/ferritecad-cli/src/import.rs"
document="$root/crates/ferritecad-document/src/document.rs"
scene="$root/crates/ferritecad-scene/src/lib.rs"
snapshot="$root/crates/ferritecad-viewport/src/snapshot.rs"
shader="$root/crates/ferritecad-viewport-gpu/src/shader.wgsl"
renderer="$root/crates/ferritecad-viewport-gpu/src/renderer.rs"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-complex-pixel-mutations.XXXXXX")"
occt_dir="${OpenCASCADE_DIR:-/opt/homebrew/Cellar/opencascade/7.9.3/lib/cmake/opencascade}"
mutation_files=()
mutation_hashes=()
mutation_mtimes=()
killed=0
survived=0

main_gate=the_complex_partial_import_reaches_repeatable_identified_pixels
gpu_guard=the_required_gpu_run_cannot_turn_a_missing_adapter_into_a_green_skip

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
    stale="$(find "$directory" \
      \( -name '*.bak' -o -name '*.mutbak' \) -print -quit)"
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
  # Keep the restored input newer than a mutant artifact so incremental builds
  # cannot accidentally execute the mutant after its source has been restored.
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
  local test_name="$1"
  local log="$temporary/cargo-gate.log"
  local status
  if FERRITECAD_REQUIRE_OCCT=1 FERRITECAD_REQUIRE_GPU=1 \
      OpenCASCADE_DIR="$occt_dir" \
      cargo test -p ferritecad-cli --test complex_step_pixels "$test_name" \
      -- --exact --nocapture >"$log" 2>&1; then
    status=0
  else
    status=$?
  fi

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

expect_kill() {
  local name="$1"
  local test_name="${2:-$main_gate}"
  set +e
  cargo_gate "$test_name"
  local result=$?
  set -e
  case "$result" in
    10) echo "killed at runtime complex pixel gate: $name"; killed=$((killed + 1)) ;;
    20) echo "compile refusal (not a runtime kill): $name" >&2; exit 1 ;;
    30) echo "zero-test or malformed run refused: $name" >&2; exit 1 ;;
    0) echo "survived unexpectedly: $name" >&2; survived=$((survived + 1)); exit 1 ;;
    *) echo "harness refusal $result: $name" >&2; exit 1 ;;
  esac
}

no_stale_backups "$root"

# Harness controls: missing and multiply matched anchors, then a stale backup.
printf 'one\n' > "$temporary/one.txt"
if replace_once "$temporary/one.txt" missing x >/dev/null 2>&1; then
  echo "anchor-miss control was accepted" >&2
  exit 1
fi
printf 'twice twice\n' > "$temporary/twice.txt"
if replace_once "$temporary/twice.txt" twice x >/dev/null 2>&1; then
  echo "multiple-anchor control was accepted" >&2
  exit 1
fi
mkdir "$temporary/stale"
printf 'stale\n' > "$temporary/stale/control.mutbak"
if no_stale_backups "$temporary/stale" >/dev/null 2>&1; then
  echo "stale-backup control was accepted" >&2
  exit 1
fi
echo "harness controls: anchor miss, multiple matches and stale backup refused"

cargo_gate "$main_gate"
cargo_gate "$gpu_guard"

set +e
cargo_gate __ferritecad_zero_test_control__
zero_result=$?
set -e
if [ "$zero_result" -ne 30 ]; then
  echo "zero-test control was not refused, result $zero_result" >&2
  exit 1
fi
echo "harness control: an actual zero-test run was refused"

begin_mutation "$test_file"
replace_once "$test_file" \
  'use std::path::{Path, PathBuf};' \
  $'use std::path::{Path, PathBuf};\nthis is not Rust;'
set +e
cargo_gate "$gpu_guard"
compile_result=$?
set -e
if [ "$compile_result" -ne 20 ]; then
  echo "non-compiling control was not classified as compile refusal" >&2
  exit 1
fi
echo "harness control: non-compiling mutant refused before runtime"
restore_mutation

begin_mutation "$test_file"
replace_once "$test_file" \
  '.join("../../fixtures/step/interoperability/c3d-ap203-complex-assembly.stp")' \
  '.join("../../fixtures/step/canonical/01-single-part.step")'
expect_kill complex_fixture_absent_from_pixel_path
restore_mutation

begin_mutation "$cli_import"
replace_once "$cli_import" \
  '    let stored = match &outcome {' \
  $'    if !outcome.diagnostics().is_empty() {\n        return Ok(ExitCode::from(EXIT_NOTICED));\n    }\n\n    let stored = match &outcome {'
expect_kill exit_4_treated_as_no_document
restore_mutation

begin_mutation "$document"
replace_once "$document" \
  '            diagnostics_at_import: stored.imported.diagnostics_at_import,' \
  '            diagnostics_at_import: Vec::new(),'
expect_kill validation_diagnostics_lost_after_reopen
restore_mutation

definition_anchor=$'        // The file\x27s own name for this definition, kept beside the source it'

begin_mutation "$scene"
replace_once "$scene" "$definition_anchor" \
  $'        if definition.key == "step.product_definition#2428" {\n            continue;\n        }\n\n'$definition_anchor
expect_kill definition_2428_silently_removed
restore_mutation

begin_mutation "$scene"
replace_once "$scene" "$definition_anchor" \
  $'        if definition.key == "step.product_definition#2583" {\n            continue;\n        }\n\n'$definition_anchor
expect_kill definition_2583_silently_removed
restore_mutation

begin_mutation "$scene"
replace_once "$scene" "$definition_anchor" \
  $'        if matches!(definition.key.as_str(),\n            "step.product_definition#2428" | "step.product_definition#2583")\n        {\n            continue;\n        }\n\n'$definition_anchor
expect_kill all_invalid_definitions_silently_removed
restore_mutation

begin_mutation "$cli_main"
replace_once "$cli_main" 'const EXIT_NOTICED: u8 = 4;' 'const EXIT_NOTICED: u8 = 0;'
expect_kill partial_import_declared_clean_exit_0
restore_mutation

begin_mutation "$cli_import"
replace_once "$cli_import" \
  '    let stored = match &outcome {' \
  $'    if !outcome.diagnostics().is_empty() {\n        return Ok(ExitCode::from(EXIT_REJECTED));\n    }\n\n    let stored = match &outcome {'
expect_kill warning_causes_full_refusal
restore_mutation

begin_mutation "$test_file"
replace_once "$test_file" \
  'fn picture(path: &Path) -> LoadedScene {' \
  $'fn picture(path: &Path, external: &Path) -> LoadedScene {\n    std::fs::read(external).expect("the viewer again requires the external STEP");'
replace_once "$test_file" \
  '    let loaded = picture(&output);' \
  '    let loaded = picture(&output, &input);'
expect_kill reopen_requires_external_step
restore_mutation

begin_mutation "$test_file"
replace_once "$test_file" \
  '        .prepare(Arc::clone(&snapshot))' \
  '        .prepare(Arc::new(ferritecad_viewport::SnapshotBuilder::new().build()))'
expect_kill gpu_receives_empty_snapshot
restore_mutation

begin_mutation "$snapshot"
replace_once "$snapshot" \
  $'        for item in &self.items {\n            extent.include(&self.meshes[item.mesh], item);\n        }' \
  $'        for item in self.items.iter().take(1) {\n            extent.include(&self.meshes[item.mesh], item);\n        }'
expect_kill frame_uses_only_first_placement
restore_mutation

begin_mutation "$shader"
replace_once "$shader" '    out.pick = draw.pick;' '    out.pick = 0u;'
expect_kill identity_target_cleared_over_model
restore_mutation

begin_mutation "$renderer"
replace_once "$renderer" \
  $'        self.require_own(prepared)?;\n\n        let snapshot = Arc::clone(&prepared.snapshot);' \
  $'        self.require_own(prepared)?;\n        self.geometry_uploads += prepared.meshes.len() as u64;\n\n        let snapshot = Arc::clone(&prepared.snapshot);'
expect_kill repeated_render_reuploads_geometry
restore_mutation

begin_mutation "$test_file"
replace_once "$test_file" \
  $'fn missing_adapter(required: bool) -> MissingAdapter {\n    if required {\n        MissingAdapter::Fail\n    } else {\n        MissingAdapter::Skip\n    }\n}' \
  $'fn missing_adapter(required: bool) -> MissingAdapter {\n    let _ = required;\n    MissingAdapter::Skip\n}'
expect_kill required_gpu_is_allowed_to_skip "$gpu_guard"
restore_mutation

cargo_gate "$main_gate"
cargo_gate "$gpu_guard"
no_stale_backups "$root"

echo "mutation campaign: $killed runtime mutants killed"
echo "mutation campaign: $survived unexpected survivors"
echo "mutation campaign: compile refusal and zero-test controls were not credited"
