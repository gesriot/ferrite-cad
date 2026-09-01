#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Applies the §22B-1b1 export-scene mutations. Every edit is restored
# byte-for-byte; compile failures and zero-test invocations are refused rather
# than credited as mutation kills.
#
# What is under test is the boundary a future FBX writer is handed: one
# preparation spine behind both the picture and the export, definitions that
# own geometry once, nodes that keep their parent and their exact local
# placement, three distinct geometry states, and a refusal for anything a
# static-mesh hierarchy cannot express.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
model="$root/crates/ferritecad-export/src/scene.rs"
builder="$root/crates/ferritecad-scene/src/export.rs"
spine="$root/crates/ferritecad-scene/src/prepare.rs"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-export-scene-mutations.XXXXXX")"
# No Open CASCADE anywhere in this campaign. Every gate here runs against the
# mock kernel, which is what lets the whole campaign be about the boundary
# rather than about whether a particular tessellator is installed.
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
    scene)
      cargo test -p ferritecad-scene --test export_scene "$test_name" -- --nocapture \
        >"$log" 2>&1
      ;;
    picture)
      cargo test -p ferritecad-scene --lib "$test_name" -- --nocapture >"$log" 2>&1
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
    10) echo "killed at runtime $gate gate: $name"; killed=$((killed + 1)) ;;
    20) echo "compile refusal (not a runtime kill): $name" >&2; exit 1 ;;
    30) echo "zero-test or malformed run refused: $name" >&2; exit 1 ;;
    0) echo "survived unexpectedly: $name" >&2; survived=$((survived + 1)); exit 1 ;;
    *) echo "harness refusal $result: $name" >&2; exit 1 ;;
  esac
}

