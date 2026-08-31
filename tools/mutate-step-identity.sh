#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Applies the §22A-1a STEP identity mutations to the real shared C++ resolver,
# probe and production bridge. Every source edit is restored byte-for-byte.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
header="$root/crates/ferritecad-occt-bridge/src/step_identity.hpp"
bridge="$root/crates/ferritecad-occt-bridge/src/bridge.cpp"
probe_source="$root/tools/step-key-probe/src/main.cpp"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-step-mutations.XXXXXX")"
probe_build="${FCAD_STEP_MUTATION_BUILD:-/tmp/fcad-step-key-probe-22a}"
probe_binary=""
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
  # Make the restored source newer than both the mutant object and its
  # original timestamp, so an incremental compiler cannot reuse a mutant.
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

probe_gate() {
  local build_log="$temporary/probe-build.log"
  if ! cmake --build "$probe_build" --parallel >"$build_log" 2>&1; then
    cat "$build_log" >&2
    return 20
  fi

  local report="$temporary/probe-report.txt"
  local status
  if "$probe_binary" \
      "$root/fixtures/step/damaged/06-duplicate-product-definition.step" \
      "$root/fixtures/step/interoperability/c3d-ap203-complex-assembly.stp" \
      >"$report" 2>&1; then
    status=0
  else
    status=$?
  fi
  if [ "$status" -ne 0 ]; then
    return 10
  fi
  if [ "$(rg -c 'ferritecad::definition_keys\(' "$bridge")" != 1 ] ||
     [ "$(rg -c 'ferritecad::definition_keys\(' "$probe_source")" != 1 ]; then
    return 10
  fi
  if rg -q '#include <Interface_Graph.hxx>|[.]Sharings[(]' "$header"; then
    return 10
  fi
  for expected in \
    '# typed ambiguity rejected yes' \
    '    product definition usable on 1/2 files' \
    '        unusable on: 06-duplicate-product-definition.step' \
    '    definitions 46' \
    '    roots 1' \
    '    placed occurrences 139' \
    '    assemblies with equal children but distinct products 1 pair(s)' \
    '    same after reversed traversal yes' \
    '    foreign source entity rejected yes' \
    'identity index model scans 1  entities' \
    'non-transforming relationships 35' \
    'transforming relationships ignored 67' \
    'XDE product associations 46' \
    'ambiguous source identities 0' \
    '        durable key         step.product_definition#1' \
    '        durable key         step.product_definition#1764' \
    '        durable key         step.product_definition#2927' \
    '    product definition  present 46/46  unique yes  same on a second read yes'; do
    if ! grep -Fq "$expected" "$report"; then
      return 10
    fi
  done
  return 0
}

cargo_gate() {
  local test_name="$1"
  local log="$temporary/cargo-gate.log"
  local status
  if cargo test -p ferritecad-cli --test import_step "$test_name" \
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

expect_probe_kill() {
  local name="$1"
  set +e
  probe_gate
  local result=$?
  set -e
  case "$result" in
    10) echo "killed at runtime probe: $name"; killed=$((killed + 1)) ;;
    20) echo "compile refusal (not a runtime kill): $name" >&2; exit 1 ;;
    0) echo "survived unexpectedly: $name" >&2; exit 1 ;;
    *) echo "harness refusal $result: $name" >&2; exit 1 ;;
  esac
}

expect_cargo_kill() {
  local name="$1"
  local test_name="$2"
  set +e
  cargo_gate "$test_name"
  local result=$?
  set -e
  case "$result" in
    10) echo "killed at runtime importer gate: $name"; killed=$((killed + 1)) ;;
    20) echo "compile refusal (not a runtime kill): $name" >&2; exit 1 ;;
    30) echo "zero-test or malformed run refused: $name" >&2; exit 1 ;;
    0) echo "survived unexpectedly: $name" >&2; exit 1 ;;
    *) echo "harness refusal $result: $name" >&2; exit 1 ;;
  esac
}

expect_cargo_survival() {
  local name="$1"
  local test_name="$2"
  set +e
  cargo_gate "$test_name"
  local result=$?
  set -e
  if [ "$result" -ne 0 ]; then
    echo "metamorphic mutant $name should survive, result $result" >&2
    exit 1
  fi
  echo "survived as required: $name"
  survived=$((survived + 1))
}

