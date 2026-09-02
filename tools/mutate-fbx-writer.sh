#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Applies the §22B-1b2 FBX writer mutations. Every edit is restored
# byte-for-byte; compile failures and zero-test invocations are refused rather
# than credited as mutation kills.
#
# What is under test is the file: the measured version, axes and units, one
# conversion of positions and normals, the polygon order, the hierarchy and its
# local transforms, one geometry per definition however often it is placed, the
# material slots and the per-placement override, the omission properties, the
# report that cannot call a partial export complete, the one string escaping
# rule, and the absence of anything that would make two writes of one scene
# differ.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
writer="$root/crates/ferritecad-export/src/fbx/mod.rs"
contract="$root/crates/ferritecad-export/src/fbx/contract.rs"
syntax="$root/crates/ferritecad-export/src/fbx/syntax.rs"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-fbx-writer-mutations.XXXXXX")"
# No Open CASCADE and no Unity anywhere in this campaign. Every gate here runs
# against scenes built in memory, which is what lets the whole campaign be
# about the writer rather than about what is installed.
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

# 0 survived, 10 killed, 20 compile refusal, 30 zero-test or malformed run.
cargo_gate() {
  local gate="$1"
  local test_name="$2"
  local log="$temporary/${gate}.log"
  local status

  if [ "$gate" = "boundary" ]; then
    # The boundary is a mechanical check over the sources, so a mutation it
    # catches has no test count. It still has to compile: a writer that does
    # not build is not a writer that was caught.
    # The library only. A boundary mutation is about what the writer names,
    # and one that changes its signature stops the gates compiling without
    # saying anything about whether the boundary noticed.
    if ! cargo build -p ferritecad-export --lib >"$log" 2>&1; then
      return 20
    fi
    if tools/check-export-boundary.sh >>"$log" 2>&1; then
      return 0
    fi
    return 10
  fi

  case "$gate" in
    fbx)
      cargo test -p ferritecad-export --test fbx_ascii "$test_name" -- --nocapture \
        >"$log" 2>&1
      ;;
    model)
      cargo test -p ferritecad-export --lib "$test_name" -- --nocapture >"$log" 2>&1
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
    0) echo "survived as required (metamorphic): $name" ;;
    10) echo "a metamorphic change was killed, so a gate depends on it: $name" >&2; exit 1 ;;
    20) echo "compile refusal (not a runtime result): $name" >&2; exit 1 ;;
    30) echo "zero-test or malformed run refused: $name" >&2; exit 1 ;;
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
    0) echo "survived unexpectedly: $name" >&2; survived=$((survived + 1)); exit 1 ;;
    *) echo "harness refusal $result: $name" >&2; exit 1 ;;
  esac
}

baseline() {
  baseline_gate fbx the_file_says_it_is_fbx_7400_ascii_and_nothing_about_when_it_was_made
  baseline_gate fbx the_global_settings_are_the_one_measured_axis_and_unit_contract
  baseline_gate fbx the_geometry_is_converted_exactly_once_and_keeps_its_polygon_order
  baseline_gate fbx every_node_is_a_model_and_the_hierarchy_is_the_scenes
  baseline_gate fbx two_placements_of_one_definition_share_one_geometry
  baseline_gate fbx a_node_colour_override_is_a_binding_and_not_a_change_to_the_definition
  baseline_gate fbx an_omitted_definition_is_a_node_with_no_geometry_and_says_why
  baseline_gate fbx the_same_scene_always_produces_the_same_bytes
  baseline_gate fbx the_local_transform_is_converted_and_never_accumulated
  baseline_gate fbx a_name_the_format_cannot_spell_is_refused_rather_than_quietly_changed
  baseline_gate model every_placement_the_corpus_can_hold_survives_being_decomposed
  baseline_gate model a_placement_the_three_values_cannot_rebuild_is_refused
  baseline_gate model the_gimbal_lock_boundary_is_decomposed_rather_than_guessed
  baseline_gate model the_three_escaped_characters_survive_a_round_trip
  baseline_gate model a_name_this_format_cannot_spell_is_refused
  baseline_gate model zero_has_one_spelling_and_a_non_number_has_none
  baseline_gate boundary -
}

cd "$root"
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
cargo_gate fbx __ferritecad_zero_test_control__
zero_result=$?
set -e
if [ "$zero_result" -ne 30 ]; then
  echo "zero-test control was not refused, result $zero_result" >&2
  exit 1
fi
echo "harness control: an actual zero-test run was refused"

