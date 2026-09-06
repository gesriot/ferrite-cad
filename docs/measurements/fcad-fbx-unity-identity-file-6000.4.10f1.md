<!-- SPDX-License-Identifier: MIT -->
# §22B-1e3b: the identity channel in the shipped file, read by Unity 6000.4.10f1

What the real editor makes of the two invisible properties the production FBX
writer now puts on every node. Unlike §22B-1e2a and §22B-1e2b, nothing here is
a candidate: the bytes are the writer's own, checked against
`tools/fbx/digests.tsv` before the editor sees them, and there is no rewriter,
no transformer and no second serializer anywhere in this measurement.

**Editor:** Unity 6000.4.10f1, checked rather than assumed.
**Files:** `fcad-measured.fbx`, `fcad-legacy.fbx`, `fcad-identity-escaping.fbx`
and `fcad-renamed.fbx`, produced by `fbx_gate_artefacts` through
`write_fbx_ascii_7400`.
**Tool:** [`tools/unity-identity-file`](../../tools/unity-identity-file).
**Runs:** two freshly created temporary projects outside the repository, each
deleted afterwards; the two canonical reports must be byte-identical.

## The pair, and why it is the measurement

`fcad-measured.fbx` and `fcad-legacy.fbx` are one scene written by one writer,
differing only in whether the document recorded identities. The second has the
digest `0d1a5ba1e7bded87f1c9aa76f2e7d66575f5e986ac45f6a089f1c58adcd9a721` —
byte for byte the file this repository committed as `fcad-measured.fbx` before
§22B-1e3b. Comparing the two imports is therefore not "the new file looks
right"; it is "the new file and the old file differ in the channel and in
nothing the editor can see".

`fcad-identity-escaping.fbx` carries the definition keys chosen to break the
wire grammar. `fcad-renamed.fbx` is a **neutral test variant**: the same
identities under different designations, built by naming different strings. It
is not a STEP reimport — this build has no reimport semantics — and nothing
here presents it as one.

## What was asserted, and what came back

Nine imported objects in `fcad-measured.fbx`, four of which place one shared
definition. Every question below was asked of the real editor by the probe and
again from outside by
[`verify_file.py`](../../tools/unity-identity-file/scripts/verify_file.py), and
every one of them came back true. The probe performed 78 checks and both clean
projects produced byte-identical canonical reports.

| question | answer |
| --- | --- |
| a vanilla `ModelImporter` reads both properties on every object | yes, 9 of 9 |
| every value stayed in the domain it was written in | yes |
| the join is unambiguous — one placement per value, and the two domains disjoint | yes, 9 distinct placement identities |
| placements of one definition share one `Mesh` | yes, 4 placements on 1 mesh |
| the designations a person reads did not move | yes, 0 of 8 comparable objects |
| the hierarchy did not move | yes, 0 parents moved |
| the geometry and material bindings did not move | yes |
| the properties §22B-1b2 already wrote did not move | yes, `FerriteCADNodeKey` and `FerriteCADDefinitionKey` identical on every object |
| the import says nothing it did not say before | yes, 0 errors and the same 0 `Identifier uniqueness violation` in both |
| a layout that recorded nothing carries no property | yes, 0 of 9 objects in `fcad-legacy.fbx` |
| an escaped key survives the editor unchanged | yes, all five values came back in five parts |
| a placement is found again by its identity after every designation changed | yes, 9 of 9 joined |

The last row is the one the channel exists for. In `fcad-renamed.fbx` every
designation is different — eight of the nine, plus the root, which this editor
names after the asset file — and all nine placements were still found again by
their placement identity.

## What was measured and deliberately not judged

These are numbers, not verdicts. Each of them is a property of the editor that
§22B-1e2a and §22B-1e2b already measured, and adding a property to a file does
not change any of them.

| measured | value |
| --- | --- |
| `GameObject` local file identifiers that moved, current against legacy | 0 of 9 |
| `Mesh` local file identifiers that moved, current against legacy | 0 of 1 |
| `Material` local file identifiers that moved, current against legacy | 0 of 4 |
| **`GameObject` local file identifiers that moved across the rename** | **8 of 9** |
| meshes before and after the rename | 1 and 1 |
| materials before and after the rename | 4 and 4 |
| `Identifier uniqueness violation`, current and legacy | 0 and 0 |
| the editor sorts an imported hierarchy by name by default | yes |
| the designation this editor gives the model root | `fcad-measured` / `fcad-legacy`, that is, the asset's file name |

The bold row is the price, and it is exactly the price §22B-1e2a measured. The
channel makes a placement **findable** again after a rename — nine of nine
joined by identity — and it does **not** make the editor's own serialised
references survive that rename: eight of nine local file identifiers moved,
because in this editor an identifier is derived from the visible name. A
project that had a reference to one of those objects still has to re-resolve
it; what the channel gives it is a way to know *which* object to re-resolve it
to.

