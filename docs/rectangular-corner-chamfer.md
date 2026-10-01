# §29A — one equal-distance Chamfer on one vertical edge of a rectangular plate, in a new copy

[Executed verification and limitations](rectangular-corner-chamfer-verification.md).
[History decision](decisions/0004-feature-predecessor.md); the stack it parallels is
[§28A](single-edge-fillet.md) (create) and [§28B](edit-fillet-radius.md) (edit the one number).

A person or an agent opens a saved rectangular plate, picks one of its four
vertical edges and a distance, and publishes a new `.fcad` whose Body ends in a
real `feature.chamfer`. The copy reopens, rebuilds cold or from the cache,
exports to STL/FBX, and the distance is edited in another new copy. This is one
feature and one number, not a chamfer system.

## Commands

Existing names are `fillet-edge-copy` (create) and `edit-fillet-radius`,
`edit-revolve-angle`, `edit-extrude` (edit one named parameter of a feature).
The Chamfer follows them:

```text
ferritecad chamfer-edge-copy <source.fcad> --body UUID --expect-version HASH \
    --request <request.json> -o <copy.fcad> [--json]
ferritecad edit-chamfer-distance <source.fcad> --feature UUID --expect-version HASH \
    --request <request.json> -o <copy.fcad> [--json]
```

`edit-chamfer-distance` (not `edit-chamfer-copy`) because the existing edit
commands are named for the parameter they change, and a Chamfer has exactly
one. Requests (strict: unknown fields, arrays, extra keys, non-finite, zero or
negative numbers, malformed UUIDs are refused; nothing is parsed from prose):

```json
{"request_version":1,"edge":{"feature_id":"…","joint":["<Line UUID>","<Line UUID>"]},"distance_mm":2.5}
{"request_version":1,"distance_mm":3.0}
```

Unit: **millimetres**. `distance_mm` is the distance from the edge **measured
along each of the two adjacent faces** (equal on both). It is *not* the width
of the slanted flat, which is `distance_mm * sqrt(2)`.

Exit codes and the `{ok,…}`/`{ok:false,error}` envelope are JSON v1 as for every
copy command: 0 published, 2 refused (nothing written), 7 published but the
report was lost.

## The class

* one untransformed XY datum; one Sketch of four Lines forming an
  axis-aligned rectangle; **free or closure-only** (no constraint, or only the
  four Coincident closure links); a Sketch with any other constraint is refused
  with the constraint's UUID and the kind;
* one forward literal Blind `Extrude`/`NewBody`, one Body, the plane/profile/tip
  dependencies and nothing else;
* **no earlier Cut, Fillet or Chamfer**. Exactly one Chamfer, on one of the four
  vertical edges. A second Chamfer, a Chamfer on a rounded Body, a Fillet on a
  chamfered Body, a Cut on it, a cap/arbitrary edge, unequal distances, an angle,
  in-place Save and live preview are refused by name with their reason.

Fractional sizes, translated (offset) rectangles, either winding and any
starting Line are accepted; nothing below depends on them.

## Architecture (ADR 0004, unchanged)

`Chamfer.previous` names the Extrude whose result it modifies
(`Predecessor` edge Chamfer → Extrude); the Body names its tip (`BodyTip` moves
Extrude → Chamfer); ownership is derived. No feature→Body edge, no second
table, no cycle. The validator's `forked-history`, `shared-tip`, `shared-history`
rules cover it as they cover a Fillet.

The edge is the base Extrude's vertical sweep edge at one corner, named by its
producer and the unordered `ProfileJoint` of the two adjacent Line UUIDs — the
same meaning `ExtrudeSweepEdge` carries. No OCCT index, nearest edge, or
coordinate selects it.

### Document model

New kind `feature.chamfer`, payload **v1** (it did not exist before, so v1 is its
first layout):

```text
Chamfer { previous: ObjectId, edge: SweptEdge { feature, joint }, distance_mm: f64 }
```

`SweptEdge` is the neutral name of the struct that was `FilletEdge`; its fields,
`deny_unknown_fields` and therefore the persisted and wire form of a Fillet are
unchanged (`FilletEdge` stays as an alias). `edge.feature == previous` is
required: this slice chamfers the Extrude's own edge.

* **Capabilities:** the object requires `core.part.v1`, `feature.predecessor.v1`
  and the new `feature.chamfer.v1`. The new role `EdgeChamferFace` requires
  `feature.chamfer.v1`; the origin roles keep `topology.origin-face.v1`.
