# §28A — one named Fillet on one vertical edge, in a new copy

[Executed verification and limitations](single-edge-fillet-verification.md).
[History decision](decisions/0004-feature-predecessor.md).

A person or an agent opens a saved rectangular plate. They pick one of its
four vertical edges from a list, enter a radius, and publish a new `.fcad`.
The Body of the copy ends in a real, named Fillet feature. The copy reopens
and rebuilds cold or from the cache to the same rounded solid, and it exports
to STL and FBX.

This is a deliberately bounded first slice, not a general fillet system.

* **UI:** `Fillet edge of <Body> — <UUID>…` opens a form with a list of the
  four corners and a radius field.
* **CLI:**

  ```text
  ferritecad fillet-edge-copy <source.fcad> --body UUID --expect-version HASH \
      --request <request.json> -o <copy.fcad> [--json]
  ```

  The request is `{"request_version":1,"edge":{"feature_id":"…",
  "joint":["<curve UUID>","<curve UUID>"]},"radius_mm":2.5}`.

## Contract recorded before implementation

### The supported source

The class is exactly the §25K/§26A frame, with no Cuts:

* one untransformed XY datum;
* one unconstrained Sketch of four Lines forming an **axis-aligned
  rectangle** (`CutBoundary::rectangle_mm`);
* one forward literal Blind `Extrude`/`NewBody`;
* its one Body;
* exactly the plane, profile and body-tip dependencies.

The reader is the shared `saved_history` reader of the Cut editors, with zero
Cut links. Fractional sizes, translated coordinates, either winding and any
starting segment are accepted, because nothing below depends on them.

Each of these is refused with its reason:

* imported STEP;
* several Bodies;
* a placement;
* a formula height;
* a constrained profile;
* a non-rectangle, a Circle or an annulus;
* a Revolve;
* a Cut history;
* a Body that already ends in a Fillet (a second fillet is a follow-up).

### What is rounded

One **convex vertical edge**: the edge the base Extrude swept along +Z at one
corner of the rectangle. It is named by that producer and the existing
semantic `ExtrudeSweepEdge { joint }` — the unordered pair of the two
adjacent Line UUIDs. The radius is one constant, finite, positive number.

Nothing else is offered:

* no chains, no cap edges, no fillet-all;
* no chamfer, no shell;
* no preview, no mouse picking, no in-place Save;
* no editing of the new Fillet (a follow-up).

### The selection contract

The edge is chosen by its meaning, never by an index, a coordinate, the
nearest geometry, an invented curve UUID or a reference UUID that was never
recorded.

* **Discovery.** Discovery lists the four candidates of the saved plate as
  `{ feature_id, joint: [a, b] }`. The pair is the two real adjacent curve
  UUIDs, in the canonical order `ProfileJoint` stores. Each candidate also
  carries the corner's coordinates, its two adjacent Line lengths and its
  radius limit, as labels. They are not identity.
* **Request.** A request may give the pair in either order; `ProfileJoint`
  makes both one meaning.
* **Refusals.** A pair that is not two adjacent Lines of this profile is
  refused with its reason. So is a feature other than the base Extrude, and a
  meaning that names no single edge.
* **What is persisted.** The Fillet payload stores the typed meaning:
  `edge: { feature, joint }`. No input `TopologyRef` exists for the edge today,
  and none is invented. The new Fillet persists references only to what **it**
  produced (see below).
* **Evaluation.** The evaluator resolves exactly that meaning against the
  predecessor's output. It requires exactly one sweep edge under that joint;
  none or several is a refusal.

### Document model

`feature.fillet` is a new object kind, stored at payload v1:

```text
Fillet {
    previous: ObjectId,        // the feature whose result this rounds
    edge: { feature: ObjectId, joint: ProfileJoint },
    radius_mm: f64,            // finite, > 0
}
```

* **Capabilities.** It requires `core.part.v1`, `feature.predecessor.v1` and
  the new `feature.fillet.v1`.
