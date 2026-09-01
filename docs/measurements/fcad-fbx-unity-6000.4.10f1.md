# §22B-1a: measured FCAD → FBX → Unity contract

Status: measurement and architecture decision only. There is no production
`export-fbx` command, FBX serializer, or production writer dependency in this
slice.

## Failing-first baseline

The baseline commit was
`2cea3c19637956a12b58007f7c48d79020189f73`, equal to `origin/main` when the
measurement began.

- `target/debug/ferritecad export-fbx` exited 2. clap reported
  `unrecognized subcommand 'export-fbx'` and suggested `export-stl`. The
  command remains absent after this slice.
- `RenderSnapshot` has only packed drawable meshes and draw items. A draw item
  carries the already accumulated transform and transient picking identity;
  it has no parent, stable source key, source-local transform, assembly name,
  or typed `GeometryOmission`.
- The complex document has 46 definitions and 140 scene nodes (one root and
  139 non-root occurrences). Its GPU snapshot has only 35 meshes and 112
  draws. Therefore neither the assembly tree nor its omissions can be
  reconstructed from the snapshot.
- The stored imported scene retains source unit and schema, definition keys,
  definition names and solids, occurrence parent indices, occurrence names,
  row-major local 3×4 placements, and colour source/linear colour. Existing
  end-to-end tests reopen the `.fcad` after deleting the external STEP and
  recover 46 definitions and all 140 nodes.
- `step.product_definition#2583` intentionally has no mesh and has a typed
  `GeometryOmission`. `step.product_definition#2428` remains real mesh
  geometry. No geometry is healed, sewn, or invented here.

## Reproducible instruments

The project at [`../../tools/unity-fbx-smoke`](../../tools/unity-fbx-smoke)
uses the installed native arm64 editor at the explicit path documented in its
README. The process reported:

```text
Unity Editor version 6000.4.10f1 (feeafc12a938)
Build type: Release
Architecture: arm64
Running under Rosetta: NO
Batch mode: YES
```

This report calls it exactly “Unity 6000.4.10f1”; it does not infer another
release label.

The C# probe forces synchronous AssetDatabase import, captures importer
warnings/errors, and exits nonzero on mismatch. Its canonical report has 226
measurement checks. The shell helper deletes the report before launch and
requires one execution anchor emitted by the live C# method, so a previously
written report cannot substitute for Unity.

The asymmetric reference has raw FCAD extents 1000 × 2000 × 3000 mm, four
non-coplanar vertices, four triangles with distinguishable winding, authored
per-corner normals, two material slots, and two base colours. It has an FBX
root, a transformed assembly parent, two differently transformed placements
of one geometry, an empty omitted child, and four named control points below
the first placement. Both placements deliberately use the display name
`Repeated Part` but carry different stable-key properties.

The independent gate uses ufbx 0.23.0 in strict mode, pinned to commit
`fcc5d6ba444cfd3eb80677dba5e37e493941abe5`. It reads the raw FBX rather than
the Unity asset and confirms format/version, global settings, connections,
hierarchy, local transforms, geometry sharing, polygon indices, authored
normals, material assignments, and colours. It is a measurement dependency,
not a production dependency decision.

## STEP transform classification before TRS policy

The real OCCT integration measurement classified every placement in the 14
STEP fixtures, including the full complex assembly. Eleven files imported and
three deliberately damaged files rejected; the imported files contained 170
placements in total.

| Property | Whole imported corpus | Complex fixture |
| --- | ---: | ---: |
| transforms | 170 | 140 |
| finite | 170 | 140 |
| determinant min / max | 1 / 1 | 1 / 1 |
| orthogonal | 170 | 140 |
| uniform scale | 170 | 140 |
| non-uniform scale | 0 | 0 |
| reflection | 0 | 0 |
| shear | 0 | 0 |
| singular | 0 | 0 |
| scale min / max | 1 / 1 | 1 / 1 |

The measured corpus is representable by Unity local TRS. No shear/reflection
fallback or bake policy is guessed. A future source that fails the same
classification must stop export with a typed refusal until a separately
designed bake policy exists.

## Format and encoding