begin_mutation "$writer"
replace_once "$writer" \
  'const FBX_VERSION: i64 = 7400;' \
  $'const FBX_VERSION: i64 = 7400;\nthis is not Rust,'
set +e
cargo_gate fbx the_file_says_it_is_fbx_7400_ascii_and_nothing_about_when_it_was_made
compile_result=$?
set -e
if [ "$compile_result" -ne 20 ]; then
  echo "non-compiling control was not classified as compile refusal" >&2
  exit 1
fi
echo "harness control: non-compiling mutant refused before runtime"
restore_mutation

# ------------------------------------------------------------ format and units

# 1. A version this writer was never measured against.
begin_mutation "$writer"
replace_once "$writer" 'const FBX_VERSION: i64 = 7400;' 'const FBX_VERSION: i64 = 7300;'
expect_kill wrong_fbx_version fbx \
  the_file_says_it_is_fbx_7400_ascii_and_nothing_about_when_it_was_made
restore_mutation

# 2. Z up rather than the measured Y up.
begin_mutation "$writer"
replace_once "$writer" $'            ("UpAxis", 1),\n            ("UpAxisSign", 1),' \
  $'            ("UpAxis", 2),\n            ("UpAxisSign", 1),'
expect_kill wrong_axis_metadata fbx \
  the_global_settings_are_the_one_measured_axis_and_unit_contract
restore_mutation

# 3. Millimetre metadata on metre coordinates.
begin_mutation "$contract"
replace_once "$contract" 'pub(crate) const UNIT_SCALE_FACTOR: f64 = 100.0;' \
  'pub(crate) const UNIT_SCALE_FACTOR: f64 = 0.1;'
expect_kill wrong_unit_metadata fbx \
  the_global_settings_are_the_one_measured_axis_and_unit_contract
restore_mutation

# 4. Millimetres written as though they were metres.
begin_mutation "$contract"
replace_once "$contract" 'const MILLIMETRES_PER_METRE: f64 = 1000.0;' \
  'const MILLIMETRES_PER_METRE: f64 = 1.0;'
expect_kill mm_as_metres fbx the_geometry_is_converted_exactly_once_and_keeps_its_polygon_order
restore_mutation

# 5. The unit conversion applied twice.
begin_mutation "$contract"
replace_once "$contract" \
  $'    let [x, y, z] = value.map(f64::from);\n    [\n        x / MILLIMETRES_PER_METRE,\n        z / MILLIMETRES_PER_METRE,\n        -y / MILLIMETRES_PER_METRE,\n    ]\n}\n\n/// One direction' \
  $'    let [x, y, z] = value.map(f64::from);\n    [\n        x / MILLIMETRES_PER_METRE / MILLIMETRES_PER_METRE,\n        z / MILLIMETRES_PER_METRE / MILLIMETRES_PER_METRE,\n        -y / MILLIMETRES_PER_METRE / MILLIMETRES_PER_METRE,\n    ]\n}\n\n/// One direction'
expect_kill unit_conversion_applied_twice fbx \
  the_geometry_is_converted_exactly_once_and_keeps_its_polygon_order
restore_mutation

# 6. The measured Y sign dropped from the coordinate map.
begin_mutation "$contract"
replace_once "$contract" \
  $'        x / MILLIMETRES_PER_METRE,\n        z / MILLIMETRES_PER_METRE,\n        -y / MILLIMETRES_PER_METRE,\n    ]\n}\n\n/// One direction' \
  $'        x / MILLIMETRES_PER_METRE,\n        z / MILLIMETRES_PER_METRE,\n        y / MILLIMETRES_PER_METRE,\n    ]\n}\n\n/// One direction'
expect_kill wrong_y_sign fbx the_geometry_is_converted_exactly_once_and_keeps_its_polygon_order
restore_mutation

# 7. Positions converted, authored normals left in FerriteCAD axes.
begin_mutation "$contract"
replace_once "$contract" \
  $'    let [x, y, z] = value.map(f64::from);\n    [x, z, -y]\n}' \
  $'    let [x, y, z] = value.map(f64::from);\n    [x, y, z]\n}'
expect_kill normals_not_converted fbx \
  the_geometry_is_converted_exactly_once_and_keeps_its_polygon_order
restore_mutation

# ------------------------------------------------------------------- geometry

