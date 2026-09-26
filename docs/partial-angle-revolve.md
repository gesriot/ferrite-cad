# §27D — partial Revolve: a sector of a turned part

[Executed verification and limitations](partial-angle-revolve-verification.md).

A person or an agent turns the same simple closed Line profile as §27A/§27C
(a part with a bore, or a solid part closed on the axis) through a stated
angle instead of a full turn, and gets a new `.fcad` whose Body is that
sector: a quarter of a bushing at 90°, half a cylinder at 180°, three
quarters of a cone at 270°. The sector is real OCCT geometry with two
planar end faces, each named. It is not a full body cut or hidden after the
fact.

Full-turn documents, their payloads, JSON, cache keys and editing are
unchanged.

## Contract recorded before implementation

### Measured first (OCCT 8.0.1, pinned)

A C++ probe against the installed kernel turned three profiles about the Y
axis through the origin with `BRepPrimAPI_MakeRevol(face, axis, θ)`:
the cylinder [(0,0),(10,0),(10,15),(0,15)] (solid, axis-closed), the bushing
[(4,0),(10,0),(10,15),(4,15)] (with a bore) and the cone
[(0,0),(10,0),(0,15)] (solid, axis-closed), at θ = 90°, 180°, 270° and
137.5°, both windings. Every result is one valid solid whose volume equals
the full-turn volume × θ/360 to a relative 4e-16.

| Profile | Faces of the sector | From the Lines | Caps |
|---|---|---|---|
| Cylinder | 5: Plane, Cylinder, Plane, Plane, Plane | 3 (the axis Line raises nothing) | 2 |
| Bushing | 6: Plane, Cylinder, Plane, Cylinder, Plane, Plane | 4 | 2 |
| Cone | 4: Plane, Cone, Plane, Plane | 2 (the axis Line raises nothing) | 2 |

What the history reported:
* **Every non-axis Line raises exactly one face.** For a partial angle
  `MakeRevol::Generated(edge)` and `BRepSweep_Revol::Shape(edge)` agree on
  it, radial Lines included. (For a full turn `Generated` is empty for
  radial Lines; see §27A. The bridge keeps reading `Shape(edge)` in both
  cases.)
* **The axis Line still raises nothing.** `Shape(edge)` is null,
  `Generated` is empty and `IsDeleted` is false, as for a full turn.
* **Two caps with their own history.** `BRepSweep_Revol::FirstShape(face)`
  and `LastShape(face)` of the profile face are each one face of the
  finished solid. They are distinct from each other and from every Line's
  face, and together with the Line faces they cover the solid exactly. The
  first cap is not the profile face object itself (a copy, `IsSame` false),
  so it is taken from the sweep's history, never matched by position.
* **Orientation is read from the solid.** The cap faces in the solid carry
  their own orientation: the end cap's instance inside the solid is REVERSED
  relative to `LastShape`. Outward normals are measured on the solid's
  instances.
* **A full turn is different topology.** At exactly 360° `FirstShape` and
  `LastShape` are the same face and are not faces of the solid. The full
  turn keeps its own extent; a partial angle is never widened to it.

### Angle, unit, direction

* **Unit and form.** The angle is in **degrees**, a finite number, stored
  exactly as given. Nothing is rounded, reduced modulo 360 or converted to
  another operation. The kernel converts it once, at the bridge, as
  `θ_rad = degrees × (π / 180)`.
* **Direction.** A right-handed turn about the sketch's +Y axis through its
  origin, starting at the profile itself. With the sketch on the untransformed
  XY datum, a profile point (x, y) sweeps through
  (x·cos φ, y, −x·sin φ) for φ from 0 to θ. A 90° sector of a profile with
  x > 0 therefore occupies x ≥ 0, z ≤ 0.
* **Caps.**
  - **Start cap:** the profile's own region at φ = 0, on the sketch plane
    z = 0, outward normal (0, 0, +1).
  - **End cap:** the profile's region turned to φ = θ, outward normal
    (−sin θ, 0, −cos θ).
