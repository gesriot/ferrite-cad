# The remaining Unity identity mechanisms, and an alternative FBX graph — §22B-1e2b

§22B-1e2a measured names, custom properties and a companion postprocessor on
the **flat** production FBX graph, and said in its own conclusion that it had
not measured any other graph, the `.meta` identity table, `AddRemap`, or a
`ScriptedImporter`. This slice measures those four, on the same documents, with
the same rules, so the three results can be read together.

It is measurement only. Nothing here changes the FBX writer, `ExportScene`, the
document schema, the shipped commands or the window, and it chooses no policy.
What it produces is a decision table for a later slice.

## The contract it is measuring against

A mechanism passes only if it does all of this at once:

* real serialized `{fileID, guid, type}` references survive;
* the `GameObject`, `Mesh` and `Material` names a person reads stay the
  designations a person recognises;
* every placement of one definition keeps **one** shared `Mesh`, by object
  identity and not by an equal vertex count;
* identity does not depend on traversal order, sibling ordinal, FBX object
  number or Unity's collision counter;
* two `ImportedSourceId`s carrying the same source-local key stay two things;
* a removed object becomes a missing reference and is never re-pointed at
  something that looks like it;
* two clean Unity projects agree.

The failing-first run asserts that whole list against the production graph and
is meant to fail. Its refusal is kept in `evidence/failing-first.log` rather
than described.

## The four questions, and why each runs in its own project

| mode | question |
| --- | --- |
| `graph` | does a different FBX graph move a shared `Mesh`'s identity onto the definition? |
| `meta` | what is `internalIDToNameTable` in 6000.4.10f1, and does any public API write it? |
| `remap` | what do `SourceAssetIdentifier` and `AddRemap` really do, for `Mesh`, `Material` and `GameObject` separately? |
| `scripted` | can `AssetImportContext.AddObjectToAsset` choose the local file identifiers from a durable identity? |
| `fbxclaim` | can a `ScriptedImporter` own the `fbx` extension, or does it need its own? |

Three of them change what the editor *is*: `meta` edits serialized importer
metadata, `remap` puts external assets in the project, `scripted` registers an
importer, and `fbxclaim` registers one that claims `fbx`. Sharing a project
would make each result a property of the others, so each mode runs in freshly
created temporary projects outside the repository, twice, and the two canonical
reports must be byte-identical.

## The graphs

| variant | topology | written by |
| --- | --- | --- |
| `g-flat` | the production graph: one `Model` per placement, the `Geometry` connected to every placement | the writer, copied |
| `g-flat-id` | the same graph, with only the invisible identity properties added | the transformer |
| `g-carrier` | a machine-named definition carrier `Model` parented to the scene root, connected to the `Geometry` **before** any placement | the transformer |
| `g-carrier-detached` | the same carrier with **no** parent connection at all | the transformer |
| `g-two-level` | each placement becomes a `Null` keeping its designation and transform, with a machine-named geometry-bearing child | the transformer |

`g-flat-id` exists so the topology question is asked on its own: every variant
below it differs from it in graph shape and in nothing else, so a difference
cannot be attributed to the identity channel §22B-1e2a already measured.
`g-carrier-detached` is the fourth variant the brief asks for, and it was
chosen after `g-carrier` turned out to cost a visible node: it asks whether the
carrier has to be visible at all.

## Where the bytes come from

Every file is the production writer's output or that output transformed:

* `crates/ferritecad-export/examples/fbx_channel_documents.rs` builds twelve
  documents through `ExportSceneBuilder` and writes them with
  `write_fbx_ascii_7400` — the shipped writer, called the way the shipped route
  calls it. It is §22B-1e2a's generator, reused unchanged.
* `scripts/rewrite_graph.py` is handed those bytes and changes exactly three
  named sections: the two `Count:` numbers in `Definitions`, new `Model` blocks
  and two edits inside existing ones in `Objects`, and new or re-pointed
  `C: "OO"` lines in `Connections`. It never touches a `Geometry` block, a
  `Material` block, a vertex, a colour, a transform, the header or
  `GlobalSettings`, and it never renumbers an existing object.
* `g-flat` is a plain copy, and the runner refuses if it is not byte-identical
  to the writer's output.

