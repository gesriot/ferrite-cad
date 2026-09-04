# A Unity-safe identity channel, separated from the visible names — §22B-1e2a

This measures one thing: whether there is a channel that keeps a Unity
reference across a real re-export **and** leaves the names a person reads as
the names a person recognises.

It is measurement only. Nothing here changes the FBX writer, `ExportScene`, the
document schema, the shipped commands or the window, and it chooses no policy.
What it produces is a decision table for a later slice.

## What §22B-1e1 left open, and why one document is not enough

§22B-1e1 measured documents with **one** imported source, so the
`FerriteCADDefinitionKey` the writer exports — which contains only the
source-local key — was unambiguous inside every file it measured. Two STEP
sources may legally both contain `step.product_definition#42`, and then that
property names two different parts with one string.

So the base document here contains exactly that: two `ImportedSourceId`s, both
carrying `step.product_definition#42`, with different geometry and different
colours. Every candidate is measured on it. The control cannot tell them apart,
and that is the first result rather than an assumption.

## What it is not

* Not a check that a reference is non-null. A reference that resolves to some
  object is not a reference that survived.
* Not a claim about the FBX when the answer needs a plugin. Candidate D is the
  same *bytes* as candidate C imported with a FerriteCAD companion
  postprocessor active, in a separate editor run, and the report says so on its
  own line.
* Not a proposal. No candidate is recommended here.

## The candidates

| candidate | FBX object names | designations live | plugin |
| --- | --- | --- | --- |
| `a-control` | the production writer's, unchanged | in the names | no |
| `b-ordinal` | `fcad~<source>~<key>~occ~<ordinal>` | nowhere | no |
| `b-occurrence` | `fcad~<source>~<key>~occ~<occurrence uuid>` | nowhere | no |
| `c-property` | the same as `b-occurrence` | in custom properties | no |
| `d-companion` | **the same bytes as `c-property`** | in custom properties | **yes** |

`b-ordinal` and `b-occurrence` differ in one field and nothing else, which is
what makes the placement experiment about the durable occurrence identity
rather than about two unrelated schemes. FerriteCAD persists no occurrence
identity today, so the UUIDs are synthetic and exist only inside this
measurement: no schema, no capability and no writer change.

## Where the bytes come from

Every file is the production writer's output or that output rewritten:

* `crates/ferritecad-export/examples/fbx_channel_documents.rs` builds twelve
  documents through `ExportSceneBuilder` and writes them with
  `write_fbx_ascii_7400` — the shipped writer, called the way the shipped route
  calls it. It also writes a manifest of what the writer does *not* put in the
  file: the `ImportedSourceId` behind each definition, and the synthetic
  occurrence identities.
* `scripts/rewrite_channel.py` is handed those bytes and changes exactly two
  things: the name in an object's own header line, and the custom properties
  inside a `Model`'s `Properties70`. Object numbers, connections, geometry,
  transforms, colours and the whole `Definitions` section are copied through
  byte for byte. `a-control` is a plain copy, and the runner refuses if it is
  not byte-identical to the writer's output.

There is no second serializer, and the decision record names, per candidate,
which of the two produced its files.

## What is measured

Twelve document changes — byte-identical, re-export, a designation change,
inserting, removing and reordering a definition, inserting, removing and
reordering a sibling placement, removing a definition outright, changing a
material and reusing a material designation — with the same eighteen tracked
references each time: six meshes, five materials and seven placements. The base
document carries, all at once, two sources sharing one local key, two
definitions sharing a designation, four siblings sharing a designation, a
geometry with two placements, two material slots sharing a designation, two
structural nodes and one omitted definition. Those counts are asserted from the
file by the independent reader, so a scenario cannot pass because the confusion
it is about was never there.

Six naming questions are measured separately: the source-qualified token as it
stands, the same token 160 characters longer, a Cyrillic designation of the
shape the real AP203 assembly actually contains, a short FNV-1a token, a
deliberate collision between two durable identities, and one designation
written precomposed while another is written decomposed.

## How a reference is judged

A real `ScriptableObject` is created, given real references to the tracked
Mesh, Material and GameObject sub-assets, and saved; Unity writes each as
`{fileID, guid, type}`. The document is exported again over the same asset
path, and the reference is resolved twice — through the reloaded asset and by
putting the stored identifier back through `GlobalObjectId`. The two must
agree.

The verdict is decided on meaning, cross-checked against a witness the identity
scheme did not supply: a vertex count for a mesh, a colour for a material, the
placement's own translation for a `GameObject`.

* `same_semantic` — still the same FerriteCAD definition, and for a placement
  still the same occurrence of it;
* `same_definition_other_occurrence`;
* `retargeted_to_another_definition` — resolved, silently, to a different part;
* `missing_though_object_still_exported`;
* `missing_because_object_was_removed`;
* `ambiguous_join` — the candidate's identity could not name this object at
  all, because it names two different definitions. Not a kept reference and not
  a broken one, and never reported as either.

## Reproducing it

```sh
scripts/run_channel_measurement.sh --expect-durable-join   # the failing first run
scripts/run_channel_measurement.sh                          # the measurement
scripts/mutate_channel.sh                                   # controls and mutants
scripts/check_channel_record.sh                             # the half that needs no editor
```

`check_channel_record.sh` is what CI runs: it re-joins both recorded runs to
the independent `ufbx` reading of the same bytes, rebuilds the decision record
and compares it with the committed one, runs the semantic mutation campaign
against the real verifier, and refuses if anything Unity produced was left in
the repository. It needs Python 3 and nothing else.

The editor runs `-batchmode -nographics` in freshly created temporary projects
outside the repository, twice per mode, and the two canonical reports must be
byte-identical. `--no-expected` skips the two comparisons with the committed
measurement, so the mutation campaign can prove that a mutant is caught by a
check that understands it rather than by "these bytes are not the recorded
bytes".

## The recorded measurement

* `expected/vanilla-report.json` — the four candidates a stock editor reads.
* `expected/companion-report.json` — the same editor with the FerriteCAD
  companion postprocessor renaming objects from the designations in the file,
  plus `a-control` again as the control that the plugin leaves alone.
* `expected/channel-oracle-report.json` — what pinned `ufbx` 0.23.0 read from
  the same fifty-four files.
* `expected/vanilla-plan.json`, `expected/companion-plan.json` — the scenarios.
* `expected/channel-decision.json` — the join: every transition, the candidate
  summaries, the naming table and the rename-timing result the decision table
  is read from.

Local file identifiers are left exactly as Unity produced them, because they
are the measurement. GUIDs are tokenised, because a GUID is new in every
project — and a mutant that leaves them untokenised is killed only by the
second clean project, which is how this harness proves that second project is
load-bearing.

The reading of these numbers, the honest limits and the decision table are in
[`../../docs/measurements/fcad-fbx-unity-identity-channel-6000.4.10f1.md`](../../docs/measurements/fcad-fbx-unity-identity-channel-6000.4.10f1.md).
