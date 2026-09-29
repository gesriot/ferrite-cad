# §28G — a second Fillet on another vertical corner, in a new copy

[Executed verification and limitations](sequential-edge-fillets-verification.md).
[The first Fillet](single-edge-fillet.md);
[on a dimensioned plate](fillet-constrained-plate.md) and
[dimensioned afterwards](fillet-base-constraints.md).

A person opens a plate that already carries one Fillet (§28A, on an
unconstrained plate or one dimensioned with the constraint editor's Line
constraints) and rounds a **different** vertical corner with its own radius,
into a new copy. The history becomes Extrude → Fillet 1 → Fillet 2 → Body
tip: two real features, each built by its own kernel operation on its own
predecessor's result, never one hidden operation that rounds both. The
existing **Fillet edge of …** form and `fillet-edge-copy` are the only
entry points; no command, request version or copy pipeline is added.

## Contract recorded before implementation

### The supported source

The §28B frame (`fillet_radius::fillet_over_plate`): one untransformed XY
datum; one Sketch of four Lines whose stored coordinates are an axis-aligned
rectangle, unconstrained or carrying the constraint editor's managed Line
family; one forward literal Blind `Extrude`/`NewBody`; exactly one Fillet,
whose `previous` and `edge.feature` are that Extrude, which is the Body's tip;
no Cut; exactly the plane, profile, predecessor and body-tip dependencies and
the seven §28A names.

The new Fillet rounds one of the **three other** corners of the same plate,
named as before by the base Extrude and the unordered pair of Line UUIDs that
meet there. Refused, typed, before anything is written: the corner the first
Fillet rounded (`input`), a document that already holds two Fillets
(`unsupported`: a third Fillet is out of scope), and everything §28A refuses.

### The stored model

* **Fillet 2** is a Fillet row with `previous` = Fillet 1 (the result it
  actually rounds) and `edge` = { `feature`: the base Extrude, `joint`: the
  Line pair } — the stable meaning of the corner. The producer is not
  rewritten to Fillet 1 to keep the old `edge.feature == previous` equality.
* That is new stored meaning, so it moves the layout: a Fillet whose
  `previous` is another Fillet is **payload v2** and requires the new
  capability **`feature.fillet.sequential.v1`**. A §28A Fillet stays v1. A
  build that predates this one does not read Fillet v2, keeps the row verbatim
  and opens the document read-only; its rebuild refuses the object rather than
  building part of the history. No SQLite schema change.
* The validator adds one rule: a Fillet's `edge.feature` must be its
  `previous` or an ancestor of it through the predecessor chain
  (`fillet.edge-outside-history`).

### Names

Fillet 2 persists eight references (all `Exact` except the four sides, which
keep §28A's `AllDerivedFrom`):

* `EdgeFilletFace { edge_feature: base, joint: j2 }` — its own cylinder;
* `OriginCap { base, Start|End }`, `OriginSide { base, segment }` × 4 — the
  plate's faces as the final Body has them;
* **new** `OriginFilletFace { origin_feature: Fillet 1, edge_feature: base,
  joint: j1 }` — Fillet 1's cylinder as the final Body has it. Its own role:
  Fillet 1's own `EdgeFilletFace` reference keeps naming Fillet 1's result,
  and this one names the same face in the Body. It requires
  `feature.fillet.sequential.v1`.

Fillet 1's seven names are untouched and keep resolving on Fillet 1's result.

### Topology

`TopologyMap::record_fillet` carries, from the predecessor's names, through
the fillet's own history:

* every face it carried before (caps, sides and their origins), as now;
* **every vertical sweep edge** of the base, as an origin name
  `(base, SweepEdge(joint))`; the rounded one is recorded as removed;
* **the predecessor's own fillet face**, as `(Fillet 1, FilletFace(base, j1))`.

Fillet 2's kernel edge is the edge `(base, SweepEdge(j2))` names on Fillet
1's result — exactly one, found by history. Missing, removed (already rounded),
ambiguous or belonging to another shape is a typed `topology` refusal before
anything is published. No OCCT index, nearest edge or coordinate match is
used, and a handle of the source shape is never taken for one of the result.

Measured on Open CASCADE 8.0.1: `BRepFilletAPI_MakeFillet::IsDeleted` answers
`true` for edges and vertices the result still contains unchanged (7 of 8
edges and 6 of 8 vertices of a box after rounding one vertical edge), and
`false` for kept faces. The bridge's history therefore classifies by
`Modified` first, then membership of the result, and only then as deleted;
the answer for every face is unchanged. It also refuses a reported output
whose shape type differs from the input's.

### Cache

The named archive gains `OriginSweepEdge { origin_feature, joint }` (an edge)
and `OriginFilletFace { origin_feature, edge_feature, joint }` (a face), so a
Fillet restored from the cache carries the same edges and faces a cold build
does, and Fillet 2 built on a restored Fillet 1 picks the same edge. The
archive format moves from v3 to **v4**; a v3 entry is refused and rebuilt, not
read with the new meaning. Fillet 2's key already includes Fillet 1's, which
includes the Extrude's and the Sketch's: changing the Sketch or the first
radius invalidates the suffix. No extra solve or rebuild is added.

### One validator for structure and solved geometry

`evaluable_fillet` checks Fillet 2 structurally (previous is the one Fillet
over the frame above, the same base, a different corner) and geometrically on
the Lines the base was **built** from — the one solve of the rebuild, cold or
cached (§28E/F): the same Line UUIDs in stored order, an axis-aligned
rectangle at the unchanged 1e-7 mm, every Line on its side, both joints
corners, each radius within §28A's bound (`0.01 mm ≤ r ≤ ½` the shorter side
at its corner), and, for **adjacent** corners, the new pair policy:

