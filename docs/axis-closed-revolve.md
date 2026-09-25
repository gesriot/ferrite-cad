# §27C — solid full-turn Revolve: a profile closed on the axis

[Executed verification and limitations](axis-closed-revolve-verification.md).

A person creates an ordinary solid cylinder, cone or stepped shaft in the
same sketch window, or through the existing `create-sketch-revolve`. The
profile's saved coordinates are then edited through the existing Edit Sketch
/ `edit-sketch-copy`, with the same guards.

Until now (§27A/B) every vertex needed X > 1e-6 mm, so only parts with a
bore could be made. This slice widens exactly that limit. It adds no new
family of primitives, no builder of its own, and no fictitious small bore.

## Contract recorded before implementation

### Measured first (OCCT 8.0.1, pinned)

A C++ probe against the installed kernel revolved three profiles once about
the Y axis through the origin: the cylinder [(0,0),(10,0),(10,15),(0,15)],
the cone [(0,0),(10,0),(0,15)] and the stepped shaft
[(0,0),(10,0),(10,5),(6,5),(6,15),(0,15)]. Each came out as one valid solid:

| Profile | Volume | Faces |
|---|---|---|
| Cylinder | exactly 1500π | Plane, Cylinder, Plane |
| Cone | exactly 500π | Plane, Cone (no face at the apex) |
| Stepped shaft | exactly 860π | Plane, Cylinder, Plane, Cylinder, Plane |

What the history reported:
* **The Line on the axis raises nothing.** `MakeRevol::Generated(edge)` is
  empty, `IsDeleted` is false, and `BRepSweep_Revol::Shape(edge)` is null.
* **Every other Line raises exactly one face of the solid**, radial Lines
  included (unlike the hollow case, where `Generated` is empty for them).
* **Full coverage.** The faces of the non-axis Lines cover the solid exactly.

### The accepted classes

`FullTurnRevolution` keeps one policy with two named classes. The shared
simple-polygon rules — finite values, |coordinate| ≤ 1e6 mm, 3..256
vertices, closure, no self-touching, nonzero area — apply to both.

* **`RadialClear`, unchanged from §27A.** Every vertex has
  X > `AXIS_CLEARANCE_MM` (1e-6 mm). This is a part with a bore.
* **`AxisClosed`, new.** Every vertex has X ≥ 0, and exactly two vertices
  have X exactly 0. Those two must be the ends of one Line: the **axis
  Line**. Every other vertex has X > `AXIS_CLEARANCE_MM`.
  - The axis Line has nonzero length, because the shared rules forbid
    repeated vertices.
  - Any other Line touches the axis at most at one of its own ends.
  - Either winding, fractional sizes, any Y translation, and any position of
    the axis Line in saved order are accepted.

**Exactly zero.** "On the axis" means the coordinate is exactly 0. IEEE
−0.0 is the same number: it is accepted and stored as +0.0, so a request
saying −0 and one saying 0 make the same document. There is no snapping,
no `abs`, and no tolerance that turns a small positive X into 0. A vertex
with 0 < X ≤ 1e-6 mm is refused as near the axis but not on it.

**Refused, with a message naming the vertex or Line:**
* X < 0 (crossing the axis);
* one isolated vertex on the axis;
* two vertices on the axis that are not the ends of one Line (two touches);
* more than two vertices on the axis (several axis Lines or intervals);
* an off-axis vertex within the clearance.

### Storage and capability

* **The axis Line is stated.** An axis-closed Revolve stores it by curve
  UUID in its own payload, as `axis_segment`. The layout moves with the
  meaning, exactly as for `previous` and ThroughAll:
  - a Revolve that names one is **payload v2**;
  - it requires the new capability **`feature.revolve.axis-closed.v1`** as
    well as `feature.revolve.v1`.
* **Hollow Revolves unchanged.** They stay payload v1, with byte-identical
  payloads, the same capabilities, refs and cache keys.
