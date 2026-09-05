#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Applies the §22B-1e3a occurrence-identity mutations. Every edit is restored
# byte-for-byte; compile failures, zero-test runs and zero-check runs are
# refused rather than credited as mutation kills.
#
# What is under test is the one claim of the slice: after the first save of a
# newly imported STEP document, every placement has a durable identity that the
# *document* owns, that reaches the neutral export boundary, and that is derived
# from nothing — not an ordinal, a parent, a traversal order, a display name, a
# transform, a definition key, or anything a writer later numbers. Every way of
# getting that wrong compiles.
#
# No Open CASCADE anywhere in this campaign. Every gate here runs against the
# mock kernel or against pure arithmetic, which is what lets the whole campaign
# be about identity rather than about whether a particular tessellator is
# installed. The one gate that needs a real kernel — the 140-node assembly end
# to end through the shipped commands — is a test, not a mutant target.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
persist="$root/crates/ferritecad-exchange/src/persist.rs"
document="$root/crates/ferritecad-document/src/document.rs"
payload="$root/crates/ferritecad-document/src/model.rs"
spine="$root/crates/ferritecad-scene/src/prepare.rs"
builder="$root/crates/ferritecad-scene/src/export.rs"
model="$root/crates/ferritecad-export/src/scene.rs"
writer="$root/crates/ferritecad-export/src/fbx/mod.rs"
digests="$root/tools/fbx/digests.tsv"
temporary="$(mktemp -d "${TMPDIR:-/tmp}/ferritecad-occurrence-mutations.XXXXXX")"

mutation_files=()
mutation_hashes=()
mutation_mtimes=()
killed=0
survived=0

# A UUIDv7 spelled out as bytes, so a mutant can produce a well-formed
# identifier from something it must not be derived from. The variant and
# version nibbles are what `OccurrenceId` checks; everything else is payload.
readonly FIXED_V7='1, 153, 0, 0, 0, 0, 112, 0, 128, 0, 0, 0, 0, 0, 0'

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

# One gate run. Returns 0 when it passed, 10 when a test failed at runtime, 20
# when the mutant did not compile, and 30 when the run tested nothing.
cargo_gate() {
  local gate="$1"
  local test_name="$2"
  local log="$temporary/${gate}.log"
  local status
  case "$gate" in
    scene)
      cargo test -p ferritecad-scene --test export_scene "$test_name" -- --exact --nocapture \
        >"$log" 2>&1
      ;;
    exchange)
      cargo test -p ferritecad-exchange --lib "$test_name" -- --nocapture >"$log" 2>&1
      ;;
    document)
      cargo test -p ferritecad-document --test imported_step "$test_name" -- --exact \
        --nocapture >"$log" 2>&1
      ;;
    writer)
      cargo test -p ferritecad-export --test fbx_ascii "$test_name" -- --exact --nocapture \
        >"$log" 2>&1
      ;;
    bytes)
      # The committed digests of the production FBX bytes, recomputed from the
      # writer through the same example the shell gate uses. A slice that
      # promises the file does not change is a slice whose file does not change.
      bytes_gate >"$log" 2>&1
      ;;
    *)
      echo "unknown mutation gate: $gate" >&2
      return 40
      ;;
  esac
  status=$?

  if [ "$gate" = "bytes" ]; then
    if grep -q 'could not compile\|error: building' "$log"; then
      return 20
    fi
    if ! grep -q '^FCAD_FBX_BYTES_CHECKED 2$' "$log"; then
      return 30
    fi
    if [ "$status" -eq 0 ]; then
      return 0
    fi
    return 10
  fi

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