* **Accepted range.** 0.01 ≤ θ ≤ 359.99 degrees, both ends inclusive.
* **Refused, each with a message naming the value:**
  - 0, −0 and any negative angle (no mirrored sweep);
  - 0 < θ < 0.01 (too thin to be a sector this build keeps);
  - 359.99 < θ < 360 (within 0.01° of a full turn);
  - exactly 360: refused with "state a full turn instead". It is not
    rewritten to the full-turn extent: the two have different faces;
  - θ > 360: several turns are not a solid;
  - NaN and ±∞.

### Why these numeric limits

They are chosen by kernel and precision checks, separately from the full
turn:
* **Kernel.** OCCT built one valid solid with the exact volume at every angle
  probed, down to 1e-6° and up to 359.999999°, including a ring at a radius
  of 1e6 mm. The kernel is not the limit.
* **Mesh precision.** The mesh stores single-precision positions (§27C).
  At 1e-6° a 10 mm bushing loses three whole faces to float collapse, and a
  cone loses one; export would then refuse. At 0.01° every face of the
  10 mm bushing and cone, and of a 1e6 mm ring, keeps its triangles, and so
  does the full-size gap left by 359.99°.
* **Angular precision.** 0.01° is 1.745e-4 rad, eight orders of magnitude
  above OCCT's `Precision::Angular()` (1e-12), at both ends of the range.
* **Still guarded.** A sector can still be too thin to mesh near the axis
  clearance: a profile at x = 2e-6 mm turned 0.01° kept a valid B-Rep but
  lost one face's triangles. Such an export is refused atomically by the
  §27C rule ("a whole face loses its triangles"). A valid B-Rep alone still
  does not guarantee an exportable mesh.

### Profiles

Exactly the §27A/§27C policy (`FullTurnRevolution`), shared unchanged:
* a part with a bore, every vertex beyond the clearance; or
* a solid part closed on the axis along one whole Line.

Either winding, fractional sizes, any Y translation and any position of the
axis Line in saved order are accepted. The profile policy and the angle
policy are separate values; a request needs both.

### Storage and capability

* **Extent.** `RevolveExtent` gains `Partial { degrees }`. The degrees are
  a validated `RevolveAngle`, checked again when a payload is decoded.
* **The layout moves with the meaning**, one payload version per
  combination, decided by what the Revolve holds:

  | Payload | Extent | Axis Line | Required capabilities |
  |---|---|---|---|
  | v1 | full turn | none | core, `feature.revolve.v1` |
  | v2 | full turn | stated | + `feature.revolve.axis-closed.v1` |
  | v3 | partial | none | core, `feature.revolve.v1`, **`feature.revolve.partial.v1`** |
  | v4 | partial | stated | core, `feature.revolve.v1`, `feature.revolve.axis-closed.v1`, **`feature.revolve.partial.v1`** |

* **Full turns unchanged.** v1/v2 payloads stay byte-identical, with the
  same capabilities, references and cache keys.
* **Old builds.** A §27C build reads Revolve payloads v1–v2 only. It keeps a
  v3/v4 object verbatim and opens the document read-only, because it does
  not implement the new capability. It cannot read a sector as a full turn.
  This is checked with the real §27C binary, not emulated.
* **No SQL schema change.**
* **Payload and header must agree.** A header version that disagrees with
  what the payload holds is refused, as for v1/v2.

### Topology outputs

* **Faces of revolution.** One `RevolveFace { profile_segment }` per
  non-axis Line, unchanged.
* **Caps.** New role `RevolveCap { side: start | end }`.
  - **Owner:** the Revolve, as producer and owner of the reference.
  - **Provenance:** the sweep's `FirstShape`/`LastShape` of the profile
    face, checked in the bridge. Each cap must be one face of the finished
    solid, the two must differ, and neither may also be a Line's face.
    Every face of the solid must be claimed once.
  - It is not `ExtrudeCap`: an Extrude cap reference never resolves against
    a Revolve, and a Revolve cap reference never against an Extrude.
  - It requires `feature.revolve.v1` and `feature.revolve.partial.v1`.