# 8. Polygon winding reversed, which the +1 determinant says it must not be.
begin_mutation "$writer"
replace_once "$writer" \
  $'                        (triangle[0], false),\n                        (triangle[1], false),\n                        (triangle[2], true),' \
  $'                        (triangle[2], false),\n                        (triangle[1], false),\n                        (triangle[0], true),'
expect_kill polygon_winding_flipped fbx \
  the_geometry_is_converted_exactly_once_and_keeps_its_polygon_order
restore_mutation

# 9. One geometry per placement rather than one per definition.
begin_mutation "$writer"
replace_once "$writer" \
  $'            geometries.push(index);' \
  $'            for _ in 0..scene.nodes().iter().filter(|n| n.definition.index() == index).count() {\n                geometries.push(index);\n            }'
expect_kill geometry_per_placement fbx \
  the_geometry_is_converted_exactly_once_and_keeps_its_polygon_order
restore_mutation

# 10. Only the first placement connected to the shared geometry.
begin_mutation "$writer"
replace_once "$writer" \
  $'                        Value::Int(model_id(node.id.index())?),\n                    ],\n                )?;\n            }\n        }\n        for (node, bound)' \
  $'                        Value::Int(model_id(node.id.index())?),\n                    ],\n                )?;\n                break;\n            }\n        }\n        for (node, bound)'
expect_kill only_the_first_placement fbx two_placements_of_one_definition_share_one_geometry
restore_mutation

# 11. Triangle material assignment thrown away.
begin_mutation "$writer"
replace_once "$writer" \
  '                .map(|slot| syntax::integer(i64::from(as_index(*slot)?))),' \
  '                .map(|slot| syntax::integer(i64::from(as_index(*slot)? * 0))),'
expect_kill triangle_material_indices_ignored fbx \
  the_geometry_is_converted_exactly_once_and_keeps_its_polygon_order
restore_mutation

# ------------------------------------------------------- hierarchy and transforms

# 12. Every node written at the top of the file.
begin_mutation "$writer"
replace_once "$writer" \
  $'            let parent = match node.parent {\n                None => 0,' \
  $'            let parent = match None::<crate::scene::ExportNodeId> {\n                None => 0,'
expect_kill hierarchy_flattened fbx every_node_is_a_model_and_the_hierarchy_is_the_scenes
restore_mutation

# 13. The accumulated transform written where the local one belongs.
begin_mutation "$writer"
replace_once "$writer" \
  '        let trs = contract::local_transform(&node.local_transform)?;' \
  $'        let mut trs = contract::local_transform(&node.local_transform)?;\n        if let Some(parent) = node.parent {\n            let outer = contract::local_transform(\n                &self.scene.node(parent).ok_or_else(|| CadError::topology("no parent"))?.local_transform,\n            )?;\n            for axis in 0..3 {\n                trs.translation[axis] += outer.translation[axis];\n            }\n        }'
expect_kill world_transform_as_local fbx the_local_transform_is_converted_and_never_accumulated
restore_mutation

# 14. The conjugation done the other way round.
begin_mutation "$contract"
replace_once "$contract" \
  $'    let permuted = [\n        [m[0][0], m[0][1], m[0][2]],\n        [m[2][0], m[2][1], m[2][2]],\n        [-m[1][0], -m[1][1], -m[1][2]],\n    ];' \
  $'    let permuted = [\n        [m[0][0], m[0][1], m[0][2]],\n        [-m[2][0], -m[2][1], -m[2][2]],\n        [m[1][0], m[1][1], m[1][2]],\n    ];'
expect_kill wrong_transform_conjugation model \
  every_placement_the_corpus_can_hold_survives_being_decomposed
restore_mutation

# 15. A rotation order other than the one declared.
begin_mutation "$contract"
replace_once "$contract" \
  $'    let sin_y = (-rotation[2][0]).clamp(-1.0, 1.0);' \
  $'    let sin_y = (rotation[0][2]).clamp(-1.0, 1.0);'
expect_kill wrong_euler_order model every_placement_the_corpus_can_hold_survives_being_decomposed
restore_mutation

# 16. The recomposition check removed, so a decomposition is a guess again.
begin_mutation "$contract"
replace_once "$contract" \
  $'    let rebuilt = euler_xyz(rotation_degrees);\n    let tolerance = TRANSFORM_TOLERANCE * scale.max(1.0);' \
  $'    if true {\n        return Ok(trs);\n    }\n    let rebuilt = euler_xyz(rotation_degrees);\n    let tolerance = TRANSFORM_TOLERANCE * scale.max(1.0);'