* **Why a new capability.** A prior-main build does not know
  `feature.fillet`. It preserves the object verbatim and opens the document
  read-only, and its evaluator cannot build the Body's tip. That is a real
  prior-reader incompatibility, and it is why the capability exists; it is
  tested against a real prior-main binary.

History follows ADR 0004, unchanged:

* the Fillet names the feature it consumes (`previous`, with a `Predecessor`
  edge);
* the Body names its tip (`BodyTip` moves from the Extrude to the Fillet);
* ownership stays derived. There is no feature-to-Body edge, no second
  ownership table, and no copier of its own.

The validator's `feature.forked-history`, `body.shared-tip` and
`body.shared-history` rules, and its missing-edge and kind checks, cover the
Fillet's `previous` exactly as they cover an Extrude's. The SQLite schema does
not move.

### Kernel operation

`GeometryKernel::fillet_edge(request { target, edge, radius }, track, ctx)` is
a selected-edge operation, and not a wrapper of the experimental
`OcctKernel::fillet_all` corpus probe. The OCCT bridge gains
`fc_occt_fillet_edge` and `fc_occt_fillet_faces`:

* `BRepFilletAPI_MakeFillet` with exactly the one edge;
* session and shape-ownership checks, and a check that the edge is an EDGE
  sub-shape of the target;
* cancellation before and during the build;
* success requires all of:
  * `IsDone`;
  * exactly one solid;
  * `BRepCheck_Analyzer` valid;
  * a finite, positive removed volume;
  * at least one face `Generated` from the edge, lying in the result.
* the answers for every registered sub-shape of the target (kept, modified,
  deleted) are stored while the algorithm is alive, as for a Cut. They are
  read back through the existing `fc_occt_cut_carried`.

The Rust side validates the result again. The history must speak only about
the target; outputs must belong to the result; the rounded edge must be
reported deleted; exactly one fillet face must be generated from it; and the
removal must be positive. The C ABI change is covered by argument and layout
tests.

### Names

* **Existing references keep their producer.** A reference to the base
  Extrude's faces or edges keeps addressing the base's historical output. It
  is never repointed to the latest solid.
* **The Fillet's own references**, all owned and produced by the Fillet:
  * `EdgeFilletFace { edge_feature: base, joint }` — **new role** — the one
    cylindrical face rounded from that edge. `Face`, `Exact`.
  * `OriginCap { base, Start|End }` — the base caps as the Fillet leaves them
    (trimmed).
  * `OriginSide { base, segment }` for each of the four Lines — the two sides
    next to the corner are trimmed, the other two are kept.
* **Honest history.** Carried faces come only from the fillet history. A name
  the fillet removed is recorded as removed, never matched by geometry.
  Missing or ambiguous history refuses.
* **Edges.** The base Extrude's edge and vertex names are historical, at the
  base producer; its rounded sweep edge exists only there. The Fillet tip
  publishes no edge or vertex names in this slice.

The new role requires `feature.fillet.v1`; the origin roles keep
`topology.origin-face.v1`. The existing `FilletFace { source_edge }` role
keeps its meaning and is not used: one `StableEntityId` cannot name a producer
and a joint.

### Cache

The key is `eval.fillet.named`. It includes:

* the predecessor's identity and cache key (content);
* the edge producer and the canonical joint;
* the radius bits;
* the kernel and tolerance.

An upstream change changes the predecessor key and invalidates the fillet.

The archive gains one tag (`TAG_EDGE_FILLET_FACE`), the vocabulary-only route
earlier tags took. An older build refuses the entry as malformed and rebuilds.
The archive format version stays 3, because the layout is unchanged; older
v1/v2 entries stay explicitly invalidated. Cold, Miss and Hit give the same
Body, names and refusals.

### Radius policy (measured, then chosen)

Measured on OCCT 8.0.1 with a 37.5 × 12.25 × 6.75 mm plate at (−4.5, 3.25).
The corner's shorter adjacent side is D = 12.25.