Adding the two properties moved nothing at all when the designations stayed the
same: zero identifiers moved between `fcad-measured.fbx` and `fcad-legacy.fbx`,
which are the same scene at two document layouts.

## The limits, stated rather than implied

* **A local file identifier is not stabilised, and this record does not claim
  it is.** In this editor an identifier is a function of the visible name plus
  the type plus a collision counter, or of the hierarchy path; §22B-1e2a and
  §22B-1e2b measured that on five graphs and four importer mechanisms. No
  property in an FBX changes it, and the eight-of-nine result above is that,
  measured rather than argued. Four of the semantic mutants in the campaign are
  *metamorphic controls* that make every identifier move and must **survive**,
  precisely so this record cannot quietly acquire a stability claim later.
* **A `Mesh` and a `Material` carry no properties of their own.** This editor
  hands custom properties to a callback about a `GameObject` and to nothing
  else, so there is no identity on a sub-asset for anything to join two imports
  by, whatever the file says. Material identity remains unchosen, as §22B-1e1
  left it.
* **The root's designation is the asset's file name.** Two files of two names
  have two root designations however identical their contents, so the root is
  excluded from the designation comparison and reported instead. Its identity
  properties are read like any other object's, and a mutant that widened that
  exclusion is killed.
* **The hierarchy sort is turned off by the probe.** This editor sorts an
  imported hierarchy by name by default, which reorders it relative to the file;
  the callback that hands over the custom properties sees the tree *before* the
  sort and the finished asset is the tree *after* it, so a measurement that left
  the default on would be lining up two different orderings of one import. The
  default is recorded rather than hidden.
* **`Identifier uniqueness violation` is not fixed here.** It is §22B-1e1's
  finding. What is measured is that the channel adds none: zero in both files of
  the pair.
* **One editor version, one machine, one small scene.** Nine objects on Unity
  6000.4.10f1 in two clean temporary projects on one host. Nothing here says
  anything about another editor version or another importing program. The 140-node
  assembly is gated outside Unity, by
  [`tools/check-fbx-identity.sh`](../../tools/check-fbx-identity.sh).
* **No companion package, `ScriptedImporter`, `.meta` edit or `AddRemap` takes
  part.** The probe reads and reports; it renames nothing and publishes nothing.
* **`fcad-renamed.fbx` is a neutral test variant**, built by naming different
  strings. It is not a STEP reimport: this build has no reimport semantics, and
  nothing here presents it as one.

## The campaigns

Two, because the measurement has two halves that fail in different ways.

[`scripts/run_file_mutations.py`](../../tools/unity-identity-file/scripts/run_file_mutations.py)
perturbs the recorded canonical measurement and feeds it to the real decision
verifier: nineteen mutants, each a way this measurement could look like it had
proved something it had not, all killed. Beside them four **metamorphic
controls**, each of which must *survive*: every `GameObject` identifier moving,
every `Mesh` and `Material` identifier moving, the existing
`Identifier uniqueness violation` still being there, and every identifier
moving across the rename. A verifier that refused one of those would be a
verifier asserting a stability nothing in an FBX can deliver, which is exactly
the failure this record exists to avoid.

[`scripts/mutate_file.sh`](../../tools/unity-identity-file/scripts/mutate_file.sh)
mutates the probe and the runner and runs each mutant against the real editor
with the byte-for-byte comparison switched off, so a mutant dies from a check
that understands the defect rather than from "these bytes are not the recorded
bytes": the file the channel is asserted on replaced by the one that has no
channel; the control of the pair replaced by a file whose designations really
did move; the property callback that records nothing; the rename variant
replaced by a rename that never happened; the join across the rename made by
name instead of by identity; and the editor shown bytes the production writer
did not produce. All six killed; a non-compiling probe is refused and is not
credited as a kill.

Every one of those is a defect the measurement must *notice*, rather than a
question deleted from it. Deleting an assertion about a file that satisfies it
changes nothing and survives for the right reason, which is not a mutation
campaign — the first draft of this campaign contained exactly such a mutant, it
survived, and it was replaced rather than explained away.

[`scripts/check_file_record.sh`](../../tools/unity-identity-file/scripts/check_file_record.sh)
is the half CI runs on every push: it rebuilds the decision record from the
recorded report, compares it with the committed one, runs the semantic
campaign, and refuses if a measurement left anything Unity produced inside the
repository.

## Raw measurement

The canonical report and the decision record are committed under
[`expected/`](../../tools/unity-identity-file/expected). The report lists every
imported object of every file with its path, its designation, both identity
properties, the two properties §22B-1b2 already wrote, its mesh binding and its
local file identifier, plus every sub-asset with its own identifier.