* **What is not named.** The axis Line still raises nothing and gets no
  name. There is no seam, axis face, apex, cap edge or vertex name, no face
  index, and no fallback to the first face of any kind.
* **A full turn names no caps.** A cap reported for a full turn is refused.
* **Creation** writes one `RevolveFace` reference per non-axis Line and the
  two `RevolveCap` references. Publication requires every one to resolve
  after the cold build.
* **Cache.** The archive stores the caps under new `RevolvedCap` names (new
  tags; an older cache reader refuses the unknown tag and rebuilds). The
  evaluator's key adds the angle only for a partial turn, so full-turn keys
  do not move.

### Kernel

* **Request.** `RevolveTurn` gains `Partial { degrees }`. `RevolveResult`
  gains `start_cap` and `end_cap`, which must be empty for a full turn and
  exactly one face each for a partial turn.
* **Bridge.**
  - `fc_occt_revolve` is unchanged, including its `full_turn` flag.
  - A new entry point, `fc_occt_revolve_partial`, takes `double
    angle_degrees` in its own place. It refuses a non-finite value or one
    outside the open interval (0, 360); the 0.01° policy is the caller's.
  - `fc_occt_revolve_caps(shape, side)` answers the caps with the same
    two-call protocol as the face query. It is refused for a full turn or a
    decoded shape.
* **Doubles.** The mock kernel still does not build revolutions, and the
  unavailable kernel still refuses them.

### CLI

`create-sketch-revolve` accepts two request versions:
* **Request v1** keeps exactly its bytes and meaning: `"angle":"full_turn"`
  is required and is the only value.
* **Request v2**, strict (`deny_unknown_fields` at every level):

  ```json
  {"request_version":2,"points_mm":[[4,0],[10,0],[10,15],[4,15]],
   "axis":"sketch_y","extent":{"kind":"angle","degrees":90}}
  {"request_version":2,"points_mm":[[0,0],[10,0],[10,15],[0,15]],
   "axis":"sketch_y","extent":{"kind":"full_turn"}}
  ```

  - A v2 full turn makes the same document class a v1 request does.
  - v2 has no `angle` field. A v2 request carrying v1's
    `"angle":"full_turn"`, or no `extent`, is refused.
  - Any other version is refused as unsupported.

Envelopes, operation names, the result object and exit codes (0 published,
2 refused, 7 report delivery failed) are unchanged.

### JSON discovery (additive)

* **`revolves[]`**
  - `extent` keeps its type (a string). It is `"partial_turn"` for a sector;
    a client that knows only `"full_turn"` stops at the new value.
  - **`angle_deg`**, new: the stored degrees for a sector, null for a full
    turn.
* **`sketches[]`** of a sector's profile:
  - `editable` is false, and `refusal` says coordinate editing of a partial
    Revolve is not supported in this build.
  - `vertices` and `profile_feature` are null.
  - No field of the full-turn kinds changes.
* **`print-topology`** names the caps as `revolve start cap` and
  `revolve end cap`.

### Editing

* **Full turns.** Coordinate editing (§27B/§27C) is unchanged.
* **A saved sector is not editable in this slice.**
  - `edit-sketch-copy` and Edit Sketch refuse atomically: exit 2, nothing
    written, the source unchanged.
  - The writer's own re-derivation refuses too, so the refusal does not
    depend on the discovery layer alone.
  - Angle editing is a separate slice.

### UI

The same sketch window and Line editor. With **Feature: Revolve** selected,
a **Turn** choice appears:
* **Full turn (360°):** the §27A/§27C behaviour.
* **Partial angle**, with a degrees field. An invalid angle (empty, text,
  0, 360, out of range) shows the domain's refusal in place of the Create
  button, and the draft is kept.

The choice and the angle are part of the draft: Undo/Redo step through them.
Publication goes through the same worker, create job and Open path as every
other creation.

### Not in this slice

Angle editing and coordinate editing of a saved sector; other axes; a
negative (mirrored) sweep; Arc/Circle profiles; booleans on a Revolve;
several bodies; live preview; in-place Save.
