# §27A — full-turn Revolve / NewBody from a simple Line profile

[Executed verification and limitations](full-turn-revolve-verification.md).

The first vertical slice of Revolve from wave 5A: a person draws, or an agent
states, one closed Line profile and gets a new `.fcad` whose Body is that
profile turned once around an axis. The document stores the **intent** —
profile, axis, full turn — never a mesh, an import or an equivalent Extrude
or Boolean chain. Later editing of the Revolve, constraints, booleans on it,
face attachment, in-place Save, preview, fillets and new selection kinds are
out of scope. This slice does not complete Revolve or wave 5A.

## Contract recorded before implementation

### Coordinates, axis, units

* One untransformed XY datum, one Sketch on it. Millimetres throughout.
* On the sketch canvas **X is the radial distance** from the axis and **Y is
  the axial coordinate**.
* The axis is the sketch's **local Y axis through the datum origin**:
  `RevolveAxis::SketchY`. The angle is **exactly one full turn**, 2π:
  `RevolveExtent::FullTurn`. Both are named, typed values in the document and
  in the kernel request; no adapter supplies them as constants of its own.

### Supported profiles

A profile is accepted iff all of these hold:

* **Simple line polygon.** 3..256 vertices of one unconstrained closed Line
  polygon, validated by the same simplicity rules `PolygonExtrusion` uses:
  - no repeated vertex and no zero-length edge;
  - no collinear or backtracking vertex within 1e-6 mm;
  - no crossing or touching edges;
  - nonzero area;
  - |coordinate| ≤ 1 000 000 mm.

  Those rules are extracted into one shared function. The extrusion keeps its
  own order of checks and its messages.
* **Either winding.** Sloped walls, steps, and any translation along Y are
  accepted.
* **Strictly on the positive radial side.** Every vertex has
  `x > AXIS_CLEARANCE_MM` = 1e-6 mm. Because every edge is a straight segment
  between two such vertices, the profile then neither touches nor crosses the
  axis.
* **Refused:** zero or negative radius, touching or crossing the axis, a
  solid shaft that closes on the axis, partial angles, another axis or plane,
  several loops, construction or non-Line geometry, and constraints.

The same domain value, `FullTurnRevolution`, is checked in several places:
* the UI draft;
* the CLI request;
* the create job;
* the evaluator, again from the saved Sketch before any kernel call.

A profile the domain refuses is never published, even if the kernel could
build it.

### Persistence and capability

* **New object kind** `feature.revolve`, payload schema v1:
  `{profile, axis:"sketch_y", extent:"full_turn", operation:"new_body"}`.
* **New capability** `feature.revolve.v1`. It is required by:
  - that payload;
  - every stored `RevolveFace` reference;
  - the document's capability index.

  The Extrude capabilities are not widened.
* **Dependency edges:**
  - `revolve → sketch` (`Profile`);
  - `sketch → plane` (`Plane`);
  - `body → revolve` (`BodyTip`).

  The DAG and BodyTip rules are the ones Extrude uses; a Revolve is a feature
  for `Body.tip_feature`.
* **Old builds** (9d1a5f5 and earlier) do not know the object kind or the
  capability. They preserve the object verbatim and open the document
  read-only: a write or rebuild is refused rather than corrupting it. No SQL
  schema change.

### Topology outputs

* **Faces only.** Each profile Line raises exactly one face of revolution,
  read from `BRepPrimAPI_MakeRevol`'s own history (`Generated(edge)`):
  - a Line parallel to the axis gives a cylinder;
  - a Line perpendicular to it gives an annular plane;
  - any other Line gives a cone.
* **Role.** Stored as `SemanticRole::RevolveFace { profile_segment }`, where
  `profile_segment` is the Line's saved UUID. The selection is
  `AllDerivedFrom { ancestor: segment }` or `Exact`.
* **What is not stored.**
  - There are no start/end caps: a full turn has none, and the annular end
    faces are the rotations of the radial Lines.
  - No face index, traversal order, seam vertex or edge, and no
    `ProfileJoint` name.
* **History check.** Every Line must raise exactly one face of the finished
  solid, and no face may be claimed by two Lines. Anything else refuses the
  build.
* **Reference check.** A `RevolveFace` reference resolves only against a
  Revolve's names, and an `ExtrudeSide` reference never against them. There
  is no fallback to a similar face. Creation writes one `RevolveFace`
  reference per Line, and publication requires every one to resolve after the
  cold build.

### Kernel, evaluator and cache

* **Kernel.** `GeometryKernel::revolve(RevolveRequest) → RevolveResult`
  (shape plus segment → face history). The OCCT bridge adds a new entry point,
  `fc_occt_revolve`, with its own ABI:
  - the segments (`FC_OCCT_SEGMENT_LINE` only);
  - explicit model-space axis origin and direction;
  - an explicit full-turn flag.

  No existing argument is reinterpreted. The bridge re-checks everything it
  is given: finite inputs, the axis lying in the plane, the full turn, a
  valid solid with positive volume, and a face for every edge. It also
  handles cancellation, cleanup and handle ownership. The mock and the
  unavailable kernel implement the same contract.
* **Evaluator.** Cold and cached rebuilds share one path. The cache key hashes
  all of these:
  - the kernel identity;
  - the tolerance;
  - the profile (plane, labels, coordinates);
  - the axis;
  - the extent.

  The archive stores the faces under `RevolvedFace` names keyed by segment
  UUID, never by traversal order. A restored B-Rep still has every stored
  reference checked. Nothing solved or native is written to the document.

### CLI

`ferritecad create-sketch-revolve <request.json> -o <new.fcad> [--json]`,
strict request v1 (`deny_unknown_fields`, ≤ 65536 bytes):

```json
{"request_version":1,"points_mm":[[4,0],[10,0],[10,15],[4,15]],"axis":"sketch_y","angle":"full_turn"}
```

* **Required fields.** `axis` and `angle` are required and have exactly one
  accepted value each, so the request says what it means.
* **JSON output.** `--json` uses the shared envelope with
  `operation:"create-sketch-revolve"` and result `{destination, document_id}`.
* **Exit codes.** 0 published, 2 refused, 7 report delivery failed after
  publication.
* **Publication.** It goes through the shared create job, the same way
  `create-sketch-extrude` does: checked cold build and reference check, closed
  SQLite, atomic Keep publish, cleanup and cancellation.
* **Stub builds.** A build without a kernel refuses and publishes nothing.

`inspect --json` adds a top-level `revolves` array, read from the same pinned
snapshot. Each entry is:

```json
{"feature_id", "name", "body_id", "profile_sketch_id", "plane_id",
 "axis": "sketch_y", "extent": "full_turn", "operation": "new_body",
 "profile": {"available", "refusal", "segments": [{"curve_id","start_mm","end_mm"}]}}
```

`features`, `bodies` and `sketches` keep their fields and types:
* a Revolve is not listed as a feature, so an old consumer cannot take it for
  an Extrude;
* its Body's `cut_edit` blocks refuse;
* its Sketch's editors refuse.

### UI

The existing Line canvas and numeric editor gain an explicit **Revolve 360°**
action beside Extrude:
* the local Y axis is drawn and labelled "axis (Y)", and the positive radial
  side is labelled;
* the draft is checked by the same domain value.

Undo/Redo, the draft, Save Cancel, invalid input, a worker refusal and a
failed async Open behave as they do for creating an Extrude. Publication goes
through the same worker, job and Open path.