* **Old builds.** A §27B build does not list Revolve v2 as readable. It
  keeps the object verbatim and opens the document read-only. It cannot
  mistake the axis Line for a Line that failed to raise a face. This is
  checked with the real §27B binary, not emulated.
* **Two checks, both must pass.** The stored UUID is checked against the
  class the policy derives from the saved coordinates, every time the
  document is built, discovered, prepared or written. A payload naming a
  Line that is not the derived axis Line, or a hollow payload whose profile
  touches the axis, is refused. Neither the data nor the stored claim is
  trusted alone.

### The axis Line raises no face

The class is decided once, in the document policy. Every later layer checks
it and never infers it:
* **Kernel request.** `RevolveRequest` carries `axis_segment:
  Option<label>`, taken from the checked class.
* **Bridge.** `fc_occt_revolve` gains an explicit `axis_segment` index
  (`FC_OCCT_NO_AXIS_SEGMENT` for none). It requires exactly that Line's two
  vertices to lie on the axis and every other vertex to be off it. It
  requires the sweep to make no face of that Line and exactly one face of
  every other Line, with every face of the solid claimed once.
* **Topology.** `record_revolve` takes the same `Option<label>`. The named
  axis Line must raise nothing. Any other Line that raises nothing is still
  a refusal, exactly as before; the empty-history check is not weakened for
  anyone else.
* **Names.** Creation writes one `RevolveFace` reference per non-axis Line.
  The axis Line gets no reference, no seam or vertex name, and no
  placeholder. A cone's apex is not a face and gets no name.
* **Cache.** The archive stores the same `RevolvedFace` names, so a
  restored solid resolves them identically. The cache key includes the axis
  Line (fed only when present, so hollow keys stay the same).

### Editing coordinates

The same `edit-sketch-copy` and Edit Sketch, request v1 unchanged, and the
same job, writer and SQL copier.

* **Axis-closed profiles.** They stay axis-closed with the **same** axis
  Line. It must stay exactly on X = 0 and keep a nonzero length. Radii,
  lengths and Y position may change, and a cylinder may become a cone where
  the same Lines and names remain.
* **Hollow profiles** keep exactly the §27B rules.
* **No hollow ↔ solid transitions.** A transition would destroy or add
  faces, and needs its own reference policy. It is refused before copying,
  atomically, and the message says the part's axis closure cannot change.
  The same refusal is re-derived by the writer inside its transaction.

### CLI and JSON

* **Unchanged:** `create-sketch-revolve` and `edit-sketch-copy` keep their
  requests, envelopes, operations and exit codes. What changes is the
  geometry policy: requests with an axis Line that §27A/B refused are now
  accepted.
* **`sketches[].profile_feature`** gains a third kind for this class:

  ```json
  {"kind": "full_turn_revolve_axis_closed", "feature_id": "…", "body_id": "…",
   "axis": "sketch_y", "extent": "full_turn", "axis_curve_id": "…",
   "off_axis_clearance_mm": 1e-6}
  ```

  `full_turn_revolve` keeps meaning a profile strictly off the axis, with
  its `axis_clearance_mm`. A client that knows only the old kinds stops at
  the new one; it never reads it as the old contract.
* **`revolves[]`** gains two additive fields: `closure`
  (`"radial_clear"` or `"axis_closed"`) and `axis_curve_id` (null or the
  UUID).

### UI

The same Line editor, with precise coordinates, drag, Snap and Undo/Redo.
* **What the window says.** The canvas labels the axis, and the text
  explains both options: keep every point off the axis for a part with a
  bore, or put exactly one whole edge on X = 0 for a solid part. It no
  longer says unconditionally that every point needs X > 0.
* **Editing.** There is no feature, axis or angle switch. Moving an
  axis-Line vertex off the axis is refused with the reason, and the draft is
  kept.

### Not in this slice

* hollow ↔ solid transitions;
* partial turns and angle editing;
* other axes;
* Circle/Arc, constraints, several profiles or axis intervals;
* Boolean/Cut on the Revolve;
* in-place Save, live preview and new primitive builders.