There is no second serializer, and the transformer's claim is checked rather
than believed: pinned `ufbx` 0.23.0 reads every variant and every control, and
`verify_graph.py` refuses if any geometry array digest, material colour, node
world transform or existing object number differs from the control's.

## What is measured, per graph

Twelve document changes — byte-identical, re-export, a designation change,
inserting, removing and reordering a definition, inserting, removing and
reordering a sibling placement, removing the tracked definition with its only
placement, changing a material and reusing a material designation — with the
same eighteen tracked references each time: six meshes, five materials and
seven placements. Beside them, per variant: the `GameObject`, `MeshFilter`,
`MeshRenderer`, `Mesh` and `Material` counts, whether Unity had to invent a
root, `sharedMesh` reference equality per definition, the triangle count and
material-slot count under every placement, every placement's local and world
transform, and every visible name.

## How a reference is judged

A real `ScriptableObject` is created, given real references to the tracked
`Mesh`, `Material` and `GameObject` sub-assets, and saved; Unity writes each as
`{fileID, guid, type}`. The document is exported again over the same asset
path, and the reference is resolved twice — through the reloaded asset and by
putting the stored identifier back through `GlobalObjectId`. The two must
agree.

The verdict is decided on meaning, cross-checked against a witness the identity
scheme did not supply: a vertex count for a mesh, a colour for a material, the
placement's own **world** position for a `GameObject` — world rather than local,
because a graph that inserts a node between a placement and the root would
otherwise get a free pass.

* `same_semantic`;
* `same_definition_other_occurrence`;
* `retargeted_to_another_definition` — resolved, silently, to a different part;
* `missing_though_object_still_exported`;
* `missing_because_object_was_removed`;
* `ambiguous_join` — the identity could not name this object at all. Not a kept
  reference and not a broken one, and never reported as either.

## What it is not

* Not a check that a reference is non-null.
* Not a claim about the FBX when the answer needs a plugin or a project-side
  setting. The `remap` report says on its own first line that `AddRemap` is not
  a property of the exported file; the `scripted` report says the same about
  the importer, and says that importer reads no FBX.
* Not a proposal. No mechanism is recommended here.

## Reproducing it

```sh
scripts/run_graph_measurement.sh --expect-full-contract   # the failing first run
scripts/run_graph_measurement.sh                          # the measurement
scripts/mutate_graph.sh                                   # controls and mutants
scripts/check_graph_record.sh                             # the half that needs no editor
```

`check_graph_record.sh` is what CI runs: it re-joins all five recorded runs to
the independent `ufbx` reading of the same bytes, holds the transformer to its
claim against the control, rebuilds the decision record and compares it with
the committed one, runs the semantic mutation campaign against the real
verifier, and refuses if anything Unity produced was left in the repository. It
needs Python 3 and nothing else.

`--no-expected` skips the comparisons with the committed measurement, so the
mutation campaign can prove that a mutant is caught by a check that understands
it rather than by "these bytes are not the recorded bytes". `--mode` and
`--variants` narrow a run, and a narrowed `graph` run is still held to the
structural half of the join.

## The recorded measurement

* `expected/graph-report.json` — the five graphs a stock editor reads.
* `expected/meta-report.json` — what 6000.4.10f1 writes into a `.fbx.meta`,
  what public API names it, and what a direct edit does.
* `expected/remap-report.json` — `AddRemap` for `Mesh`, `Material` and
  `GameObject`.
* `expected/scripted-report.json` — `AddObjectToAsset` for the same three.
* `expected/fbxclaim-report.json` — what happened when a `ScriptedImporter`
  claimed `fbx`.
* `expected/graph-oracle-report.json` — what pinned `ufbx` 0.23.0 read from the
  same sixty files.
* `expected/*-plan.json` — the scenarios.
* `expected/graph-decision.json` — the join: every transition, the variant
  summaries, the three mechanism summaries, the decision table and the stop
  conditions.

Local file identifiers are left exactly as Unity produced them, because they
are the measurement. GUIDs are tokenised, because a GUID is new in every
project — and a mutant that leaves them untokenised is killed only by the
second clean project, which is how this harness proves that second project is
load-bearing.

The reading of these numbers, the honest limits and the decision table are in
[`../../docs/measurements/fcad-fbx-unity-identity-graph-6000.4.10f1.md`](../../docs/measurements/fcad-fbx-unity-identity-graph-6000.4.10f1.md).