apply_bridge_override() {
  local body="$1"
  local anchor=$'      keys = ferritecad::definition_keys(identity_index, definition_shapes,\n                                         definition_labels,\n                                         definition_assemblies);\n'
  replace_once "$bridge" "$anchor" "$anchor$body"
}

no_stale_backups "$root"

# Harness controls: missing and multiply-matched anchors, and stale backups.
printf 'one\n' > "$temporary/one.txt"
if replace_once "$temporary/one.txt" 'missing' 'x' >/dev/null 2>&1; then
  echo "anchor-miss control was accepted" >&2
  exit 1
fi
printf 'twice twice\n' > "$temporary/twice.txt"
if replace_once "$temporary/twice.txt" 'twice' 'x' >/dev/null 2>&1; then
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

cmake_generator=()
if command -v ninja >/dev/null 2>&1; then
  cmake_generator=(-G Ninja)
fi
cmake -S "$root/tools/step-key-probe" -B "$probe_build" \
  "${cmake_generator[@]}" -DCMAKE_BUILD_TYPE=Debug \
  >"$temporary/probe-configure.log"
cmake --build "$probe_build" --parallel >"$temporary/probe-build.log"
probe_binary="$(find "$probe_build" -type f \
  \( -name step_key_probe -o -name step_key_probe.exe \) -print -quit)"
if [ -z "$probe_binary" ] || [ ! -x "$probe_binary" ]; then
  echo "probe baseline did not build" >&2
  exit 1
fi
probe_gate
cargo_gate the_complex_ap203_assembly_becomes_a_durable_document
cargo_gate a_file_whose_parts_cannot_be_named_writes_no_document

set +e
cargo_gate __ferritecad_zero_test_control__
zero_result=$?
set -e
if [ "$zero_result" -ne 30 ]; then
  echo "zero-test control was not refused, result $zero_result" >&2
  exit 1
fi
echo "harness control: an actual zero-test cargo run was refused"

# A syntax error is a compile refusal and must never be credited to a runtime
# gate. This control is restored with the same mechanism as every mutant.
begin_mutation "$probe_source"
replace_once "$probe_source" 'namespace {' $'namespace {\nthis is not C++;'
set +e
probe_gate
compile_result=$?
set -e
if [ "$compile_result" -ne 20 ]; then
  echo "non-compiling control was not classified as compile refusal" >&2
  exit 1
fi
echo "harness control: non-compiling mutant refused before runtime"
restore_mutation

begin_mutation "$header"
replace_once "$header" 'if (transformed.IsNull()) {' 'if (false) {'
expect_probe_kill no_nontransforming_representation_relationship
restore_mutation

begin_mutation "$header"
replace_once "$header" 'if (transformed.IsNull()) {' 'if (!transformed.IsNull()) {'
expect_probe_kill transforming_relationship_used_as_ownership
restore_mutation

begin_mutation "$header"
replace_once "$header" \
  $'return products.size() == 1 ? products.front()\n                              : Handle(StepBasic_ProductDefinition)();' \
  $'return products.empty() ? Handle(StepBasic_ProductDefinition)()\n                          : products.front();'
expect_probe_kill first_product_definition_wins
restore_mutation

begin_mutation "$header"
replace_once "$header" 'products.size() == 1' 'products.size() >= 1'
expect_probe_kill typed_ambiguity_is_accepted
restore_mutation

begin_mutation "$header"
replace_once "$header" \
  'keys[i] = definition_key(index.source_ident(products[i]));' \
  'keys[i] = "step.definition#" + std::to_string(i + 1);'
expect_probe_kill key_by_definition_ordinal
restore_mutation

begin_mutation "$header"
replace_once "$header" \
  'keys[i] = definition_key(index.source_ident(products[i]));' \
  $'const auto geometry = entities_from(index.transfer(), shapes[i]);\n    keys[i] = geometry.empty()\n                  ? std::string()\n                  : definition_key(index.source_ident(geometry.front()));'
expect_probe_kill key_by_geometry_entity
restore_mutation

begin_mutation "$header"
replace_once "$header" 'StepIdentityMetrics metrics_;' 'mutable StepIdentityMetrics metrics_;'
replace_once "$header" \
  $'ProductCandidates products;\n    if (start.IsNull()) {' \
  $'ProductCandidates products;\n    ++metrics_.model_scans;\n    if (start.IsNull()) {'
