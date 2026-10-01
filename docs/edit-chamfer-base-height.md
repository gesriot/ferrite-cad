# §29B — change the height of a chamfered plate, in a new copy

[Executed verification and limitations](edit-chamfer-base-height-verification.md).
[The Chamfer this keeps](rectangular-corner-chamfer.md);
[the same edit under a Fillet](edit-fillet-base-height.md).

A person opens a saved rectangular plate with the one §29A Chamfer, changes the
plate's Blind height in the existing **Edit extrusion** form, and saves a new
`.fcad`. An agent does the same with the existing `edit-extrude`. The Chamfer
is the same edge at the same distance, the Sketch is the same Sketch; only the
height changed. There is no new command, request format, copier or history
table.

## Contract recorded before implementation

### The supported source

Exactly the §29A class, read again from the saved history on every discovery,
preparation and write by the one reader the distance edit uses
(`chamfer::saved_chamfer`, over `cut_edit::saved_history_under_chamfer`):

* one untransformed XY datum, one Sketch of four Lines forming an axis-aligned
  rectangle that is free or closure-only (the four Coincident links);
* one forward literal Blind `Extrude`/`NewBody` (a formula or parameter height
  is refused as today), no Cut and no Fillet;
* one Chamfer whose `previous` and `edge.feature` are that Extrude, whose joint
  is a corner of that rectangle and whose distance answers to the §29A policy;
* one Body whose tip is the Chamfer, and exactly the plane, profile,
  predecessor and body-tip dependencies;
* exactly the seven names §29A gives the Chamfer, by meaning, and no other
  name it owns.

The selected `--feature` must be that Chamfer's `previous`, the base Extrude;
the existing literal-Blind check (`editable_extrude`) still applies to it. Only
the height becomes editable here.

Everything else keeps refusing a chamfered part **by naming the Chamfer UUID**:
`refuse_chamfered` is not disabled for any other editor — the Sketch
coordinates and constraints, circle and annulus edits, Cut add and edit,
Revolve edits, a Fillet or a second Chamfer, and `chamfer-edge-copy` on a
chamfered plate. `edit-chamfer-distance` is unchanged and works on the copy
this edit publishes.

Refused through the existing typed routes, each with the guilty feature's UUID
and nothing written:

* the Chamfer, the Sketch or any feature other than the base Extrude;
* a document whose Chamfer is outside the class (two Chamfers, a Fillet or a Cut
  beside it, a non-rectangle, a dimension, a foreign or extra name, a formula
  height, ThroughAll/Symmetric/reversed);
* a zero, negative or non-finite height (`input`, as today).

### Height policy — measured first

Probed through the product's own `chamfer-edge-copy` on pinned OCCT 8.0.1: the
37.5 × 12.25 mm plate at (−4.5, 3.25), the corner (33, 3.25), distances 0.001,
2.375 and 12.24 mm (the §29A maximum), heights from 1e-7 to 1e5 mm; then each
built plate exported to STL.

| height (mm) | every distance |
| --- | --- |
| 1.2e-5 … 1e5 | builds; exports; STL volume within tessellation of `(W·D − d²/2)·h` |
| 1e-5 and below | OCCT refuses to chamfer the edge (`kernel`) |

The limit does not depend on `d`, and `d`'s own bound (shorter adjacent side −
0.01 mm) does not depend on `h`. So the domain adds **no height bound of its
own**: the policy stays the existing `edit-extrude` rule, a finite positive
distance, applied by the shared validator. A height OCCT cannot chamfer is
refused by the kernel during the copy's strict rebuild, typed `kernel`, and
nothing is published. Nothing is clamped, and the plate alone (no Chamfer) can
still be made that thin — the refusal is the Chamfer's, so it is reported as the
kernel's.

### Identity — what may change

The only model change is the base Extrude's Blind distance: that row's `payload`
and `payload_hash`, plus the established `meta.modified_at` stamp. Everything
else is preserved byte for byte: the document id, every other object row
(including the Chamfer's payload — UUID, `previous`, edge producer, joint,
distance), every dependency, every topology reference UUID and meaning, object
names/ordinals/parents, the Body's tip and every capability row. Nothing is
minted, deleted or recreated, and no solved geometry or reference is written.
The selected corner and the Chamfer distance are the same; the plane of the
cut, its outward normal and its area (`d·√2·h`) follow the new height.

### Mechanism (all shared)

* **Discovery** comes from the one pinned snapshot `ExtrudeEditSource`; the
  base Extrude's row carries the saved Chamfer as context
  (`ExtrudeChoice::chamfer`), the other Extrudes refuse naming it. Additive
  JSON: `features[].chamfer_base` (`chamfer_feature_id`, `body_id`, `edge`,
  `corner_mm`, `distance_mm`, `distance_unit`), `null` elsewhere. Every
  existing field and type is unchanged.
* **Preparation** `prepare_extrude_height` reads the Chamfer first through
  `saved_chamfer`; the prepared edit carries it and the complete content
  version of the snapshot.
* **Write** `write_extrude_height` re-derives the prepared edit in its own
  transaction (a forged or stale edit fails), then updates one row and the stamp
  exactly as for a Cut history or a Fillet; the legacy standalone writer is not
  used.
* **Job** the existing `edit_object_copy`: pinned snapshot and version guard,
  source/alias/no-clobber, strict cold rebuild that resolves every saved name,
  cancellation and cleanup, atomic publication, exit 7 after publication.
* **Evaluator and cache** unchanged: the Chamfer judges the Lines the rebuild
  built, its cache key includes the predecessor's key, so a new height misses
  for the base and the Chamfer and never returns the old plate.

### UI

The existing **Edit extrusion…** form shows the Chamfer as context ("Chamfered
by Chamfer … at (x, y), d … mm; the Chamfer keeps its edge and distance; only
the plate's height changes."), validates the number through the same shared
rule, and saves through the same worker into a new file that opens
asynchronously. This form has **no** Undo/Redo and none is claimed. A refusal
and **Save Cancel** keep the draft.

### Out of scope, refused

Sketch or constraint edits under a Chamfer, a second Chamfer, a Fillet or Cut
beside it, changing the Chamfer's edge, ThroughAll, an arbitrary plane, chains,
in-place Save and live preview. Milestone 5C is not complete.