* **Why a new capability and kind:** a `main` build does not know
  `feature.chamfer`. It preserves the object verbatim and opens the document
  **read-only**; it cannot build the Body whose tip is a Chamfer, so it never
  shows a partial plate. Run against a real `main` binary (see the verification
  record). The SQLite schema does not move; the archive format version stays 4
  (one appended tag, below).

## Kernel operation

`GeometryKernel::chamfer_edge(ChamferRequest{target, edge, distance}, track, ctx)
-> ChamferResult`, default `Unsupported` (a test double never answers with an
invented solid). The OCCT bridge gains `fc_occt_chamfer_edge` and
`fc_occt_chamfer_faces`: `BRepFilletAPI_MakeChamfer::Add(distance, edge)` — the
**symmetric** form, which takes no reference face, so equal-distance cannot
depend on any arbitrary face or walk order (measured identical to
`Add(d, d, edge, face)` for either face of the edge). Success requires `IsDone`,
exactly one solid, `BRepCheck_Analyzer` valid, a positive removed volume, and one
face `Generated` from the edge that lies in the result. The answers for every
registered sub-shape of the target (kept, modified, deleted) are read while the
builder is alive, exactly as for a Cut or a Fillet, and the Rust side validates
them again. The two operations share the shim's post-build code; the Fillet's
behaviour does not change (its tests are the proof).

## The distance policy (measured on OCCT 8.0.1, then chosen)

37.5 × 12.25 × 6.75 mm plate, edge at (33, 3.25) (shorter adjacent side
12.25 mm). Both forms (`Add(d,E)` and `Add(d,d,E,F)` for either face) agree.

| d (mm) | OCCT |
| --- | --- |
| 1e-9 | `IsDone` false |
| 1e-7 … 12.249 | valid, 7 faces, volume `(W·D − d²/2)·H` to 1.7e-16 relative, flat area `d·√2·H` |
| 12.25 (= the side) and larger | `IsDone` false |

