# §28L — all four vertical corners of a rectangular plate, in a new copy

[Executed verification and limitations](rectangular-fillet-history-verification.md).
Builds on [§28G](sequential-edge-fillets.md) (a second Fillet),
[§28H](edit-sequential-fillet-radii.md) (radii),
[§28I](edit-two-fillet-base-height.md) (height),
[§28J](edit-two-fillet-base-sketch.md) (coordinates) and
[§28K](edit-two-fillet-base-constraints.md) (constraints).

A person rounds a **third and a fourth** different vertical corner of the same
plate, one at a time, with the existing **Fillet edge of …** form and
`fillet-edge-copy`. The existing editors of a radius, of the plate's height, of
the base Sketch's coordinates and of its Line constraints keep working on the
whole supported history. The history is Extrude → F1 → … → Fn → Body tip, with
`n` from 0 to 4. Every Fillet is a real Open CASCADE fillet of the result of
its predecessor; nothing is replaced by one operation over a rebuilt profile.
No command, request version, copier, payload, capability, archive or SQLite
schema is added: Fillet 3 and Fillet 4 mean what Fillet 2 already means.

## Contract recorded before implementation

### The supported class (unchanged)

One untransformed XY datum; one Sketch of four Lines whose stored coordinates
are an axis-aligned rectangle, unconstrained or carrying the constraint
editor's managed Line family; one forward literal Blind `Extrude`/`NewBody`;
`n ≤ 4` Fillets in one unbranched chain; no Cut. **One Fillet per joint** of
the base Lines' UUIDs, so at most four. Refused with kind and reason: a joint
rounded twice, a fifth Fillet, a branch (two Fillets with one `previous`), a
Fillet whose `previous` is neither the Extrude nor a Fillet of the chain, a
foreign producer, a non-rectangular or ambiguous profile, and everything the
earlier slices refuse. Arbitrary edges, Chamfer, Cut with Fillet, new
constraint kinds, in-place Save and live preview are out of scope; the 5C
milestone stays open.

### One reader

`fillet_radius::fillets_over_plate` is the only reader of the Fillets of a
plate and is bounded by `MAX_PLATE_FILLETS = 4`. It returns the Fillets in
history order as typed `SavedFillet`s, each with its place, its corner on the
stored Lines, its radius and the **list of every other Fillet** of the history
(`neighbours`), each with the Line it shares with this one, or none for the
opposite corner. It reads the real predecessor chain (from the Body's tip down
to the base Extrude), proves that every Fillet belongs to the one Body and
that each edge's producer is the base Extrude, and checks the exact object and
dependency sets (`cut_edit::read_history`, already general in the chain) and
the exact names by meaning: 7 per Fillet and one `OriginFilletFace` per
earlier Fillet, so `7k + k(k−1)/2` in all (7, 15, 24, 34).

`fillet_chain` is its pure structural half, which `evaluable_fillet` uses too:
the chain below a Fillet, walked by `previous`, bounded, cycle-safe, each
member a Fillet that rounds an edge of the base Extrude, joints distinct. There
is no `second`/`third`/`fourth` field and no check by the number of objects.
The older one-Fillet and two-Fillet shapes are projections of this list (see
*Wire*), never a second source of facts.

### Radii and gaps (policy unchanged, applied to every neighbour)

Per corner: `MIN_RADIUS_MM ≤ r ≤ MAX_RADIUS_FRACTION × shorter side`. Per pair
of **geometrically adjacent** corners (they share a Line, whatever their order
in the history): in history order, the later radius `r_b ≤ L − r_a − MIN_RADIUS_MM`
(`check_pair`). Opposite corners share no Line and add nothing. The fourth
corner closes the perimeter, so its two neighbours are both checked.