# Writes both production FBX scenes and compares them with the committed
# digests. Prints how many files it actually checked, so a run that compared
# nothing cannot pass for a comparison.
bytes_gate() {
  local out="$temporary/fbx"
  rm -rf "$out"
  mkdir -p "$out"
  cargo run --quiet --example fbx_gate_artefacts -p ferritecad-export -- "$out" || return 1
  [ -f "$digests" ] || { echo "the recorded digests are missing"; return 1; }
  local checked=0 expected name actual failed=0
  while IFS=$'\t' read -r expected name; do
    [ -n "$name" ] || continue
    [ -f "$out/$name" ] || { echo "the writer produced no $name"; return 1; }
    actual="$(digest "$out/$name")"
    checked=$((checked + 1))
    if [ "$actual" != "$expected" ]; then
      echo "$name is $actual here and $expected in the recorded digests"
      failed=1
    fi
  done <"$digests"
  echo "FCAD_FBX_BYTES_CHECKED $checked"
  return "$failed"
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
    30) echo "zero-test or zero-check run refused: $name" >&2; exit 1 ;;
    0) echo "survived unexpectedly: $name" >&2; survived=$((survived + 1)); exit 1 ;;
    *) echo "harness refusal $result: $name" >&2; exit 1 ;;
  esac
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
    30) echo "zero-test or zero-check run refused: $name" >&2; exit 1 ;;
    *) echo "harness refusal $result: $name" >&2; exit 1 ;;
  esac
}

baseline() {
  baseline_gate exchange one_scene_persisted_twice_mints_two_sets_of_identities
  baseline_gate exchange two_placements_of_one_definition_are_two_identities
  baseline_gate exchange binding_does_not_change_the_stored_identities
  baseline_gate exchange two_placements_claiming_one_identity_are_refused_before_anything_binds
  baseline_gate exchange \
    a_version_2_scene_offers_its_keys_and_says_it_has_no_placement_identities
  baseline_gate document a_document_written_before_placements_had_identities_still_opens_and_binds
  baseline_gate document a_current_layout_payload_with_no_placement_identities_is_refused
  baseline_gate document two_placements_answering_to_one_identity_are_refused_before_anything_is_read
  baseline_gate document a_placement_identity_that_is_not_a_uuidv7_is_refused_while_it_is_read
  baseline_gate document an_import_is_stored_at_the_current_layout_and_declares_no_new_capability
  baseline_gate scene two_placements_of_one_definition_are_two_identities_on_one_shared_mesh
  baseline_gate scene identical_names_transforms_and_keys_do_not_collapse_two_placements
  baseline_gate scene the_same_identities_come_back_from_every_cold_rebuild_and_every_session
  baseline_gate scene the_identity_of_each_node_is_the_one_stored_for_that_place
  baseline_gate scene a_fresh_reading_of_the_same_bytes_does_not_replace_the_stored_identities
  baseline_gate scene two_objects_storing_the_same_bytes_keep_their_own_placement_identities
  baseline_gate scene a_native_body_is_identified_by_its_object_and_not_by_a_fresh_uuid
  baseline_gate scene a_document_written_before_placements_had_identities_says_so_and_still_exports
  baseline_gate scene placement_identities_that_were_swapped_or_stolen_are_refused_before_any_export
  baseline_gate scene one_identity_naming_two_placements_is_refused_at_the_export_boundary_too
  baseline_gate scene \
    a_placement_of_a_definition_with_no_triangles_still_has_its_stored_identity
  baseline_gate scene one_export_solves_once_reads_each_source_once_and_meshes_each_definition_once
  baseline_gate scene a_nested_assembly_keeps_every_parent_and_exact_local_transform
  baseline_gate writer a_placement_identity_changes_nothing_the_writer_writes
  baseline_gate bytes -
}

# ------------------------------------------------------- harness controls

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

# And a bytes gate that compares nothing is refused too, on its own terms: the
# gate reads a list of digests, and an empty list would pass silently.
saved_digests="$temporary/digests.tsv"
cp "$digests" "$saved_digests"
: >"$digests"
set +e
cargo_gate bytes -
empty_result=$?
set -e
cp "$saved_digests" "$digests"
if [ "$(digest "$digests")" != "$(digest "$saved_digests")" ]; then
  echo "the digest list was not restored" >&2
  exit 1