The full semantic fixture is FBX 7.4.0 ASCII (`FBXVersion: 7400`). Unity
6000.4.10f1 accepted it with no importer warning/error. A trusted FBX 7.4.0
binary asset from the installed editor distribution was independently
identified as binary by ufbx and was also accepted with a non-empty mesh and
no importer warning/error.

The single contract for the next slice is **FBX 7.4.0 ASCII**. Binary 7.4
acceptance is recorded as compatibility evidence only; it is not a reason to
select a production writer. Other versions were not claimed or inferred.

## Axis and handedness measurement

The chosen raw FBX settings read independently as:

```text
right = +X
up = +Y
front-opposite-forward = +Z
unit = 1 metre
```

FerriteCAD is right-handed, Z-up, in millimetres. The writer-side coordinate
map selected from the matrix is:

```text
C_fbx(x, y, z) = (x, z, -y) * 0.001
```

Its rotation-only determinant is +1. Raw polygons therefore keep FerriteCAD
winding, and raw normals use `(nx, ny, nz) → (nx, nz, -ny)` without a winding
flip by the writer.

Unity then measured an additional X reflection. The observed world-coordinate
map is:

```text
C_unity(x, y, z) = (-x, z, -y) * 0.001
```

This is an importer result, not an assumption from Unity documentation. For
the selected fixture the imported root has identity local rotation and
`[1,1,1]` scale. The assembly's raw local translation `[0.1,0.3,-0.2]`
becomes Unity local `[-0.1,0.3,-0.2]`; the committed report records every
local quaternion and world matrix.

Changing only the raw Z-up metadata to Y-up produced different Unity matrices.
The valid raw Z-up/mm comparison reached the same metre distances but introduced
an axis-conversion rotation at the imported root. This distinguishes metadata
application from “coordinates happened to look right” and is why the selected
contract preconverts to Y-up.

The raw first polygon remains `[0,2,1]`. After Unity's X reflection, the
imported split-vertex indices are correspondingly reordered; the four measured
geometric orientations remain the intended orientations. Thus the future
writer does not reverse winding, while Unity's importer performs the required
handedness/index conversion. The exact raw polygons, Unity indices, geometric
normals, and imported authored normals are semantic gates.

## Units

For the selected metre-valued/metre-metadata fixture Unity reported:

| `ModelImporter` property | Measured value |
| --- | --- |
| `fileScale` | 1 |
| `useFileScale` | true |
| `globalScale` | 1 |
| `bakeAxisConversion` | false |

There is no hidden root scale. The named control-point distances from the
origin are exactly 0, 1, 2, and 3 Unity world units for FCAD distances 0, 1000,
2000, and 3000 mm.

Two negative controls separate the common mistakes:

- millimetre-valued coordinates falsely marked as metres measured 0, 1000,
  2000, and 3000 Unity units;
- already-metre coordinates falsely marked as millimetres measured 0, 0.001,
  0.002, and 0.003 Unity units.

The future scene builder performs mm → m exactly once before the writer. The
FBX declares metre units (`UnitScaleFactor = 100` in FBX centimetre terms).
No additional hierarchy scale or `globalScale` compensation is allowed.

## Hierarchy, local transforms, instances, and names

The independent reader sees nine explicit FBX nodes and one geometry. It sees
both `Repeated Part` model connections pointing to that same geometry and
retains their distinct local transforms below `Assembly Frame`.

Unity measures nine GameObjects, two MeshFilters, one unique Mesh asset, and
reference-equal `sharedMesh` values for the two placements. The assembly and
placement local transforms are not accumulated twice; every world matrix is
recorded independently. The four control points below the first placement make
local-vs-world and parent-loss mutations observable.

Unity uses the FBX asset filename for its prefab root rather than retaining
`FCAD_ROOT` as that GameObject's visible name. It also renames the second equal
sibling display name to `Repeated Part 1`; the nodes remain distinct and their
stable-key properties remain distinguishable in the import callback. Unity
also changes sibling order, so neither auto-renaming nor imported sibling order
is a durable identity contract.

The future writer must therefore emit deterministic unique `stable_name`
values for FBX node names and retain the source display name/key as explicit
metadata. Definitions and nodes are ordered deterministically, parents before
children; equality of display names never merges nodes.

## Normals and materials