> the flat left on the Line the two corners share must be at least
> `MIN_RADIUS_MM` (0.01 mm): `L_shared − r1 − r2 ≥ 0.01 mm`.

Measured on OCCT 8.0.1 on the 37.5 × 12.25 × 6.75 mm plate, second corner
adjacent across the 12.25 mm side: touching (`r1 + r2 = L`, flat 0) is not
built at all; a flat of 1e-7, 1e-5, 1e-4, 1e-3, 5e-3, 0.01 and 0.025 mm builds
a valid 8-face solid whose volume matches `(W·D − (1 − π/4)(r1² + r2²))·h` to
2e-16 relative, with Fillet 1's face kept. The chosen minimum is the smallest
feature this build rounds: a strip narrower than the smallest radius is
treated as touching. Nothing is clamped or snapped. Opposite corners share no
Line and need no further condition. Stored lengths of a constrained plate are
not solved bounds.

### Writer and allowlist

Unchanged pipeline (snapshot, version guard, read-only source, baseline
rebuild, every baseline and minted name resolving, cancellation, no-clobber
atomic publication, exit 7). The writer re-derives the whole preparation
inside its transaction. The SQL allowlist:

* **objects:** one new Fillet row (schema v2); the Body row's `payload` and
  `payload_hash` (the tip). The Sketch (constraints included), the Extrude and
  Fillet 1 rows are byte-identical.
* **deps:** + `Fillet 2 → Fillet 1` (Predecessor), + `Body → Fillet 2`
  (BodyTip), − `Body → Fillet 1` (BodyTip).
* **topology_refs:** Fillet 2's eight new rows; every existing row
  byte-identical.
* **capabilities:** + `feature.fillet.sequential.v1`; the others as they were.
* **meta.modified_at.**

### Discovery and UI (JSON v1, additive)

* `bodies[].fillet_edge` of a Body whose tip is one supported Fillet becomes
  `available`, structurally: `target.previous_feature_id` (the Fillet the new
  one rounds; for a plain plate the base, as before) and `target.fillets`
  (the existing Fillet: feature, edge, radius); `candidates` are the three
  other corners, each with `adjacent_fillet_feature_id` (null for the opposite
  corner). For an unconstrained plate `max_radius_mm` also answers the pair
  policy; for a constrained one it stays `null`. The rebuild decides finally.
* With two Fillets, `fillet_edge.available` is `false` with the reason; the
  radius, height, Sketch and constraint editors refuse the two-Fillet history
  honestly (their rows say so) and keep working for none or one Fillet.
* The form shows the history (Extrude → Fillet 1 at its corner, r → new
  Fillet), lists only the other corners, marks the adjacent ones with the
  shared-side rule, and refuses at Save what the rebuild refuses, keeping the
  draft.

### Compatibility

The reader on `main` before this change is measured on a copy with two
Fillets: it must refuse the unknown semantics (payload v2 / capability)
without a partial rebuild and without rewriting the file. Every document this
build wrote before still reads and rebuilds.

### Out of scope

A third Fillet, the same corner twice, touching or merged arcs, cap edges,
edge chains, Cut with Fillet, Chamfer, editing a history with two Fillets,
picking, live preview, in-place Save.