| r | OCCT |
| --- | --- |
| 12.25 (= D) and larger | `IsDone` false |
| 12.2 (0.996 D) | valid; removed volume = (1 − π/4)·r²·H to 3e-15 |
| 0.01 … 12.2 | valid; relative volume error ≤ 5e-10 |
| 0.001 | error 1.3e-7 |
| 1e-5 | error 1.4e-3 |
| 1e-7 | "valid", removes **0** mm³ |

The chosen class leaves wide margins on both sides:

    0.01 mm ≤ r ≤ min(len_a, len_b) / 2

`len_a` and `len_b` are the corner's two adjacent Lines.

* The upper bound keeps at least half of each adjacent side flat, far from the
  degenerate limit. It was not widened to make a bad example pass.
* The lower bound is 10⁵ × the kernel linear tolerance, and keeps volumes
  measurable.

Nothing is clamped. A radius outside the class is refused with the numbers, in
discovery validation, in preparation, in the writer's re-derivation and in the
evaluator. The fillet never changes another corner, and never returns the
unmodified block.

### Measured expectations

Removing one convex corner of a W × D × H block gives:

* volume `W·D·H − (1 − π/4)·r²·H`;
* 7 faces: two trimmed caps, four side planes and one cylindrical fillet face;
* a cylinder of radius r whose axis is parallel to Z and passes through
  `(cx ∓ r, cy ∓ r)`, inward from the chosen corner.

These are measured on the B-Rep (surface type, radius and axis location),
not only on a tessellation.

### Copy operation

One shared jobs route (`fillet_edge_copy`), through the existing
`edit_object_copy`, for UI and CLI alike:

* one snapshot and a version check;
* the source is read-only;
* a no-clobber output, and a refusal of aliasing;
* a baseline cold rebuild;
* the write;
* a strict rebuild in which every baseline reference and every minted
  reference must resolve;
* the version checked again;
* SQLite closed;
* atomic publication;
* cancellation boundaries throughout.

The writer re-derives the whole prepared value inside its transaction.

The SQL allowlist:

* **objects:** one new Fillet row; the Body row's payload and payload hash
  (the tip). Nothing else.
* **deps:** + `Fillet → base` (Predecessor), + `Body → Fillet` (BodyTip),
  − `Body → base` (BodyTip).
* **topology_refs:** the Fillet's seven new rows.
* **capabilities:** `feature.fillet.v1` and `topology.origin-face.v1` (and
  `feature.predecessor.v1`), added only if missing.
* **meta.modified_at.**

Every other cell is byte-identical.

### Clients

* **`inspect --json`**: an additive `fillets[]` per Body. Each entry has
  `available`, `refusal`, `base_feature_id` and `candidates[]`: `feature_id`,
  `joint`, `corner_mm`, `adjacent_lengths_mm` and `max_radius_mm`, plus
  `min_radius_mm`. It is structural only: it promises neither native
  libraries nor successful geometry. It is read from the existing pinned
  snapshot.
* **`fillet-edge-copy`**: JSON v1, strict. It refuses unknown, duplicate and
  escape-duplicate keys, and arrays where objects belong, with `input`.
  Exit 7 means published with the report lost; the file is never retried or
  rolled back.
* **UI**: the same typed candidates and request/job.
  * The form shows the chosen corner (both Line UUIDs, coordinates) and the
    radius.
  * The draft survives a cancelled Save, a worker refusal and a stale reply.
  * Publication goes through the existing async Open.
  * Work runs off the window thread; there is no perpetual redraw.

Existing editors refuse a Body that ends in a Fillet with a reason naming it:
extrude height, sketch coordinates and constraints, Cut add and edit, and the
Revolve editors.

### Compatibility

* Documents without a Fillet are unchanged, byte for byte.
* A prior-main reader opens a filleted copy read-only and refuses writes; its
  source file is untouched.
* Stub builds discover and refuse geometry with a typed reason. An OCCT build
  without PlaneGCS fillets normally: an unconstrained fillet has no solver
  dependency.