The raw mesh has 4 control vertices, 12 polygon vertices, 4 triangles, 12
authored per-corner normals, and face material assignment `[0,0,1,1]`. Unity
creates 9 split vertices, 12 indices, 2 submeshes, and 9 normals. The measured
normal sequence is the transformed authored axis pattern, not geometric
recalculation. A mutation replacing it with recalculated normals fails.

Both placements have two material slots. Unity reports base colours
`[0.8,0.2,0.1,1]` and `[0.1,0.35,0.9,1]`. In the measurement project's gamma
colour space, `Color.linear` measures approximately
`[0.603827,0.033105,0.010023,1]` and
`[0.010023,0.100482,0.787412,1]`.

FerriteCAD imported colours are stored as linear RGB. The future scene-to-FBX
boundary therefore converts finite clamped linear RGB to sRGB exactly once
with the standard piecewise sRGB transfer before writing FBX base colour. The
writer preserves deterministic material-slot order and node overrides; Unity's
linear reading must recover the source linear value within the gate tolerance.

## Partial assembly policy

Unity retains `Omitted #2583` as an empty hierarchy node below the assembly.
The independent reader sees the same node with no mesh. Unity's
`OnPostprocessGameObjectWithUserProperties` callback exposes
`FerriteCADGeometryOmission`, `FerriteCADComplete=False`, and the stable node
key. Those properties are importer-callback data, not an automatically created
runtime component.

The future exporter must:

1. retain definition `#2583` and each of its placements as empty hierarchy
   nodes with deterministic stable names;
2. write an explicit omission marker/property where the FBX writer supports
   it, without treating that marker as the sole report channel;
3. always emit a deterministic CLI omission report listing definition key,
   placements, typed observation, and reason;
4. reserve exit code 6 as the separately identifiable non-clean
   `partial export` result;
5. mark the asset incomplete and never describe 34 tessellated leaf
   definitions as the complete 46-definition assembly;
6. keep `#2428` as real geometry and never heal, sew, or invent geometry.

The FBX file may still be useful, but a file and report describing a partial
assembly form one result. A clean-success claim is forbidden.

## ExportScene decision for §22B-1b

`RenderSnapshot` is not the exporter input. The next slice introduces one
kernel-neutral, immutable, read-only `ExportScene` before any writer:

```text
document
  ├─ imported: persisted Scene hierarchy + one cold geometry rebuild
  └─ native: one cold rebuild in document order
                     │
                     ▼
            immutable ExportScene
        definitions: mesh/material data once
        nodes: parent, stable name, display metadata,
               local transform, definition ref,
               material/linear-colour override, omission
                     │
                     ▼
             FBX writer boundary
```

The two source roads converge before format-specific code. One export performs
no second solve, no second STEP read, and no external STEP access. Mesh
definitions own geometry once; occurrence nodes own parent-local placement and
reference the definition. Transient pick IDs, GPU buffers, packed draw lists,
camera state, and accumulated viewport placements never enter `ExportScene`.

The prospective writer boundary accepts only `&ExportScene`, deterministic
options, and a byte sink, and returns written bytes plus the deterministic
completeness report. It cannot open a Document, call a kernel, solve, read STEP,
or inspect a RenderSnapshot. The concrete production writer/dependency remains
unselected in this measurement slice.

Deterministic order is document/source definition order, then parent-before-
child node order, then source material-slot order; hash-map iteration is not
observable. ufbx strict reading remains an independent gate regardless of the
eventual production writer.

## Gates and scope boundary

The semantic campaign kills 14 runtime mutants: Y/Z axis swap, handedness
without winding repair, mm-as-metres, double unit conversion, accumulated
transform twice, world-for-local transform, per-placement mesh duplication,
parent loss, equal-name node merge, normal recalculation, material/colour loss,
partial-as-complete, omitted-node removal, and RenderSnapshot-as-structure.

Harness controls separately refuse anchor miss, multiple anchors, stale
`.mutbak`, a prewritten report without live Unity import, zero-check output,
and a real non-compiling C# probe. Compile refusal and zero-check refusal are
not counted as mutation kills. There are zero unexpected survivors; the real
Unity baseline is rerun after byte-for-byte source restoration.

This slice stops here. §22B-1b may implement the `ExportScene` boundary and
evaluate a production writer against this contract, but no such implementation
is part of §22B-1a.
