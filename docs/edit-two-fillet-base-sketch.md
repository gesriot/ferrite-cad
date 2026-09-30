# §28J — move or resize the rectangle under two Fillets, in a new copy

[Executed verification and limitations](edit-two-fillet-base-sketch-verification.md).
[The same edit under one Fillet](edit-fillet-base-sketch.md);
[two sequential Fillets](sequential-edge-fillets.md);
[their radii](edit-sequential-fillet-radii.md);
[the plate's height under them](edit-two-fillet-base-height.md).

A person opens a plate that §28G rounded twice — Extrude → Fillet 1 →
Fillet 2 → Body tip — moves or resizes its base rectangle in the existing
**Edit Sketch** form (typed coordinates, dragging vertices, Undo/Redo,
Restore saved vertices), and saves a new `.fcad`. Both roundings stay on the
same named corners with the same radii, at the rectangle's new corners. An
agent does the same with the existing `edit-sketch-copy`. There is no new
command, request format, copy pipeline, solver or geometric route: this is
§28D's edit on §28H's reading of the history.

## Contract recorded before implementation

### The supported source

Exactly the §28G class, read again from the saved history on every
discovery, preparation and write by the reader the radius and height edits
use for two Fillets (`fillet_radius::fillets_over_plate` →
`saved_sequential_fillet`, over `cut_edit::saved_history_under_fillets`):
one untransformed XY datum; one Sketch of four Lines forming an axis-aligned
rectangle; one forward literal Blind `Extrude`/`NewBody`; **Fillet 1** with
`previous` = `edge.feature` = that Extrude; **Fillet 2** with `previous` =
Fillet 1, `edge.feature` = that Extrude, another corner, the Body's tip; no
Cut and no other object; exactly the plane, profile, two predecessor and one
body-tip dependencies; Fillet 1's seven §28A names and Fillet 2's eight §28G
names, by meaning.

The Sketch is free: no constraint, or only Coincident closure links at
adjacent Line joints (what the constraint editor leaves when its last user
constraint is removed, §28E). Such links reference curve UUIDs and endpoint
selectors, not coordinates, and the coordinate editor keeps the loop exactly
closed, so they stay satisfied and are kept byte for byte. A Sketch with any
other constraint (length, H/V, equal, Parallel/Perpendicular, Fixed) keeps
refusing coordinate editing: constraints under two Fillets are a later slice.
The selected Sketch must be the plate's base Sketch.

The one-Fillet frame (§28D) is unchanged, including its refusal of every
constrained Sketch. The constraint editor and add-Fillet keep reading
`fillet_over_plate`, which admits exactly one Fillet, and keep refusing a
two-Fillet history.

### Typed context

`SketchChoice.fillet` stays the Fillet on the base (Fillet 1); a new
`SketchChoice.second_fillet` carries Fillet 2 (`history_index` 2, `previous`
= Fillet 1). Both are the `SavedFillet`s `fillets_over_plate` returns; the
history is not re-checked by a copy of that reader. Both joints name corners
swept by the base Extrude.

### What a candidate rectangle must be

The existing coordinate rules (every saved Line UUID once in saved order,
exactly closed, same winding, the Extrude's polygon policy), then, jointly,
on the candidate Lines:

* an axis-aligned rectangle of the same four Lines (`rectangle_corners`);
* **every Line keeps its side** (`keeps_every_side`, as §28D);
* Fillet 1's saved joint is still a corner, found by its two Line UUIDs,
  and its saved radius fits there (`SavedFillet::corner_on`);
* the same for Fillet 2, its own joint and radius;
* the **pair rule** in history order on the candidate corners
  (`check_pair(c1, r1, c2, r2)`): on adjacent corners the shared Line keeps
  at least `MIN_RADIUS_MM` flat between the arcs; opposite corners share no
  Line. The same predicate the rebuild and §28H apply; its bound for Fillet 1
  (`pair_bound_of_first`) keeps the bounded bisection.

Nothing is clamped: no radius is changed, no corner reselected, no epsilon
added. A Line that changes side, a rectangle too small for either radius or
too short for the pair are `input`; a non-rectangle is `unsupported`.

### SQL allowlist

| table | allowed to differ |
| --- | --- |
| `objects` | the base Sketch row's `payload` and `payload_hash` |
| `meta` | `modified_at` (the coordinate writer does not stamp it; measured) |
| every other table and column | nothing |

Only the Lines' `geometry` inside the payload changes: curve UUIDs, their
order, `construction`, the plane and every constraint stay. Preserved byte for
byte: the document id, the base Extrude (height), both Fillet rows (UUIDs,
`previous`, edges, radii), the Body tip, every dependency, topology reference
and capability. Nothing is minted; no capability, schema or payload version.
Stored coordinates are never replaced by solver output.

### Writer, version, rebuild and cache

`Document::write_sketch_geometry` unchanged: inside its transaction it
re-derives the edit from the current document through the same choice (both
Fillets read again) and compares the whole prepared payload; version, alias,
no-clobber, cancellation, cleanup and strict references are the copy job's.
Every name must resolve after the cold rebuild, both cylinders under their
own names. The base Extrude's key covers the coordinates and each Fillet is
keyed by its predecessor, so a moved rectangle misses all three; a repeat
hits.

### Clients

* `inspect --json`: the base Sketch's `sketches[]` row becomes editable with
  `fillet_base` (Fillet 1) and its additive `second_fillet` (Fillet 2),
  previously `null` on `sketches[]` rows. Other fields keep shape and type.
* `edit-sketch-copy`: unchanged request, result, envelope, exit codes.
* UI: the existing Edit Sketch form, with one context line naming both
  Fillets in history order and the smallest sides they allow.

### Out of scope

Dimensional or geometric constraints under two Fillets, a third Fillet,
arbitrary edges, Cut with Fillet, Chamfer, reselecting a corner, in-place
Save.
