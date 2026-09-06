<!-- SPDX-License-Identifier: MIT -->
# §22B-1e3b: what Unity makes of the identity channel in the shipped file

The measurement behind
[`docs/measurements/fcad-fbx-unity-identity-file-6000.4.10f1.md`](../../docs/measurements/fcad-fbx-unity-identity-file-6000.4.10f1.md).

## What is measured

The real production bytes. `fbx_gate_artefacts` writes them through
`write_fbx_ascii_7400`, the runner checks each one against
`tools/fbx/digests.tsv` before the editor sees it, and nothing rewrites them on
the way in. There is no transformer here and no second serializer: earlier
slices measured candidates, and this one measures what a person gets.

Three files, and the first two are a pair:

| file | what it is |
| --- | --- |
| `fcad-measured.fbx` | the scene as a current-layout document exports it |
| `fcad-legacy.fbx` | the same scene as a document that recorded no identities exports it — byte for byte the file this repository committed before §22B-1e3b |
| `fcad-identity-escaping.fbx` | definition keys chosen to break the wire grammar: the value separator, the escape character, an already-escaped-looking key, whitespace and a multi-byte code point |

Comparing the first two imports is what makes "nothing else moved" a
measurement rather than a claim.

## What is asked, and what is deliberately not asked

Asked, and asserted: a vanilla `ModelImporter` reads both properties on every
object; every value stays in the domain it was written in; the join is
unambiguous; the placements of one definition share one `Mesh`; the
designations, the hierarchy, the geometry and material bindings and the
properties §22B-1b2 already wrote are exactly what they were; the import says
nothing it did not say before; a layout that recorded no identity carries no
property at all; an escaped key comes back out of the editor unchanged.

Reported, and **not** asserted: how many `GameObject`, `Mesh` and `Material`
local file identifiers moved between the two imports. §22B-1e2a and §22B-1e2b
measured what an identifier is in this editor — the visible name plus the type
plus a collision counter, or the hierarchy path — and no property in an FBX
changes that. A record that demanded stability here would have to be either
wrong or quietly weakened later, so it says what happened instead.

Not touched at all: `.meta` editing, `AssetImporter.AddRemap`, a
`ScriptedImporter`, a companion package, and the `Identifier uniqueness
violation` §22B-1e1 recorded. The probe reads and reports; it renames nothing
and publishes nothing.

## Running it

Unity 6000.4.10f1 must be installed, and the version is checked rather than
assumed.

```sh
tools/unity-identity-file/scripts/run_file_measurement.sh            # the measurement
tools/unity-identity-file/scripts/run_file_measurement.sh --record   # re-record it
tools/unity-identity-file/scripts/mutate_file.sh                     # the probe campaign
tools/unity-identity-file/scripts/check_file_record.sh               # the half CI runs
```

Each run happens in a freshly created temporary project outside the
repository, twice, and the two canonical reports must be byte-identical.
Nothing imported is left behind, and `check_repository_clean.sh` is what would
notice if it were.

## Layout

| path | what it is |
| --- | --- |
| `Editor/FerriteFileIdentity.cs` | the probe: imports, reads, compares, refuses |
| `Editor/FerriteFileProperties.cs` | the one callback Unity hands custom properties to |
| `scripts/run_file_measurement.sh` | the runner |
| `scripts/verify_file.py` | rebuilds the decision record from the report |
| `scripts/run_file_mutations.py` | semantic mutants against the real verifier |
| `scripts/mutate_file.sh` | mutants in the probe, run against the real editor |
| `scripts/check_file_record.sh` | the editor-free half, run on every push |
| `expected/` | the recorded measurement and the decision record |