fi
if [ "$empty_result" -ne 30 ]; then
  echo "zero-check bytes control was not refused, result $empty_result" >&2
  exit 1
fi
echo "harness control: a bytes gate that compared nothing was refused"

begin_mutation "$model"
replace_once "$model" \
  'pub enum ExportOccurrence {' \
  $'pub enum ExportOccurrence {\n    this is not Rust,'
set +e
cargo_gate scene the_identity_of_each_node_is_the_one_stored_for_that_place
compile_result=$?
set -e
if [ "$compile_result" -ne 20 ]; then
  echo "non-compiling control was not classified as compile refusal" >&2
  exit 1
fi
echo "harness control: non-compiling mutant refused before runtime"
restore_mutation

# ================================================== where an identity comes from

# 1. Minted at every open, from the reading rather than from the payload.
begin_mutation "$document"
replace_once "$document" \
  '        let occurrences = stored.imported.scene.occurrences();' \
  $'        let occurrences = StoredOccurrences::Recorded(\n            scene\n                .instances\n                .iter()\n                .map(|_| ferritecad_types::OccurrenceId::new())\n                .collect(),\n        );'
expect_kill minted_at_every_open scene \
  the_same_identities_come_back_from_every_cold_rebuild_and_every_session
restore_mutation

# 2. Minted at every export, on the way to the neutral boundary.
begin_mutation "$builder"
replace_once "$builder" \
  '        NodeIdentity::Occurrence(occurrence) => ExportOccurrence::Occurrence(occurrence),' \
  $'        NodeIdentity::Occurrence(_) => {\n            ExportOccurrence::Occurrence(ferritecad_types::OccurrenceId::new())\n        }'
expect_kill minted_at_every_export scene \
  the_same_identities_come_back_from_every_cold_rebuild_and_every_session
restore_mutation