expect_kill recomposition_check_skipped model a_placement_the_three_values_cannot_rebuild_is_refused
restore_mutation

# ---------------------------------------------------------- names and identity

# 17. Two siblings a source called the same thing merged into one node.
begin_mutation "$writer"
replace_once "$writer" \
  $'        for node in self.scene.nodes() {\n            self.model(ascii, node)?;\n        }' \
  $'        let mut written: Vec<&str> = Vec::new();\n        for node in self.scene.nodes() {\n            let name = node.display_name.as_deref().unwrap_or_default();\n            if written.contains(&name) {\n                continue;\n            }\n            written.push(name);\n            self.model(ascii, node)?;\n        }'
expect_kill duplicate_names_merged fbx every_node_is_a_model_and_the_hierarchy_is_the_scenes
restore_mutation

# 18. Object identity taken from the display name.
begin_mutation "$writer"
replace_once "$writer" \
  $'fn model_id(index: usize) -> Result<i64> {\n    Ok(MODEL_BASE + as_int(index)?)\n}' \
  $'fn model_id(index: usize) -> Result<i64> {\n    Ok(MODEL_BASE + as_int(index)?)\n}\n\nfn model_id_of(name: &str, index: usize) -> Result<i64> {\n    let mut hash: i64 = 0;\n    for byte in name.bytes() {\n        hash = (hash * 31 + i64::from(byte)) % 1_000_000;\n    }\n    let _ = index;\n    Ok(MODEL_BASE + hash)\n}'
replace_once "$writer" \
  $'                Value::Int(model_id(node.id.index())?),\n                Value::Text(&format!("Model::{name}")),' \
  $'                Value::Int(model_id_of(name, node.id.index())?),\n                Value::Text(&format!("Model::{name}")),'
expect_kill identity_from_display_name fbx every_node_is_a_model_and_the_hierarchy_is_the_scenes
restore_mutation

# 19. A clock where a constant belongs.
begin_mutation "$writer"
replace_once "$writer" \
  'const CREATION_TIME: &str = "2000-01-01 00:00:00:000";' \
  $'fn creation_time() -> String {\n    let seconds = std::time::SystemTime::now()\n        .duration_since(std::time::UNIX_EPOCH)\n        .map(|elapsed| elapsed.as_secs())\n        .unwrap_or(0);\n    format!("2000-01-01 00:00:{seconds:03}")\n}'
replace_once "$writer" \
  'ascii.leaf("CreationTime", &[Value::Text(CREATION_TIME)])?;' \
  'ascii.leaf("CreationTime", &[Value::Text(&creation_time())])?;'
expect_kill timestamp_in_the_file fbx \
  the_file_says_it_is_fbx_7400_ascii_and_nothing_about_when_it_was_made
restore_mutation

# 20. And the same mutation seen by the mechanical boundary, which is what
# catches a clock whose value happens not to change during one test.
begin_mutation "$writer"
replace_once "$writer" \
  'const CREATION_TIME: &str = "2000-01-01 00:00:00:000";' \
  $'fn creation_time() -> String {\n    let seconds = std::time::SystemTime::now()\n        .duration_since(std::time::UNIX_EPOCH)\n        .map(|elapsed| elapsed.as_secs())\n        .unwrap_or(0);\n    format!("2000-01-01 00:00:{seconds:03}")\n}'
replace_once "$writer" \
  'ascii.leaf("CreationTime", &[Value::Text(CREATION_TIME)])?;' \
  'ascii.leaf("CreationTime", &[Value::Text(&creation_time())])?;'
expect_kill clock_reached_the_writer boundary -
restore_mutation

# 21. An unordered map iterated into the file.
begin_mutation "$writer"
replace_once "$writer" \
  $'        for node in self.scene.nodes() {\n            self.model(ascii, node)?;\n        }' \
  $'        let ordered: std::collections::HashMap<usize, &crate::scene::ExportNode> = self\n            .scene\n            .nodes()\n            .iter()\n            .map(|node| (node.id.index(), node))\n            .collect();\n        for (_, node) in ordered {\n            self.model(ascii, node)?;\n        }'
expect_kill unordered_map_iterated_into_the_file boundary -
restore_mutation

# ---------------------------------------------------------------- materials

# 22. Two measured slots collapsed into one.
begin_mutation "$writer"
replace_once "$writer" \
  $'            for slot in mesh.materials() {\n                definition_slots[index].push(push_material(' \
  $'            for slot in mesh.materials().iter().take(1) {\n                definition_slots[index].push(push_material('