* The largest radius offered for a Fillet is the **minimum over every bound that
  applies**: its corner bound, `pair_bound(L, r_earlier)` for each earlier
  neighbour and `pair_bound_of_first(L, r_later)` (the bounded bisection of #72)
  for each later one. It is accepted by `check_radius`, and the next
  representable value above it is refused. Expressions are not reordered.
* Refusals name the guilty Fillet, joint and shared Line UUIDs.
* A change of any radius is judged against every other saved radius, earlier and
  later, so the check on the whole history is the one a rebuild applies.

### Constrained Sketch

Stored coordinates are the solver's starting guess and prove nothing. At every
cold and warm rebuild `evaluable_fillet` judges, on the **solved** Lines of the
same four UUIDs, the sides, every corner, every radius and every shared side of
the Fillet being built against all earlier ones. There is no second solved
check beside the evaluator. Free and closure-only plates may be edited with
Edit Sketch; the others with Edit constraints; removing the user's constraints
leaves the closure and returns the coordinate editor.

### Names

Fillet `k` persists the seven names of §28A (its `EdgeFilletFace`, two
`OriginCap`, four `OriginSide`) and one `OriginFilletFace { origin_feature: F_j,
edge_feature: base, joint: j_j }` for **each** earlier Fillet. The edge a Fillet
rounds is found on its predecessor's result by `(base, SweepEdge(joint))`,
exactly one, else a typed refusal; no OCCT index, no nearest edge, no
coordinates. `record_fillet` carries every name of its predecessor — caps,
sides, sweep edges and the earlier Fillets' faces under their own origin — so
the cylinders of F1 and F2 keep their names through F3 and F4, and a restored
cache carries them equally. Archive v4 and `feature.fillet.sequential.v1`
already express all of it.

### Storage and the old reader

Fillet payload v2 and capability `feature.fillet.sequential.v1`, both written
for Fillet 2 and now for Fillet 3 and 4 unchanged: the stored meaning is "a
Fillet whose `edge.feature` is not its `previous`". The reader of `main`
(§28K) opens the document, reads every row, reports the Fillet editors
unavailable (it holds a different count of Fillets than it supports) and
**refuses** the rebuild of three or four Fillets with a typed error: it never
publishes a partial Body. This is exercised with the binary of `main` on a new
document.

### SQL allowlists (every cell outside them byte-identical)

* **fillet-edge-copy, Fillet 3 or 4:** `objects`: one new Fillet row (v2) and the
  Body row's `payload`/`payload_hash` (the tip). `deps`: `+ F_k → F_{k−1}`
  (Predecessor), `+ Body → F_k` (BodyTip), `− Body → F_{k−1}` (BodyTip).
  `topology_refs`: `7 + (k−1)` new rows (9, 10). `capabilities`: none new
  (`feature.fillet.sequential.v1` is already declared). `meta.modified_at`.
* **edit-fillet-radius:** the selected Fillet row's `payload` and `payload_hash`;
  `meta.modified_at`.
* **edit-extrude (base height):** the Extrude row's `payload` and `payload_hash`;
  `meta.modified_at`.
* **edit-sketch-copy:** the base Sketch row's `payload` and `payload_hash`;
  `meta.modified_at`.
* **edit-sketch-constraints-copy:** the base Sketch row's `schema_version`,
  `payload`, `payload_hash`, the one `sketch.constraints.v1` capability row
  by the existing contract; `meta.modified_at`. Constraint UUIDs as §28E.

Every writer re-derives its preparation from the document **inside its
transaction** and compares the whole list of Fillets; the version guard,
baseline rebuild, strict references, aliases, no-clobber, cancellation, cleanup
and exit 7 are the shared copy job's, unchanged.

### Wire (JSON v1, additive; no scalar changes meaning)

* `history_index` of a Fillet stays the position of that Fillet; its range is
  now 1…4.
* New `fillets[].radius_edit.neighbours`: every other Fillet of the history, in
  history order, each with `feature_id`, `history_index`, `edge`,
  `stored_corner_mm`, `radius_mm`, `shared_line_id`, `stored_shared_length_mm`
  (`null`/`null` for the opposite corner).
* New `fillet_history` beside `fillet_base` (on a base Extrude row and on the
  base Sketch row, and in the constraint editor's context): the whole history,
  `{ count, fillets: [ { fillet_feature_id, previous_feature_id, history_index,
  edge, radius_mm, corner_mm } ] }`, for one to four Fillets.
* The old shapes are projections for their supported class only:
  `radius_edit.neighbour` is non-null **only in a history of exactly two
  Fillets** (the other one) and `null` otherwise; `fillet_base` with
  `second_fillet` describes histories of one or two Fillets and is `null` for
  three or four, which `fillet_history` describes. A scalar never becomes an
  array or `null`.
* `bodies[].fillet_edge` lists only unrounded corners; a plate with four
  rounded corners gives a typed refusal.

### UI

The existing forms and workers. The candidates are the corners still sharp.
The history line and the radius editor name the selected Fillet and every
neighbour present; the form's height is bounded and scrolls, so Save and Cancel
stay reachable with four Fillets. Whole-request Undo/Redo, Cancel and a worker
refusal keep the draft; a successful copy opens asynchronously.

## Out of scope

Chamfer, Cut with Fillet, arbitrary edges, a Fillet of a Fillet's edge, a
profile other than the rectangle, new constraint kinds, in-place Save, live
preview, a window run from the cloud.