# 3. Derived from the ordinal the placement happens to sit at.
begin_mutation "$persist"
replace_once "$persist" \
  '                occurrence: OccurrenceId::new(),' \
  "                occurrence: OccurrenceId::from_bytes([${FIXED_V7}, index as u8])
                    .expect(\"a well-formed identifier\"),"
expect_kill derived_from_the_instance_ordinal scene \
  a_fresh_reading_of_the_same_bytes_does_not_replace_the_stored_identities
restore_mutation

# 4. Derived from where the placement sits in the tree.
begin_mutation "$persist"
replace_once "$persist" \
  '                occurrence: OccurrenceId::new(),' \
  "                occurrence: OccurrenceId::from_bytes([
                    ${FIXED_V7},
                    parent.map_or(0xff, |parent| parent as u8),
                ])
                .expect(\"a well-formed identifier\"),"
expect_kill derived_from_the_parent_path exchange \
  two_placements_of_one_definition_are_two_identities
restore_mutation

# 5. Derived from what the placement is called.
begin_mutation "$persist"
replace_once "$persist" \
  '                occurrence: OccurrenceId::new(),' \
  "                occurrence: OccurrenceId::from_bytes([
                    ${FIXED_V7},
                    instance.name.len() as u8,
                ])
                .expect(\"a well-formed identifier\"),"
expect_kill derived_from_the_display_name scene \
  identical_names_transforms_and_keys_do_not_collapse_two_placements
restore_mutation

# 6. Derived from where the placement puts its part.
begin_mutation "$persist"
replace_once "$persist" \
  '                occurrence: OccurrenceId::new(),' \
  "                occurrence: OccurrenceId::from_bytes([
                    ${FIXED_V7},
                    instance.placement[3].to_bits() as u8,
                ])
                .expect(\"a well-formed identifier\"),"
expect_kill derived_from_the_transform scene \
  identical_names_transforms_and_keys_do_not_collapse_two_placements
restore_mutation

# 7. Derived from the definition, so every placement of one part shares one
# identity — the exact failure §22B-1e1 measured in Unity.
begin_mutation "$persist"
replace_once "$persist" \
  '                occurrence: OccurrenceId::new(),' \
  "                occurrence: OccurrenceId::from_bytes([
                    ${FIXED_V7},
                    definition.len() as u8,
                ])
                .expect(\"a well-formed identifier\"),"
expect_kill derived_from_the_definition_key scene \
  two_placements_of_one_definition_are_two_identities_on_one_shared_mesh
restore_mutation

# ==================================================== what must be refused

# 8. Two placements answering to one identity, accepted by the persistence
# boundary. The export boundary still refuses it, which is why the gate is
# about *where* the refusal happens: a document must not reach a rebuild
# carrying it.
begin_mutation "$persist"
replace_once "$persist" \
  $'            if let Some(earlier) = seen.insert(instance.occurrence, index) {' \
  $'            if let Some(earlier) = None::<usize> {\n                let _ = seen.insert(instance.occurrence, index);'
expect_kill duplicate_identities_accepted_by_the_payload scene \
  placement_identities_that_were_swapped_or_stolen_are_refused_before_any_export
restore_mutation

# 9. And the same thing at the export boundary, which is the only place that
# sees the whole document at once: two imported objects can each be internally
# sound and still claim one identity between them.
begin_mutation "$model"
replace_once "$model" \
  $'        if occurrence.is_recorded()\n            && let Some(earlier) = self.nodes.iter().find(|node| node.occurrence == occurrence)\n        {' \
  $'        if false\n            && let Some(earlier) = self.nodes.iter().find(|node| node.occurrence == occurrence)\n        {'
expect_kill duplicate_identities_accepted_by_the_builder scene \
  one_identity_naming_two_placements_is_refused_at_the_export_boundary_too
restore_mutation

# 10. A current-layout payload with no identity, filled in while it is read.
# This is the mutant that shows why the field is required rather than optional:
# the moment a default exists, a document that never recorded an identity is
# indistinguishable from one that did.
begin_mutation "$persist"
replace_once "$persist" \
  '    pub occurrence: OccurrenceId,' \
  $'    #[serde(default = "OccurrenceId::new")]\n    pub occurrence: OccurrenceId,'
expect_kill missing_current_layout_identity_filled_in document \
  a_current_layout_payload_with_no_placement_identities_is_refused
restore_mutation

# 11. A version 2 payload read as though it were the current layout.
begin_mutation "$payload"
replace_once "$payload" \
  $'                2 => {\n                    let stored: StoredImport<KeyedScene> = envelope.decode()?;' \
  $'                99 => {\n                    let stored: StoredImport<KeyedScene> = envelope.decode()?;'
expect_kill a_version_2_payload_read_as_the_current_layout document \
  a_document_written_before_placements_had_identities_still_opens_and_binds
restore_mutation

# 12. A malformed identity accepted, because the version nibble stopped being
# checked. The refusal lives in the identifier type, so this is where it is
# removed from.
begin_mutation "$root/crates/ferritecad-types/src/ids.rs"
replace_once "$root/crates/ferritecad-types/src/ids.rs" \
  $'                if uuid.get_variant() != Variant::RFC4122\n                    || uuid.get_version() != Some(Version::SortRand)\n                {' \
  $'                if false {'
expect_kill a_malformed_identity_accepted document \
  a_placement_identity_that_is_not_a_uuidv7_is_refused_while_it_is_read
restore_mutation

# =============================================== who owns it, and who reads it

# 13. A fresh reading of the same bytes wins over the payload.
begin_mutation "$spine"
replace_once "$spine" \
  $'        let identity = match from.occurrences {\n            StoredOccurrences::Unrecorded => NodeIdentity::Unrecorded,' \
  $'        let identity = match &StoredOccurrences::Recorded(\n            scene\n                .instances\n                .iter()\n                .map(|_| ferritecad_types::OccurrenceId::new())\n                .collect(),\n        ) {\n            StoredOccurrences::Unrecorded => NodeIdentity::Unrecorded,'
expect_kill a_fresh_reading_replaces_the_persisted_identity scene \
  the_same_identities_come_back_from_every_cold_rebuild_and_every_session
restore_mutation

# 14. The identities shifted by one against the placements they belong to.
begin_mutation "$spine"
replace_once "$spine" \
  '                NodeIdentity::Occurrence(*recorded.get(index).ok_or_else(|| {' \
  '                NodeIdentity::Occurrence(*recorded.get(index + 1).ok_or_else(|| {'
expect_kill identities_shifted_against_their_placements scene \
  the_identity_of_each_node_is_the_one_stored_for_that_place
restore_mutation

# 15. Two neighbouring identities swapped, which is stable and distinct and
# still wrong.
begin_mutation "$spine"
replace_once "$spine" \
  '                NodeIdentity::Occurrence(*recorded.get(index).ok_or_else(|| {' \
  $'                NodeIdentity::Occurrence(*recorded\n                    .get(match index {\n                        0 => 1,\n                        1 => 0,\n                        other => other,\n                    })\n                    .ok_or_else(|| {'
expect_kill two_neighbouring_identities_swapped scene \
  the_identity_of_each_node_is_the_one_stored_for_that_place
restore_mutation

# 16. The identity dropped on the way through the load spine.
begin_mutation "$spine"
replace_once "$spine" \
  $'            name: Some(instance.name.clone()).filter(|name| !name.trim().is_empty()),\n            identity,' \
  $'            name: Some(instance.name.clone()).filter(|name| !name.trim().is_empty()),\n            identity: NodeIdentity::Unrecorded,'
expect_kill identity_dropped_in_the_load_spine scene \
  the_identity_of_each_node_is_the_one_stored_for_that_place
restore_mutation

# 17. The identity dropped where the neutral scene is assembled.
begin_mutation "$builder"
replace_once "$builder" \
  '                occurrence_of(node.identity),' \
  '                ExportOccurrence::Unrecorded,'
expect_kill identity_dropped_at_the_export_builder scene \
  the_identity_of_each_node_is_the_one_stored_for_that_place
restore_mutation

# 18. A native body given a fresh identity instead of the object that holds it,
# so its identity is new on every export.
begin_mutation "$spine"
replace_once "$spine" \
  '                        identity: NodeIdentity::Object(object.id),' \
  '                        identity: NodeIdentity::Occurrence(OccurrenceId::new()),'
expect_kill native_nodes_identified_by_a_fresh_uuid scene \
  a_native_body_is_identified_by_its_object_and_not_by_a_fresh_uuid
restore_mutation

# 19. A durable identity invented for a document that never recorded one.
begin_mutation "$persist"
replace_once "$persist" \
  $'            Self::V1(_) | Self::V2(_) => StoredOccurrences::Unrecorded,' \
  $'            Self::V1(_) => StoredOccurrences::Unrecorded,\n            Self::V2(scene) => StoredOccurrences::Recorded(\n                scene\n                    .instances\n                    .iter()\n                    .map(|_| OccurrenceId::new())\n                    .collect(),\n            ),'
expect_kill durable_identity_fabricated_for_a_legacy_document scene \
  a_document_written_before_placements_had_identities_says_so_and_still_exports
restore_mutation

# =========================================== what must not change around it

# 20. Structural frames left without an identity, which is the §22B-1c boundary
# eroded from the other side: a partial description that looks complete.
begin_mutation "$spine"
replace_once "$spine" \
  $'        sink.node(&PreparedNode {\n            definition,\n            parent: instance.parent.map(|parent| base + parent),' \
  $'        let identity = if structural[index] {\n            NodeIdentity::Unrecorded\n        } else {\n            identity\n        };\n        sink.node(&PreparedNode {\n            definition,\n            parent: instance.parent.map(|parent| base + parent),'
expect_kill structural_nodes_left_without_an_identity scene \
  the_identity_of_each_node_is_the_one_stored_for_that_place
restore_mutation

# 21. Nodes of an omitted definition dropped, so the identities no longer line
# up with the placements the document stored.
begin_mutation "$builder"
replace_once "$builder" \
  $'    fn node(&mut self, node: &PreparedNode) -> Result<()> {\n        self.nodes.push(node.clone());' \
  $'    fn node(&mut self, node: &PreparedNode) -> Result<()> {\n        if self.omissions.contains_key(&node.definition) {\n            return Ok(());\n        }\n        self.nodes.push(node.clone());'
expect_kill omitted_nodes_dropped scene \
  a_placement_of_a_definition_with_no_triangles_still_has_its_stored_identity
restore_mutation

# 22. A second reading of the stored source, which would be a second answer to
# a question the load answers once — and, now, a second set of identities read.
begin_mutation "$spine"
replace_once "$spine" \
  '                    imported.extend(reopened.scene.shapes());' \
  $'                    imported.extend(reopened.scene.shapes());\n                    let again = {\n                        let mut reader = Reader {\n                            kernel: &mut *kernel,\n                            read: &mut read_step,\n                        };\n                        document.reopen_step_import(object.id, &mut reader)?\n                    };\n                    imported.extend(again.scene.shapes());'
expect_kill the_stored_source_read_twice scene \
  one_export_solves_once_reads_each_source_once_and_meshes_each_definition_once
restore_mutation

# 23. The identity leaked into what a person reads. Display names are the one
# thing §22B-1e1 said must stay exactly what the user recognises, and the whole
# reason this slice stops short of the writer.
begin_mutation "$spine"
replace_once "$spine" \
  $'            name: Some(instance.name.clone()).filter(|name| !name.trim().is_empty()),\n            identity,' \
  $'            name: Some(match identity {\n                NodeIdentity::Occurrence(occurrence) => {\n                    format!("{} {occurrence}", instance.name)\n                }\n                _ => instance.name.clone(),\n            })\n            .filter(|name| !name.trim().is_empty()),\n            identity,'
expect_kill identity_leaked_into_the_display_name scene \
  a_document_written_before_placements_had_identities_says_so_and_still_exports
restore_mutation

# 24. The FBX writer reading the identity in this slice, which would change
# every file this build has ever produced.
begin_mutation "$writer"
replace_once "$writer" \
  '        let name = node.display_name.as_deref().unwrap_or_default();' \
  $'        let owned = match node.occurrence {\n            crate::ExportOccurrence::Occurrence(occurrence) => occurrence.to_string(),\n            _ => node.display_name.as_deref().unwrap_or_default().to_owned(),\n        };\n        let name = &owned[..];'
expect_kill the_writer_consumes_the_identity writer \
  a_placement_identity_changes_nothing_the_writer_writes
restore_mutation

# 25. And the bytes changed at all, in a slice whose whole claim is that the
# file a person gets is the file they already had.
begin_mutation "$writer"
replace_once "$writer" \
  'const CREATOR: &str = "FerriteCAD FBX 7.4 ASCII writer";' \
  'const CREATOR: &str = "FerriteCAD FBX 7.4 ASCII writer.";'
expect_kill existing_fbx_bytes_changed bytes -
restore_mutation

# ========================================================= metamorphic control
#
# Which of two equal spellings of a placement identity a document stores is not
# a fact about the placement: the identifier is opaque, and nothing may depend
# on the bits inside it. Reading the stored list into the load through an
# explicit clone rather than by reference must therefore change nothing, and a
# gate this killed would be a gate that depends on how the list is carried.
begin_mutation "$spine"
replace_once "$spine" \
  '            StoredOccurrences::Recorded(recorded) => {' \
  '            StoredOccurrences::Recorded(recorded) if !recorded.is_empty() => {'
expect_survivor a_non_empty_recorded_list_matched_explicitly scene \
  the_identity_of_each_node_is_the_one_stored_for_that_place
restore_mutation

baseline
no_stale_backups "$root"

echo "mutation campaign: $killed runtime mutants killed"
echo "mutation campaign: $survived unexpected survivors"
echo "mutation campaign: compile refusal, zero-test and zero-check controls were not credited"
