# Unity asset identity across a reimport — the §22B-1e1 measurement

This measures one thing: whether a reference a Unity project already holds into
a FerriteCAD FBX still means the same FerriteCAD object after the document is
exported again.

It is measurement only. Nothing here changes the FBX writer, `ExportScene`, the
document schema, the shipped commands or the window, and the answer it produces
is a contract for a later slice rather than a fix.

## What it is not

* Not a smoke test of import. §22B-1a to §22B-1d already measured that the
  content arrives — hierarchy, transforms, shared meshes, materials, custom
  properties. This asks a different question and gets a different answer.
* Not a check that a reference is non-null. A reference that resolves to some
  object is not a reference that survived; it may now point at a different
  part. That case is measured and named.
* Not a conclusion drawn from one reimport, one editor run or one project.

## What it measures

For every tracked reference, on both sides of one document change:

| recorded | where it comes from |
| --- | --- |
| Unity type, imported name | the editor |
| asset GUID, local file identifier | `AssetDatabase.TryGetGUIDAndLocalFileIdentifier` |
| `GlobalObjectId` | `GlobalObjectId.GetGlobalObjectIdSlow` |
| `FerriteCADDefinitionKey`, `FerriteCADNodeKey` | the editor's own custom-property callback |
| FBX object number, hierarchy, geometry sharing | pinned `ufbx` 0.23.0, independently |
| what a project file actually stored | the saved `.asset`, read back as text |

Geometry, Model and Material are measured apart, and the measurement is the
reason: they turn out to share one identity rule — Unity name plus type plus a
collision counter — but to take that name from three different places, so a
result about one of them is not a result about the others.

## How a reference is judged

A real `ScriptableObject` asset is created, given real references to the tracked
Mesh, Material and GameObject sub-assets, and saved. Unity writes each one to
disk as `{fileID, guid, type}` — the same pair a prefab, a scene or a material
would write. The document is then exported again over the same asset path, the
editor reimports it, and the reference is resolved twice by two independent
routes: through the reloaded asset, and by putting the stored identifier back
through `GlobalObjectId`. The two must agree or the run is refused.

The verdict is then decided on *meaning*, established from the durable key and
cross-checked against a vertex count `ufbx` read from the same bytes:

* `same_semantic` — still the same FerriteCAD definition, and for a placement
  still the same occurrence of it;
* `same_definition_other_occurrence` — the definition survived, the occurrence
  did not;
* `retargeted_to_another_definition` — resolved, silently, to a different part;
* `missing_though_object_still_exported` — null, although the object it meant
  is still in the file;
* `missing_because_object_was_removed` — null because the document dropped it.

`FerriteCADNodeKey` is reported beside the verdict and never inside it: the
writer derives that key from a position in the scene, so folding it into
meaning would make every unrelated insertion look like a broken reference.

## Reproducing it

Every FBX comes from the production writer. The portable variants come from the
`fbx_identity_variants` example over `write_fbx_ascii_7400`; the real assembly
comes from the shipped `export-fbx` route over the shared job. No Python
generator and no committed FBX fixture is involved.

```sh
scripts/run_identity_measurement.sh                 # the portable measurement
scripts/run_identity_measurement.sh --with-complex  # the real AP203 assembly
scripts/mutate_identity.sh                          # controls and mutants
scripts/check_identity_record.sh                    # the half that needs no editor
```

`check_identity_record.sh` is what CI runs: it re-joins both recorded
measurements to the independent `ufbx` reading of the same bytes, rebuilds both
transition tables and compares them with the committed ones, runs the semantic
mutation campaign against the real verifier, and refuses if anything Unity
produced was left in the repository. It needs Python 3 and nothing else. The
editor stays local, on the one measured version, as §22B-1a decided.

The editor runs `-batchmode -nographics` in a freshly created temporary project
outside the repository, once per run, and the project is deleted afterwards.
Two runs in two clean projects must produce byte-identical canonical reports.
No `.fbx`, `.meta`, `Library` or Unity project is left in the repository.

`--with-complex` needs Open CASCADE and produces a third of a gigabyte of ASCII
FBX twice, so it records its own measurement under `expected/complex-*.json`
rather than making the portable one conditional on a kernel.

## One importer setting, and why

The probe sets `ModelImporter.sortHierarchyByName = false`.

Unity applies that sort *after* the custom-property callbacks have run, so the
durable keys the callback reports would be attached to the wrong finished
objects — the first version of this probe did exactly that and put
`step.product_definition#50` on a node whose geometry has four vertices instead
of three. Turning the sort off makes the callback order and the finished order
the same.

Because that is the probe changing the thing it measures, it is measured: the
`sort_control` block imports the same file both ways and compares every
sub-asset's type, name and local file identifier. Turning the sort off reorders
the hierarchy and moves no identifier. If that ever stops being true, the
verifier refuses the whole run rather than reporting a result about the probe.

## The recorded measurement

* `expected/identity-report.json` — what the editor did, canonical, no
  timestamps, no absolute paths, GUIDs tokenised in first-seen order.
* `expected/identity-oracle-report.json` — what `ufbx` read from the same bytes.
* `expected/identity-plan.json` — the scenarios, by file name.
* `expected/identity-transitions.json` — the join: verdict, name, local file
  identifier, FBX object number and node key on both sides of each change.
* `expected/complex-*.json` — the same four for the real AP203 assembly.

Local file identifiers are left exactly as Unity produced them, because they
are the measurement. GUIDs are tokenised, because a GUID is new in every
project and this slice is not about them — and the two clean-project runs prove
that everything else is not.

The reading of these numbers, the honest limits and the contract handed to
§22B-1e2 are in
[`../../docs/measurements/fcad-fbx-unity-identity-6000.4.10f1.md`](../../docs/measurements/fcad-fbx-unity-identity-6000.4.10f1.md).