expect_kill material_slots_collapsed fbx \
  a_node_colour_override_is_a_binding_and_not_a_change_to_the_definition
restore_mutation

# 23. A placement's colour written onto the definition every placement shares.
begin_mutation "$writer"
replace_once "$writer" \
  $'                (Some(mesh), Some(colour)) => {\n                    let mut bound = Vec::with_capacity(mesh.materials().len());\n                    for slot in mesh.materials() {\n                        bound.push(push_material(&mut materials, &slot.name, colour)?);\n                    }\n                    bound\n                }' \
  $'                (Some(mesh), Some(colour)) => {\n                    let bound = definition_slots[node.definition.index()].clone();\n                    for (position, _) in mesh.materials().iter().enumerate() {\n                        if let Some(target) = bound.get(position) {\n                            let mut encoded = [0.0; 3];\n                            for (component, value) in encoded.iter_mut().zip(colour) {\n                                *component = contract::srgb(value)?;\n                            }\n                            materials[*target].colour = encoded;\n                        }\n                    }\n                    bound\n                }'
expect_kill override_mutates_the_shared_definition fbx \
  a_node_colour_override_is_a_binding_and_not_a_change_to_the_definition
restore_mutation

# --------------------------------------------------------- structure and omissions

# 24. Assembly frames dropped from the file.
begin_mutation "$writer"
replace_once "$writer" \
  $'        for node in self.scene.nodes() {\n            self.model(ascii, node)?;\n        }' \
  $'        for node in self.scene.nodes() {\n            if self\n                .scene\n                .definition(node.definition)\n                .is_some_and(|definition| definition.geometry.is_structural())\n            {\n                continue;\n            }\n            self.model(ascii, node)?;\n        }'
expect_kill structural_nodes_dropped fbx every_node_is_a_model_and_the_hierarchy_is_the_scenes
restore_mutation

# 25. The omitted definition's placements dropped from the file.
begin_mutation "$writer"
replace_once "$writer" \
  $'        for node in self.scene.nodes() {\n            self.model(ascii, node)?;\n        }' \
  $'        for node in self.scene.nodes() {\n            if self\n                .scene\n                .definition(node.definition)\n                .and_then(|definition| definition.geometry.omission())\n                .is_some()\n            {\n                continue;\n            }\n            self.model(ascii, node)?;\n        }'
expect_kill omitted_nodes_dropped fbx an_omitted_definition_is_a_node_with_no_geometry_and_says_why
restore_mutation

# 26. Structural emptiness presented as a missing part.
begin_mutation "$writer"
replace_once "$writer" \
  '        if let ExportGeometry::Omitted(omission) = &definition.geometry {' \
  $'        if definition.geometry.is_structural() {\n            ascii.property(\n                "FerriteCADGeometryOmission",\n                "KString",\n                "",\n                "U",\n                &[Value::Text(&key)],\n            )?;\n        }\n        if let ExportGeometry::Omitted(omission) = &definition.geometry {'
expect_kill structure_marked_as_missing fbx \
  an_omitted_definition_is_a_node_with_no_geometry_and_says_why
restore_mutation

# 27. The omission written as a node with no explanation.
begin_mutation "$writer"
replace_once "$writer" \
  '        if let ExportGeometry::Omitted(omission) = &definition.geometry {' \
  '        if let ExportGeometry::Omitted(omission) = &definition.geometry && false {'
expect_kill omission_properties_dropped fbx \
  an_omitted_definition_is_a_node_with_no_geometry_and_says_why
restore_mutation

# 28. A partial export reported as a complete one.
begin_mutation "$writer"
replace_once "$writer" \
  '        omissions: plan.omissions,' \
  '        omissions: Vec::new(),'
expect_kill partial_export_called_complete fbx \
  an_omitted_definition_is_a_node_with_no_geometry_and_says_why
restore_mutation

# 29. `Debug` used as a data format for the typed refusal.
begin_mutation "$writer"
replace_once "$writer" \
  $'                &[Value::Text(omission.refusal.stable_name())],' \
  $'                &[Value::Text(&format!("{omission:?}"))],'
expect_kill debug_as_a_data_format fbx an_omitted_definition_is_a_node_with_no_geometry_and_says_why
restore_mutation

# ------------------------------------------------------------------- strings

