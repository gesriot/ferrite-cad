# §28D — move or resize the rectangle under a Fillet, in a new copy

[Executed verification and limitations](edit-fillet-base-sketch-verification.md).
[The Fillet this keeps](single-edge-fillet.md);
[its radius edit](edit-fillet-radius.md); [its plate's height](edit-fillet-base-height.md).

A person opens a saved rounded plate — the §28A Fillet on one vertical edge —
changes the width, depth or position of its base rectangle in the existing
**Edit Sketch** (typed coordinates, or dragging vertices with Undo/Redo), and
saves a new `.fcad`. The rounding stays on the same named corner with the
same radius, at the rectangle's new corner. An agent gets the same copy with
the existing `edit-sketch-copy`, having found the Sketch in `inspect --json`.
There is no new command, request format or copy pipeline.

## Contract recorded before implementation

### The supported source

Exactly the frame §28B and §28C edit, read again from the saved history on
every discovery, preparation and write, by the same function
(`fillet_radius::fillet_over_plate`, over `cut_edit::saved_history_under_fillet`
and the §28A names): one untransformed XY datum; one unconstrained Sketch of
four Lines forming an axis-aligned rectangle; one forward literal Blind
`Extrude`/`NewBody`; one Fillet whose `previous` and `edge.feature` are that
Extrude and whose joint is a corner of that rectangle; one Body whose tip is
the Fillet; no Cut; exactly the plane, profile, predecessor and body-tip
dependencies; exactly the seven §28A names. The selected Sketch must be that
Fillet's profile.

The existing coordinate editor's rules still apply unchanged: the request
names every saved Line UUID once in saved order, the loop stays exactly
closed and in the same winding, and the Extrude's polygon policy holds.

### What a candidate rectangle must be

Judged by the functions the Fillet already uses, on the candidate Lines:

* it is an axis-aligned rectangle of the same four Lines
  (`rectangle_corners`);
* **every Line keeps its side**: the same axis and the same direction as
  saved (a Line running +X stays running +X). This forbids rotating or
  mirroring which Line is which side, even when the result would still be a
  rectangle of the same winding; the rounded corner is therefore the same
  corner of the part, not merely the same pair of UUIDs moved elsewhere;
* the saved joint is still a corner of it (`corner_for`), found by its two
  Line UUIDs — never by an index, a row or the nearest coordinates, so a CW
  loop or another starting Line changes nothing;
* the **saved radius** still fits that corner under §28A's unchanged policy
  (`check_radius`: `r ≤ ½ · min(adjacent Lines)`), with the numbers in the
  refusal. Nothing is clamped and the radius is never changed to fit.

Anything else is refused, typed, before any write: a non-rectangle, a Line
that changes side, a rectangle too small for the saved radius, constraints, a
reordered or reversed loop.

### Identity — what may change

Only the selected Sketch row's `payload` and `payload_hash`. The coordinate
writer does not stamp `meta.modified_at` (as for every coordinate edit since
§25B); the allowlist admits it, and the verification reports what actually
moves.

Preserved byte for byte: the document id; every other object row, including
the base Extrude (height), the Fillet (UUID, `previous`, edge producer and
joint, radius) and the Body; the curve UUIDs and their order; every
dependency, topology reference and capability row. Nothing is minted.

### The exact SQL allowlist

| table | allowed to differ |
| --- | --- |
| `objects` | the base Sketch row's `payload` and `payload_hash` |
| `meta` | `modified_at` |
| every other table, including `deps`, `topology_refs`, `capabilities` | nothing |

Row counts are equal in every table.

### Writer, version and references

`Document::write_sketch_geometry` unchanged: inside its transaction it checks
the Sketch row is the one prepared, re-derives the edit from the new
coordinates through the same preparation (and so through the Fillet frame
and radius check against the Fillet as it is now), and compares the whole
prepared payload. A forged payload or one that no longer fits is refused.

Staleness is guarded where it is for every coordinate edit: the copy job's
`DocumentVersion` is the content hash of every row, the Fillet's payload
(radius, edge) and every topology reference included; it is checked against
the snapshot copied and again before publication.

Every saved name must resolve after the copy's cold rebuild (a coordinate
edit has always required that; the standalone height exemption is not
involved). The existing job gives snapshot, no-clobber, alias refusal,
cancellation, atomic publication and exit 7 on a lost report.

### Evaluation and cache

No new route. The base Extrude's key covers its profile's coordinates and
`eval.fillet.named` is keyed by the predecessor's key, so a moved rectangle
misses on both and neither returns the old part.

### Clients

* **`inspect --json`**, additive only. The base Sketch's `sketches[]` row
  becomes `editable` with `vertices` and `profile_feature` as for any
  editable plate, and gains `fillet_base` — the same object `features[]`
  carries since §28C (`fillet_feature_id`, `body_id`, `edge`, `corner_mm`,
  `radius_mm`). It is `null` on every other row and in every other document.
  On a Fillet outside the frame the row stays refused, naming the Fillet and
  the reason.
* **`edit-sketch-copy`**: unchanged request, result, envelope and exit codes.
  Stub order: with `--json`, UTF-8 paths; the request read and parsed; the
  source opened; then the kernel — so a well-formed request is
  `unsupported` in a stub build.
* **UI:** the existing Edit Sketch form, canvas drag and Undo/Redo, one
  context line naming the Fillet, its corner and radius and the smallest side
  that radius allows; a refusal shown with its numbers; the draft kept on a
  cancelled Save and a worker refusal; publication through the async Open.

The constraint, circle, annulus, Cut and Revolve editors, the standalone and
Cut-history Sketch routes, and a second Fillet keep their behaviour; the
constraint editor still refuses a filleted part by name.

### Compatibility

No capability, schema, payload version, cache key or archive tag. A copy
carries its source's capability rows.

### Out of scope

An arbitrary quadrilateral, constraints, reordering or reversing the loop, a
rotated plane, a second Fillet, Chamfer, a Cut with a Fillet, preview, face
picking, in-place Save. The Fillet/Chamfer milestone stays open.