baseline() {
  baseline_gate scene one_native_body_is_one_definition_one_node_and_one_mesh
  baseline_gate scene two_bodies_with_one_name_and_one_shape_are_two_definitions
  baseline_gate scene authored_normals_and_a_material_colour_survive
  baseline_gate scene a_flat_assembly_keeps_its_frame_and_both_local_placements
  baseline_gate scene a_nested_assembly_keeps_every_parent_and_exact_local_transform
  baseline_gate scene two_objects_storing_the_same_bytes_share_definitions_and_keep_every_node
  baseline_gate scene one_key_in_two_sources_stays_two_definitions
  baseline_gate scene deleting_the_source_file_before_exporting_changes_nothing
  baseline_gate scene a_structural_definition_is_not_reported_as_an_omission
  baseline_gate scene matching_persisted_and_fresh_evidence_permits_the_known_omission
  baseline_gate scene a_current_refusal_without_matching_persisted_evidence_stops_the_build
  baseline_gate scene an_unrelated_kernel_failure_stops_the_build_even_with_both_observations
  baseline_gate scene a_placement_no_static_mesh_format_can_express_is_a_typed_refusal
  baseline_gate scene cancelling_produces_no_partial_scene_and_leaks_no_shapes
  baseline_gate scene one_export_solves_once_reads_each_source_once_and_meshes_each_definition_once
  baseline_gate scene the_debug_output_names_no_transient_identity
  baseline_gate scene the_render_snapshot_of_the_same_document_is_unchanged
  baseline_gate picture a_stored_assembly_is_drawn_once_per_place_it_appears
  baseline_gate model the_measured_two_slot_reference_is_expressible
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
cargo_gate scene __ferritecad_zero_test_control__
zero_result=$?
set -e
if [ "$zero_result" -ne 30 ]; then
  echo "zero-test control was not refused, result $zero_result" >&2
  exit 1
fi
echo "harness control: an actual zero-test run was refused"

begin_mutation "$model"
replace_once "$model" \
  'pub struct ExportTransform {' \
  $'pub struct ExportTransform {\n    this is not Rust,'
set +e
cargo_gate model a_representable_placement_is_kept_exactly
compile_result=$?
set -e
if [ "$compile_result" -ne 20 ]; then
  echo "non-compiling control was not classified as compile refusal" >&2
  exit 1
fi
echo "harness control: non-compiling mutant refused before runtime"
restore_mutation

# ---------------------------------------------------------------- hierarchy

# 1. Build the export the way a RenderSnapshot describes a model: frames
# dropped and every placement already multiplied out.
begin_mutation "$builder"
replace_once "$builder" \
  $'    fn node(&mut self, node: &PreparedNode) -> Result<()> {\n        self.nodes.push(node.clone());' \
  $'    fn node(&mut self, node: &PreparedNode) -> Result<()> {\n        if node.structural {\n            return Ok(());\n        }\n        self.nodes.push(node.clone());'
replace_once "$builder" \
  '            let local = ExportTransform::new(*node.local.rows())?;' \
  '            let local = ExportTransform::new(*node.world.rows())?;'
expect_kill built_from_the_flattened_picture scene \
  a_nested_assembly_keeps_every_parent_and_exact_local_transform
restore_mutation

# 2. Keep every node but throw the tree away.
begin_mutation "$builder"
replace_once "$builder" \
  $'            let parent = match node.parent {\n                None => None,' \
  $'            let parent = match None::<usize> {\n                None => None,'
expect_kill hierarchy_flattened_to_one_level scene \
  a_nested_assembly_keeps_every_parent_and_exact_local_transform
restore_mutation

# 3. Keep the tree but write the accumulated placement into it.
begin_mutation "$builder"
replace_once "$builder" \
  '            let local = ExportTransform::new(*node.local.rows())?;' \
  '            let local = ExportTransform::new(*node.world.rows())?;'
expect_kill world_transform_stored_as_local scene \
  a_nested_assembly_keeps_every_parent_and_exact_local_transform
restore_mutation

# 4. One definition, one place it appears.
begin_mutation "$builder"
replace_once "$builder" \
  $'    fn node(&mut self, node: &PreparedNode) -> Result<()> {\n        self.nodes.push(node.clone());' \
  $'    fn node(&mut self, node: &PreparedNode) -> Result<()> {\n        if self\n            .nodes\n            .iter()\n            .any(|earlier| earlier.definition == node.definition)\n        {\n            return Ok(());\n        }\n        self.nodes.push(node.clone());'
expect_kill only_the_first_placement_kept scene \
  a_flat_assembly_keeps_its_frame_and_both_local_placements
restore_mutation

# 5. Nodes in some other order.
begin_mutation "$builder"
replace_once "$builder" \
  $'        let mut ids: Vec<ExportNodeId> = Vec::with_capacity(self.nodes.len());\n        for node in &self.nodes {' \
  $'        let mut ids: Vec<ExportNodeId> = Vec::with_capacity(self.nodes.len());\n        let mut reordered = self.nodes.clone();\n        let last = reordered.len();\n        if last >= 2 {\n            reordered.swap(last - 2, last - 1);\n        }\n        for node in &reordered {'
expect_kill nodes_reordered scene \
  a_nested_assembly_keeps_every_parent_and_exact_local_transform
restore_mutation

# ---------------------------------------------------------------- identity

# 6. One geometry per occurrence rather than per definition.
begin_mutation "$spine"
replace_once "$spine" \
  '        if let Some(&index) = self.known.get(&item) {' \
  '        if let Some(&index) = None::<&usize> {'
expect_kill geometry_duplicated_per_occurrence scene \
  one_export_solves_once_reads_each_source_once_and_meshes_each_definition_once
restore_mutation

# 7. An imported definition keyed by its file-local key alone.
begin_mutation "$spine"
replace_once "$spine" \
  $'        let item = SceneItem::Imported(ImportedDefinitionRef::new(\n            from.source,' \
  $'        let item = SceneItem::Imported(ImportedDefinitionRef::new(\n            ferritecad_types::ImportedSourceId::from_bytes([\n                1, 153, 0, 0, 0, 0, 112, 0, 128, 0, 0, 0, 0, 0, 0, 1,\n            ])\n            .expect("a fixed identifier"),'
expect_kill imported_definitions_keyed_without_their_source scene \
  one_key_in_two_sources_stays_two_definitions
restore_mutation

# 8. Definitions keyed by what they are called.
begin_mutation "$model"
replace_once "$model" \
  $'        if let Some(earlier) = self\n            .definitions\n            .iter()\n            .find(|definition| definition.source == source)\n        {' \
  $'        if let Some(earlier) = self.definitions.iter().find(|definition| {\n            definition.display_name.is_some() && definition.display_name == display_name\n        }) {\n            return Ok(earlier.id);\n        }\n        if let Some(earlier) = self\n            .definitions\n            .iter()\n            .find(|definition| definition.source == source)\n        {'
expect_kill definitions_keyed_by_display_name scene \
  two_bodies_with_one_name_and_one_shape_are_two_definitions
restore_mutation

# 9. Two assembly frames taken for one.
begin_mutation "$model"
replace_once "$model" \
  $'        if let Some(earlier) = self\n            .definitions\n            .iter()\n            .find(|definition| definition.source == source)\n        {' \
  $'        if geometry.is_structural()\n            && let Some(earlier) = self\n                .definitions\n                .iter()\n                .find(|definition| definition.geometry.is_structural())\n        {\n            return Ok(earlier.id);\n        }\n        if let Some(earlier) = self\n            .definitions\n            .iter()\n            .find(|definition| definition.source == source)\n        {'
expect_kill distinct_assembly_definitions_merged scene \
  a_nested_assembly_keeps_every_parent_and_exact_local_transform
restore_mutation

# ------------------------------------------------------ the three states

# 10. A node that draws nothing is dropped.
begin_mutation "$builder"
replace_once "$builder" \
  $'    fn node(&mut self, node: &PreparedNode) -> Result<()> {\n        self.nodes.push(node.clone());' \
  $'    fn node(&mut self, node: &PreparedNode) -> Result<()> {\n        if self.omissions.contains_key(&node.definition) {\n            return Ok(());\n        }\n        self.nodes.push(node.clone());'
expect_kill empty_hierarchy_node_lost scene \
  matching_persisted_and_fresh_evidence_permits_the_known_omission
restore_mutation

# 11. Structure reported as a missing part.
begin_mutation "$builder"
replace_once "$builder" \
  $'        if prepared.structural {\n            // Every placement of it holds other placements, so its own shape is\n            // the compound of what is inside it and belongs to the parts rather\n            // than to the frame.\n            return Ok(ExportGeometry::Structural);\n        }' \
  $'        if prepared.structural {\n            return Ok(ExportGeometry::Omitted(ExportOmission::new(\n                ferritecad_exchange::Diagnostic {\n                    stage: ferritecad_exchange::Stage::Validation,\n                    severity: ferritecad_exchange::Severity::Fail,\n                    entity: String::new(),\n                    message: String::new(),\n                },\n                TessellationRefusal::IncompleteFace,\n            )));\n        }'
expect_kill structural_emptiness_reported_as_an_omission scene \
  a_structural_definition_is_not_reported_as_an_omission
restore_mutation

# 12. A missing part written as an ordinary mesh.
begin_mutation "$builder"
replace_once "$builder" \
  $'            prepare::Geometry::Omitted(refusal) => {\n                self.omissions.insert(definition, refusal);\n            }' \
  $'            prepare::Geometry::Omitted(_) => {\n                self.triangles.insert(\n                    definition,\n                    Triangles {\n                        positions: vec![[0.0; 3]],\n                        normals: vec![[0.0, 0.0, 1.0]],\n                        triangles: vec![[0, 0, 0]],\n                    },\n                );\n            }'
expect_kill omission_written_as_an_ordinary_mesh scene \
  matching_persisted_and_fresh_evidence_permits_the_known_omission
restore_mutation

# ------------------------------------------------------------- the policy

# 13. Every tessellation failure permitted.
begin_mutation "$spine"
replace_once "$spine" \
  $'    match TessellationRefusal::of(reason)? {\n        TessellationRefusal::IncompleteFace => Some(TessellationRefusal::IncompleteFace),\n        _ => None,\n    }' \
  $'    let _ = reason;\n    Some(TessellationRefusal::IncompleteFace)'
expect_kill any_tessellation_failure_accepted_as_omission scene \
  an_unrelated_kernel_failure_stops_the_build_even_with_both_observations
restore_mutation

# 14. One observation is enough.
begin_mutation "$spine"
replace_once "$spine" \
  $'                            is_topology_failure(diagnostic)\n                                && reopened.diagnostics_now.iter().any(|current| {\n                                    is_topology_failure(current)\n                                        && current.entity == diagnostic.entity\n                                })' \
  '                            is_topology_failure(diagnostic)'
expect_kill persisted_and_fresh_agreement_ignored scene \
  a_current_refusal_without_matching_persisted_evidence_stops_the_build
restore_mutation

# ------------------------------------------------------------ one reading

# 15. The external file is needed after all.
begin_mutation "$spine"
replace_once "$spine" \
  $'    fn import(&mut self, source: &[u8]) -> Result<Import> {\n        (self.read)(self.kernel, source)\n    }' \
  $'    fn import(&mut self, source: &[u8]) -> Result<Import> {\n        let _ = source;\n        let source = std::fs::read("the-file-this-was-imported-from.step")\n            .map_err(|error| CadError::io("reopening the external STEP file", error))?;\n        (self.read)(self.kernel, &source)\n    }'
expect_kill external_step_path_reopened scene \
  deleting_the_source_file_before_exporting_changes_nothing
restore_mutation

# 16. The stored source read twice.
begin_mutation "$spine"
replace_once "$spine" \
  '                    imported.extend(reopened.scene.shapes());' \
  $'                    imported.extend(reopened.scene.shapes());\n                    let again = {\n                        let mut reader = Reader {\n                            kernel: &mut *kernel,\n                            read: &mut read_step,\n                        };\n                        document.reopen_step_import(object.id, &mut reader)?\n                    };\n                    imported.extend(again.scene.shapes());'
expect_kill stored_step_read_twice scene \
  one_export_solves_once_reads_each_source_once_and_meshes_each_definition_once
restore_mutation

# 17. A second cold rebuild.
begin_mutation "$spine"
replace_once "$spine" \
  '    let built = rebuild_cold(&document, kernel, &building)?;' \
  $'    let built = rebuild_cold(&document, kernel, &building)?;\n    rebuild_cold(&document, kernel, &building)?.release_all(kernel);'
expect_kill second_cold_rebuild scene \
  one_export_solves_once_reads_each_source_once_and_meshes_each_definition_once
restore_mutation

# ----------------------------------------------------------- the geometry

# 18. Normals recomputed rather than carried.
begin_mutation "$builder"
replace_once "$builder" \
  $'            normals: mesh\n                .normals\n                .chunks_exact(3)\n                .map(|value| [value[0], value[1], value[2]])\n                .collect(),' \
  '            normals: vec![[0.0, 0.0, 1.0]; mesh.positions.len() / 3],'
expect_kill authored_normals_discarded scene \
  authored_normals_and_a_material_colour_survive
restore_mutation

# 19. One slot for everything.
begin_mutation "$model"
replace_once "$model" \
  $'        Ok(Self {\n            positions,\n            normals,\n            triangles,\n            triangle_materials,\n            materials,\n        })' \
  $'        Ok(Self {\n            triangle_materials: vec![0; triangles.len()],\n            materials: materials[..1].to_vec(),\n            positions,\n            normals,\n            triangles,\n        })'
expect_kill material_slots_collapsed model \
  the_measured_two_slot_reference_is_expressible
restore_mutation

# 20. A transient handle carried out of the session that issued it.
begin_mutation "$spine"
replace_once "$spine" \
  '            name: Some(definition.name.clone()),' \
  '            name: Some(format!("{:?} {}", definition.shape, definition.name)),'
expect_kill transient_handle_leaked_into_the_export scene \
  the_debug_output_names_no_transient_identity
restore_mutation

# 21. A placement nothing can express, accepted.
begin_mutation "$model"
replace_once "$model" \
  $'    pub fn new(rows: [[f64; 4]; 3]) -> Result<Self> {\n        for value in rows.iter().flatten() {' \
  $'    pub fn new(rows: [[f64; 4]; 3]) -> Result<Self> {\n        if true {\n            return Ok(Self { rows });\n        }\n        for value in rows.iter().flatten() {'
expect_kill non_representable_placement_accepted scene \
  a_placement_no_static_mesh_format_can_express_is_a_typed_refusal
restore_mutation

# ---------------------------------------------------------------- ownership

# 22. Imported shapes kept after a successful export.
begin_mutation "$spine"
replace_once "$spine" \
  $'    for shape in imported.into_iter().rev() {\n        kernel.release(shape);\n    }' \
  '    let _ = imported;'
expect_kill imported_shapes_leaked_on_success scene \
  a_flat_assembly_keeps_its_frame_and_both_local_placements
restore_mutation

# 23. Rebuild shapes kept after a successful export.
begin_mutation "$spine"
replace_once "$spine" \
  '    built.release_all(kernel);' \
  '    std::mem::forget(built);'
expect_kill rebuild_shapes_leaked_on_success scene \
  one_native_body_is_one_definition_one_node_and_one_mesh
restore_mutation

# 24. Shapes kept when the export does not finish.
begin_mutation "$spine"
replace_once "$spine" \
  $'    for shape in imported.into_iter().rev() {\n        kernel.release(shape);\n    }' \
  $'    if output.is_ok() {\n        for shape in imported.into_iter().rev() {\n            kernel.release(shape);\n        }\n    }'
expect_kill shapes_leaked_on_cancellation scene \
  cancelling_produces_no_partial_scene_and_leaks_no_shapes
restore_mutation

# A metamorphic change, which must survive. Which placement a definition's own
# colour was read from is not a fact about the definition: every placement that
# says the colour came from the definition has to agree, so reading them the
# other way round must produce the same material. A gate that this killed would
# be a gate depending on placement order.
begin_mutation "$builder"
replace_once "$builder" \
  '    for node in places {' \
  '    for node in places.iter().rev() {'
expect_survivor definition_colour_read_in_the_other_order scene \
  a_nested_assembly_keeps_every_parent_and_exact_local_transform
restore_mutation

baseline
no_stale_backups "$root"

echo "mutation campaign: $killed runtime mutants killed"
echo "mutation campaign: $survived unexpected survivors"
echo "mutation campaign: compile refusal and zero-test controls were not credited"