# 30. Names written as they are, which changes what a reader gets back.
begin_mutation "$syntax"
replace_once "$syntax" \
  $'    let mut out = String::with_capacity(value.len());\n    for character in value.chars() {' \
  $'    if true {\n        return Ok(value.to_owned());\n    }\n    let mut out = String::with_capacity(value.len());\n    for character in value.chars() {'
expect_kill names_not_escaped model the_three_escaped_characters_survive_a_round_trip
restore_mutation

# 31. And the same mutation as the whole-file gate sees it.
begin_mutation "$syntax"
replace_once "$syntax" \
  $'    let mut out = String::with_capacity(value.len());\n    for character in value.chars() {' \
  $'    if true {\n        return Ok(value.to_owned());\n    }\n    let mut out = String::with_capacity(value.len());\n    for character in value.chars() {'
expect_kill unescaped_names_reach_the_file fbx \
  a_name_the_format_cannot_spell_is_refused_rather_than_quietly_changed
restore_mutation

# 32. A name this format cannot spell accepted rather than refused.
begin_mutation "$syntax"
replace_once "$syntax" \
  $'    for entity in ENTITIES {\n        if value.contains(entity) {' \
  $'    for entity in ENTITIES {\n        if false && value.contains(entity) {'
expect_kill unspellable_name_accepted model a_name_this_format_cannot_spell_is_refused
restore_mutation

# 33. A value that is not a number written into the file.
begin_mutation "$syntax"
replace_once "$syntax" \
  $'    if !value.is_finite() {\n        return Err(CadError::unsupported(format!(\n            "the export produced {value}, which is not a number any file can record"\n        )));\n    }' \
  $'    if false {\n        return Err(CadError::unsupported(format!(\n            "the export produced {value}, which is not a number any file can record"\n        )));\n    }'
expect_kill non_finite_output_accepted model zero_has_one_spelling_and_a_non_number_has_none
restore_mutation

# 34. Signed zero left in the file, so two equal scenes are two files.
begin_mutation "$syntax"
replace_once "$syntax" \
  '    let canonical = if value == 0.0 { 0.0 } else { value };' \
  '    let canonical = value;'
expect_kill signed_zero_reaches_the_file fbx the_same_scene_always_produces_the_same_bytes
restore_mutation

# --------------------------------------------------------------- the boundary

# 35. A kernel reached from the writer.
begin_mutation "$writer"
replace_once "$writer" \
  $'use ferritecad_types::{CadError, Result};' \
  $'#[allow(unused_imports)]\nuse ferritecad_kernel::GeometryKernel;\nuse ferritecad_types::{CadError, Result};'
expect_kill kernel_reached_from_the_writer boundary -
restore_mutation

# 36. A document reached from the writer.
begin_mutation "$writer"
replace_once "$writer" \
  $'use ferritecad_types::{CadError, Result};' \
  $'#[allow(dead_code)]\ntype Document = u8;\n#[allow(dead_code)]\ntype RenderSnapshot = u8;\nuse ferritecad_types::{CadError, Result};'
expect_kill document_and_snapshot_reached_from_the_writer boundary -
restore_mutation

# 37. The writer given a third thing to be told.
begin_mutation "$writer"
replace_once "$writer" \
  $'    scene: &ExportScene,\n    output: &mut impl Write,\n) -> Result<FbxWriteReport> {' \
  $'    scene: &ExportScene,\n    output: &mut impl Write,\n    complete: bool,\n) -> Result<FbxWriteReport> {'
expect_kill writer_given_an_external_verdict boundary -
restore_mutation

# A metamorphic change, which must survive. Which order the two material
# objects of one definition are created in is a fact about the slots, not about
# the loop that walks them, so naming the temporary differently must change
# nothing. A gate this killed would be a gate depending on an identifier.
begin_mutation "$writer"
replace_once "$writer" \
  $'            for slot in mesh.materials() {\n                definition_slots[index].push(push_material(\n                    &mut materials,\n                    &slot.name,\n                    slot.base_colour_linear,\n                )?);\n            }' \
  $'            for declared in mesh.materials() {\n                definition_slots[index].push(push_material(\n                    &mut materials,\n                    &declared.name,\n                    declared.base_colour_linear,\n                )?);\n            }'
expect_survivor material_slot_loop_renamed fbx \
  a_node_colour_override_is_a_binding_and_not_a_change_to_the_definition
restore_mutation

baseline
no_stale_backups "$root"

echo "mutation campaign: $killed mutants killed"
echo "mutation campaign: $survived unexpected survivors"
echo "mutation campaign: compile refusal and zero-test controls were not credited"
