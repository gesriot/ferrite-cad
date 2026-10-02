# §29C — move the corners of a chamfered plate, in a new copy

[Executed verification and limitations](edit-chamfer-base-sketch-verification.md).
[The Chamfer this keeps](rectangular-corner-chamfer.md);
[the height edit beside it](edit-chamfer-base-height.md);
[the same edit under a Fillet](edit-fillet-base-sketch.md).

A person opens a saved rectangular plate with the one §29A Chamfer, changes the
four stored vertices of its base Sketch in the existing **Edit saved Sketch**
form (drag, exact coordinates, Undo/Redo, Restore saved vertices) and saves a
new `.fcad`. An agent does the same with the existing `edit-sketch-copy`. The
plate may be made longer or shorter on either side and may be moved. The
Chamfer stays on the **same corner** — the corner its two Line UUIDs meet at —
with the same distance; the height, the Body tip, the history and every name
are kept. There is no new command, request, copier or history table.

## Contract recorded before implementation

### The supported source and candidate

The class is exactly §29A/§29B, read by the one reader the distance and height
edits use (`chamfer::saved_chamfer`, over `cut_edit::saved_history_under_chamfer`):
one untransformed XY datum; one Sketch of four Lines forming an axis-aligned
rectangle that is free or closure-only (the four Coincident links, kept
byte-for-byte); one forward literal Blind `Extrude`/`NewBody`; one terminal
Chamfer on a corner of that rectangle; one Body whose tip is the Chamfer;
exactly its seven names, and nothing else in the document.

The request is the existing one: the four `curve_id`s once each in stored
order with new start points. A candidate is accepted if and only if **all**
of these hold; the first violated rule is the refusal, naming the Chamfer where
it is the Chamfer's:

1. **The existing coordinate policy**, unchanged and shared: the saved UUIDs in
   saved order, finite numbers, the `PolygonExtrusion` policy of the Extrude the
   Sketch feeds (a simple polygon of non-degenerate Lines), and the saved
   winding. Nothing is reordered, re-sorted or re-wound.
2. **Still an axis-aligned rectangle of four Lines** (the same
   `corners_of_lines` reader the creation uses), so the Chamfer's corner is read
   again on the candidate.
3. **Every Line keeps its side** (`keeps_every_side`): each Line runs along the
   same axis in the same direction as saved. This is what makes the corner
   *follow its vertex*: a candidate that mirrors the plate, or moves the
   Chamfer's two Lines to the opposite corner, is refused, because the saved
   joint would then be a different corner of the part. Sides may be made longer
   or shorter and the whole rectangle may be moved, in any combination, as long
   as no Line flips.
4. **The saved joint is still one of its corners**, found by the two Line UUIDs
   alone (`corner_for`), never by an index or an absolute coordinate.
5. **The saved distance still fits the NEW adjacent sides**, under the one §29A
   policy and its one expression: `0.001 mm ≤ d ≤ min(adjacent sides) − 0.01 mm`,
   exact at the bound (the largest accepted value is accepted, the next
   representable value above it is refused). `d` is never reduced, clamped or
   rewritten, and the Chamfer's row is not touched.

The refusal of rule 5 reads "Chamfer `<uuid>` of `<d>` mm does not fit the new
plate: … the shorter adjacent side would be … mm, so it may be at most … mm". A
degenerate or non-rectangular candidate is refused by rule 1 or 2 with the
reason the shared reader gives; the other Chamfer-class refusals (a Fillet or Cut
beside it, a second Chamfer, a dimension on the Sketch, an extra name) name the
guilty feature or constraint UUID as for the height edit.

Validation is one function, `SketchChoice::validate_coordinates`, used by the
form while typing, by `edit-sketch-copy` before the copy, and again by the
writer inside its transaction; the UI and the CLI hold no copy of it.

### Identity — what may change

The only model change is the base Sketch row's `payload` and `payload_hash` (the
Line coordinates), plus the established `meta.modified_at` stamp. The Extrude
(height), the Chamfer (UUID, `previous`, edge, joint, distance), every
dependency, every topology reference, every other payload, names, ordinals and
parents, the Body tip and every capability row are preserved byte for byte.
Nothing is minted, deleted or recreated, and no solved geometry is stored. The
seven names resolve under the same UUIDs on the new solid, with the Chamfer's
plane now at the new corner position.

### Mechanism (all shared)

* **Discovery** from the one pinned snapshot: `sketches[]` gains the additive
  `chamfer_base` (the same object as `features[].chamfer_base`, §29B) on the base
  Sketch of a chamfered plate and `null` elsewhere; the Sketch row is
  `editable` exactly when the shared reader accepts the frame. Every existing
  field and type is unchanged.
* **Preparation** `replace_sketch_coordinates` reads the Chamfer through the
  frame (`coordinate_choice`), validates, and rewrites only the Line starts and
  ends of the selected Sketch.
* **Write** `write_sketch_geometry` re-derives the prepared payload in its
  transaction from the current document through the same function; a forged or
  stale payload is refused and nothing is written.
* **Job** the existing `edit_object_copy`: pinned snapshot and version guard,
  source/alias/no-clobber, strict cold rebuild (the evaluator judges the Chamfer
  again on the Lines it built), every saved reference must resolve before and
  after (the rule `CopyWrite::Coordinates` already has and the §29B review
  required for the height; it is not weakened and is not replaced by a count of
  the Chamfer's own seven names), cancellation and cleanup, atomic publication,
  exit 7 after publication.
* **Cache** unchanged: the Extrude's key follows its profile, the Chamfer's key
  the plate's, so a moved plate misses for both and never returns the old
  contour or corner.

### UI

The existing coordinate form names the Chamfer to keep ("Chamfered by Chamfer …
at the corner of Lines a | b, d … mm. The Chamfer keeps its corner and
distance: every Line keeps its side, and the shorter adjacent side may not be
shorter than … mm"). Undo/Redo, **Restore saved vertices** and drag (one gesture,
one Undo) are unchanged. While typing, the draft may be briefly not a
rectangle; the refusal is shown, Undo/Redo/Restore stay available and Save is
unavailable until the whole candidate is valid. No neighbouring vertex is moved
automatically to make a gesture succeed. A refusal and **Save Cancel** keep the
draft; a published copy opens asynchronously.

### Out of scope, refused

A second Chamfer, a Fillet or Cut beside it, a dimension or any non-closure
constraint (the constraint editors still refuse a chamfered plate by its UUID), a
different edge, ThroughAll, an arbitrary plane, in-place Save and live preview.
No new payload, capability, archive or schema version is needed or added.
Milestone 5C is not complete.