expect_probe_kill graph_wide_work_repeated_for_each_definition
restore_mutation

begin_mutation "$probe_source"
replace_once "$probe_source" \
  $'const std::vector<std::string> durable_keys = ferritecad::definition_keys(\n      identity_index, definition_shapes, definitions, assemblies);' \
  $'std::vector<std::string> durable_keys = ferritecad::definition_keys(\n      identity_index, definition_shapes, definitions, assemblies);\n  for (std::string &key : durable_keys) {\n    key = "probe." + key;\n  }'
expect_probe_kill probe_updated_without_bridge
restore_mutation

begin_mutation "$bridge"
apply_bridge_override $'      for (std::size_t i = 0; i < keys.size(); ++i) {\n        keys[i] = label_name(definitions[i].label);\n      }\n'
expect_cargo_kill key_by_name the_complex_ap203_assembly_becomes_a_durable_document
restore_mutation

begin_mutation "$bridge"
apply_bridge_override $'      for (std::string &key : keys) {\n        key = "bridge." + key;\n      }\n'
expect_cargo_kill bridge_updated_without_probe the_complex_ap203_assembly_becomes_a_durable_document
restore_mutation

begin_mutation "$bridge"
apply_bridge_override $'      for (std::string &key : keys) {\n        if (key == "step.product_definition#2927") {\n          key = "step.product_definition#1764";\n        }\n      }\n'
expect_cargo_kill merge_equal_children_assemblies the_complex_ap203_assembly_becomes_a_durable_document
restore_mutation

begin_mutation "$bridge"
replace_once "$bridge" \
  'for (int i = 1; i <= roots.Length(); ++i) {' \
  'for (int i = 2; i <= roots.Length(); ++i) {'
expect_cargo_kill lose_root_assembly the_complex_ap203_assembly_becomes_a_durable_document
restore_mutation

begin_mutation "$bridge"
replace_once "$bridge" \
  $'for (int i = 1; i <= children.Length(); ++i) {\n            walk(children.Value(i), self,' \
  $'for (int i = 1; i < children.Length(); ++i) {\n            walk(children.Value(i), self,'
expect_cargo_kill lose_one_occurrence the_complex_ap203_assembly_becomes_a_durable_document
restore_mutation

begin_mutation "$bridge"
apply_bridge_override $'      for (std::size_t i = 0; i < keys.size(); ++i) {\n        const gp_Trsf placement = definitions[i].shape.Location().Transformation();\n        keys[i] = "step.placement#" + std::to_string(placement.Value(1, 4));\n      }\n'
expect_cargo_kill placement_used_as_identity the_complex_ap203_assembly_becomes_a_durable_document
restore_mutation

begin_mutation "$bridge"
replace_once "$bridge" \
  'if (nameless < keys.size()) {' \
  $'if (nameless < keys.size()) {\n      definitions.erase(definitions.begin() + static_cast<std::ptrdiff_t>(nameless));\n      keys.erase(keys.begin() + static_cast<std::ptrdiff_t>(nameless));\n    }\n    if (false && nameless < keys.size()) {'
expect_cargo_kill silently_drop_definition_without_key a_file_whose_parts_cannot_be_named_writes_no_document
restore_mutation

begin_mutation "$bridge"
replace_once "$bridge" \
  $'for (int i = 1; i <= children.Length(); ++i) {\n            walk(children.Value(i), self,\n                 shapes->GetShape(children.Value(i)).Location());\n          }' \
  $'for (int i = children.Length(); i >= 1; --i) {\n            walk(children.Value(i), self,\n                 shapes->GetShape(children.Value(i)).Location());\n          }'
expect_cargo_survival reversed_traversal_preserves_identity the_complex_ap203_assembly_becomes_a_durable_document
restore_mutation

probe_gate
cargo_gate the_complex_ap203_assembly_becomes_a_durable_document
cargo_gate a_file_whose_parts_cannot_be_named_writes_no_document
no_stale_backups "$root"

echo "mutation campaign: $killed runtime mutants killed"
echo "mutation campaign: $survived required metamorphic survivor"
echo "mutation campaign: compile refusal and zero-test controls were not credited"