Chosen constants (own, not the Fillet's `MIN_RADIUS_MM`/`MAX_RADIUS_FRACTION`):

    MIN_DISTANCE_MM = 0.001 ≤ d ≤ min(len_a, len_b) − MIN_FLAT_MM,   MIN_FLAT_MM = 0.01

* **Finite**, positive, and at least `MIN_DISTANCE_MM`: 10⁴ × the kernel linear
  tolerance, three orders above the smallest distance OCCT still builds
  (1e-7), a flat 1.4 µm wide, hundreds of f32 ulps in the STL.
* **At most the shorter adjacent side less `MIN_FLAT_MM`**: a chamfer of `d`
  leaves `len − d` of each adjacent face; OCCT refuses at `d = len`, and a face
  of a few µm would be a sliver a person did not intend. 0.01 mm is the smallest
  feature this build keeps (the Fillet policy's flat).
* The bound is **one expression**, `ChamferCorner::max_distance_mm`, used by
  discovery (the offered max), preparation, the writer's re-derivation and the
  evaluator. The offered max is accepted and `max.next_up()` is refused with the
  numbers. Nothing is clamped, no tolerance is widened.
* A corner whose bound is below `MIN_DISTANCE_MM` is listed with that fact and
  refused.

A distance outside the class never changes another corner and never returns the
unmodified block.

## What is measured (not only mesh)

Removing the corner prism of a W × D × H block gives:

* B-Rep volume `(W·D − d²/2)·H`;
* 7 faces: two trimmed caps, four sides (two trimmed, two kept) and **one planar
  slanted face** whose area is `d·√2·H`, whose outward normal is the diagonal
  `(±1, ±1, 0)/√2` of **the chosen corner** (not another), and which passes
  through the points `d` along each adjacent edge from the corner;
* the chamfer face is bound to exactly the two Line UUIDs of the joint.

A plane at another corner with the same volume is a failure, by the normal and
the points.

## Names

* Existing references keep their producer (the base Extrude's historical
  output); they are never repointed.
* The Chamfer's own references, all produced by the Chamfer:
  * `EdgeChamferFace { edge_feature: base, joint }` — **new role**, the one
    planar face made from that edge (`Face`, `Exact`). Not `EdgeFilletFace`:
    one is a cylinder, the other a plane, and the roles do not resolve
    against each other.
  * `OriginCap { base, Start|End }` ×2, `OriginSide { base, segment }` ×4 —
    through the Chamfer's own OCCT history: the two sides at the corner and both
    caps are trimmed (`Modified`), the other two sides are kept. A name the
    operation removed is recorded removed, never matched by geometry.
    Seven references in all.
* Edges and vertices of the base remain historical at the base producer; the
  Chamfer tip publishes no edge or vertex names in this slice.

## Cache and archive

Cache key `eval.chamfer.named`: the predecessor's identity and key (content),
the edge producer and canonical joint, the distance bits, the kernel and the
tolerance. Changing `d` invalidates the Chamfer; an upstream change changes the
predecessor key and cannot give a stale Hit. The topology archive gains one
appended tag (`TAG_EDGE_CHAMFER_FACE = 21`), in the same vocabulary-only way
`TAG_EDGE_FILLET_FACE` arrived: format version 4 is unchanged because the layout
is, and a build that does not know the tag treats the entry as malformed and
rebuilds. Cold, Miss and Hit give the same Body, names and refusals.

## SQL allowlists

**Create** (`chamfer-edge-copy`):

* `objects`: one new Chamfer row; the Body row's payload and payload hash (its tip);
* `deps`: + `Chamfer → Extrude` (Predecessor), + `Body → Chamfer` (BodyTip),
  − `Body → Extrude` (BodyTip);
* `topology_refs`: + 7 rows (the names above);
* `capabilities`: `feature.chamfer.v1`, `feature.predecessor.v1` and
  `topology.origin-face.v1` if not yet declared;
* `meta.modified_at`.

**Edit** (`edit-chamfer-distance`): the one Chamfer row's payload and payload
hash, and `meta.modified_at`. Its UUID, every reference UUID and every other
cell are unchanged.

Every other SQL cell and identity is byte-identical, and the source file is
read-only throughout.

## Copy operation

One jobs route for creation (`chamfer_edge_copy`) and one for the edit
(`edit_chamfer_distance`), both through the existing `edit_object_copy`: one
snapshot and a version guard; the source read-only; no-clobber and alias
refusal; a baseline cold rebuild; the write; a strict rebuild in which every
baseline reference and every minted one must resolve; the version checked again;
SQLite closed; atomic publication; cancellation boundaries; exit 7 after a
completed publication. The writer re-derives the whole prepared value inside
its transaction. There is no second copier, no second file read for the DTO and
no direct write from the UI.

## The other editors

A chamfered Body is outside every earlier frame. The shared refusal
(`refuse_filleted`, which every reader of the plate calls first) names the
Chamfer by UUID; so do the height, Sketch coordinate, constraint, Cut, Fillet,
Revolve and radius routes. A Chamfer is never silently dropped, ignored or
rebuilt on a changed plate by an editor that does not know it. Discovery reports
the saved Chamfer and which of these are unavailable, with the reason. Editing
the base of a chamfered plate is a later slice.

## Discovery (additive JSON)

`bodies[].chamfer_edge` — `{available, target:{previous_feature_id, height_mm,
min_distance_mm, min_flat_mm, constrained:false, candidates:[{edge:{feature_id,
joint:[a,b]}, corner_mm, adjacent_lengths_mm, max_distance_mm}]}, refusal}` — the
four corners, labels not identity — and top-level `chamfers[]` — `{feature_id,
body_id, previous_feature_id, edge, corner_mm, distance_mm, max_distance_mm,
editable, refusal, request_versions:[1]}`. Existing fields, types and codes are
unchanged; a `main` build that does not know them ignores them. Without a kernel
discovery is the same and creation refuses (`unsupported`, exit 2) and writes
nothing; Open CASCADE without the solver works for this class.

## UI

`Chamfer edge of <Body> — <UUID>…` and `Edit chamfer distance of <feature>…` open
bounded forms with the same worker as the CLI: a list of the four corners
(coordinates and the two side lengths as labels), a distance field in mm with
the allowed range, **Apply**/**Save**/**Cancel**; the draft survives a refusal
and **Save Cancel**; a published copy opens asynchronously. The Chamfer's
distance form has no whole-request Undo/Redo — this form is one number, as the
Fillet radius form is, and none is claimed.

## Recipe

[The extractable recipe](rectangular-corner-chamfer-verification.md) runs these
commands for real and prints `FCAD_29A_RECIPE_OK` (`…_NO_KERNEL` without Open
CASCADE).
