# §27B — edit a saved full-turn Revolve profile in a new copy

[Executed verification and limitations](edit-revolve-profile-verification.md).

A person opens a part created by [§27A](full-turn-revolve.md) — a bushing, a
stepped or a sloped (conical) part — changes the coordinates of its saved
profile in the ordinary **Edit Sketch** window and saves a new `.fcad`. An
agent gets exactly the same result through the existing `edit-sketch-copy`.
Changing coordinates creates no new Line, no new Revolve and no new face
name. This slice is not Revolve editing in general and does not complete
Revolve or wave 5A.

## Contract recorded before implementation

### The document class

Exactly one standalone §27A document:

* four root objects, none with a parent:
  - an untransformed XY `DatumPlane`;
  - one unconstrained, closed Line `Sketch` on it;
  - one `Revolve` of that Sketch with `axis: sketch_y`, `extent: full_turn`,
    `operation: new_body`;
  - one `Body` whose tip is that Revolve;
* exactly three dependencies: `sketch → plane (Plane)`,
  `revolve → sketch (Profile)`, `body → revolve (BodyTip)`;
* a profile of 3..256 Lines, as accepted by `FullTurnRevolution`: a simple
  polygon, every vertex at X > `AXIS_CLEARANCE_MM` (1e-6 mm). Either
  winding, steps, sloped walls and any Y translation are accepted, as at
  creation.

Anything else refuses. That includes:
* constraints, construction geometry, Circles or Arcs;
* a transformed plane;
* extra objects, a second Body, or a Cut/Boolean on the Revolve;
* a Revolve with another axis or angle (not representable today, and
  refused if it ever is);
* unknown or future payloads, and documents that `copy_access` forbids.

The Extrude-based classes — §25B polygons, the §26F/§26I Cut-history base,
and the circle, annulus, constraint and height editors — keep their exact
boundaries. Their structure check (`frame`) is not changed.

### What may change

Only the coordinates of the saved Lines. The following stay identical:
* the curve UUIDs, their number and order, and the closure;
* the Sketch, Revolve, Body, plane and document IDs;
* the Revolve payload (axis, angle, operation), units and capabilities;
* every other SQL cell, apart from the selected Sketch's `payload`/
  `payload_hash`. As before, `meta.modified_at` is preserved too.

The start of Line *i* is the end of Line *i−1*. Each vertex is set once, so
both Lines move together. As for every coordinate edit, the winding may not
change. This refuses a mirror image of the profile rather than accepting a
different drawing under the same names.

### Face names under an edit

Each stored `RevolveFace { profile_segment }` means the face raised by that
Line. It does not mean a surface type that stays fixed. Moving a Line's ends
may change the analytic kind of its face, and that is allowed and measured:
* an axial Line whose ends get different radii turns from a cylinder into a
  cone;
* a Line that becomes perpendicular to the axis turns into an annular plane.

The reference must still resolve, and exactly to the face that Line raises
now. A reference that is lost or swapped is refused; it is never the price
of an edit.

### One owner of the rules

* `sketch_edit::coordinate_choice` stays the single place that decides
  whether a Sketch's coordinates are editable. Three callers use it:
  - discovery (`sketch_choices`);
  - preparation (`replace_sketch_coordinates`);
  - the writer's in-transaction re-derivation (`write_sketch_geometry`).

  It gains a second structure check, `revolve_frame`, beside the unchanged
  Extrude `frame`. Which one applies follows from the document: a Revolve
  whose profile is this Sketch selects the Revolve frame. There is no
  fallback between them.
* **Explicit profile kind.** `SketchChoice` states which feature uses the
  profile, as an explicit `SketchProfileUse`:
  - `BlindExtrude { feature, height_mm }`;
  - `FullTurnRevolve { feature, body }`.

  It replaces the bare `height_mm: Option<f64>`. A Revolve is never given a
  height, a fictitious Extrude, or an implied UUID.
* **Numeric policy.** `SketchChoice::validate_coordinates` checks a draft
  with the policy of that kind: `PolygonExtrusion` for an Extrude and
  `FullTurnRevolution` for a Revolve. UI and CLI copy no arithmetic.
* **Unchanged route.** `EditSketchRequest`, `edit_sketch_copy`,
  `edit_object_copy`, `Document::write_sketch_geometry`, the snapshot
  copier, the cold rebuild before publication, the check that every saved
  reference resolves, and the source/version/alias/no-clobber guards,
  cancellation, cleanup, SQLite close and atomic Keep publish are all
  reused unchanged. There is no second job and no second CLI command.

### CLI and JSON

* **Command.** `edit-sketch-copy` with request v1 is unchanged: all ordered
  `curve_id`/`start_mm`, `deny_unknown_fields`, ≤ 65536 bytes. The response
  operation, schema and exit codes 0/2/7 are unchanged.
* **Discovery.** `inspect --json` reads the same pinned snapshot and
  `content_version`. For a §27A document:
  - `sketches[].vertices` is now filled, and `editable` is true unless the
    document or structure refuses;
  - on every older model, every existing field keeps its value and type;
  - the Revolve stays out of `features`;
  - `revolves[].profile.available` keeps its meaning: the profile can be
    read, not written.
* **New field.** `sketches[].profile_feature` is additive. It is null when
  `vertices` is null, and otherwise one of:

  ```json
  {"kind": "blind_extrude", "feature_id": "…", "height_mm": 10.0}
  {"kind": "full_turn_revolve", "feature_id": "…", "body_id": "…",
   "axis": "sketch_y", "extent": "full_turn", "axis_clearance_mm": 1e-6}
  ```

  A client that knows the new field can tell which policy the job will
  apply. An old client ignores it.
* **Without a kernel.** Coordinates and IDs are available without OCCT or
  PlaneGCS. The edit itself needs OCCT.

### UI

**Edit Sketch** on a Revolve part opens the existing coordinate editor:
* selection, drag, exact coordinates, Snap and Undo/Redo work as for an
  Extrude;
* the header states the saved full turn about the sketch Y axis, and that
  X is the radius;
* the canvas draws the labelled axis;
* there is no Blind height field and no Extrude/Revolve switch.

Save Cancel, a worker refusal and a failed async Open keep the draft being
edited. Publication goes through the existing worker and Open path.

### Not in this slice

* adding, removing or reordering Lines;
* constraints and Circle/Arc profiles;
* editing the axis or the angle, and partial turns;
* touching the axis;
* Boolean/Cut on the Revolve, and face attachment;
* in-place Save and live preview.
