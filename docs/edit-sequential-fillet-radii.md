# §28H — the radius of either of two sequential Fillets, in a new copy

[Executed verification and limitations](edit-sequential-fillet-radii-verification.md).
[Two sequential Fillets](sequential-edge-fillets.md);
[the one-Fillet radius edit](edit-fillet-radius.md).

A person opens a plate that §28G rounded twice — Extrude → Fillet 1 →
Fillet 2 → Body tip — and changes the radius of **either** Fillet, named by
its exact UUID, into a new copy. The existing `edit-fillet-radius` command and
**Edit Fillet radius** form are the only entry points: same request v1
(`{"request_version":1,"radius_mm":R}`), envelope, operation, exit codes and
publication guarantees, same shared copy job, writer and evaluator. No
command, request version, copier, solver or geometric route is added.

## Contract recorded before implementation

### The supported source

Exactly the §28G class: one untransformed XY datum, one Sketch whose stored
Lines are an axis-aligned rectangle (unconstrained, or carrying the constraint
editor's managed Line family), one forward literal Blind `Extrude`/`NewBody`,
and exactly two Fillets:

* **Fillet 1**: `previous` = `edge.feature` = the base Extrude (payload v1);
* **Fillet 2**: `previous` = Fillet 1, `edge.feature` = the base Extrude
  (payload v2), at another corner; the Body's tip.

No Cut, no other object; exactly the plane, profile, two predecessor and one
body-tip dependencies; Fillet 1's seven §28A names and Fillet 2's eight §28G
names, by meaning, and no other name owned by either. Anything else is
refused by name, as before.

The one-Fillet path is unchanged: a document with one Fillet goes through the
§28B frame exactly as it did, with the same wire. The guard that the base
height, Sketch and constraint editors apply (exactly one Fillet) is **not**
lifted: with two Fillets they still refuse, now saying that only the radius
edit is offered.

### What an edit is

Only `radius_mm` of the selected Fillet's payload. Both Fillets keep their
UUIDs, `previous`, edge meaning (base Extrude + two Line UUIDs), payload
version and names; the Body keeps its tip; nothing is minted, squashed,
retargeted or re-created.

### SQL allowlist

* `objects`: the selected Fillet row's `payload` and `payload_hash`;
* `meta.modified_at`.

Nothing else: not the Sketch, not the other Fillet, not `schema_version`,
dependencies, names or capabilities. The writer re-derives the whole
preparation inside its transaction against the document version it was
prepared from; a forged payload (another predecessor, corner, the other
Fillet's radius, another row) is refused.

### One validator; stored and solved

The shared rule is §28A's per-corner bound plus §28G's pair policy, applied
in **history order**: `check_pair(Fillet 1's corner, r1, Fillet 2's corner,
r2)`, i.e. `r2 ≤ L_shared − r1 − 0.01 mm` for adjacent corners, nothing for
opposite ones. An edit of either radius is judged by that same predicate with
the other radius as saved, so raising Fillet 1 is refused exactly when the
rebuild's check of Fillet 2 would refuse it.

* **Unconstrained plate:** preparation applies the whole rule to the stored
  Lines (which are the part). Discovery reports the selected Fillet's exact
  bound: its §28A bound and, beside an adjacent Fillet, the pair bound — for
  Fillet 2 `L − r1 − 0.01` (the check's own expression); for Fillet 1 the
  largest `r1` the same predicate accepts with the saved `r2`, found by
  stepping the float next to `L − r2 − 0.01`, so the offered maximum is
  accepted and the next float refused.
* **Constrained plate:** the stored Lines are the solver's starting guess.
  Preparation checks only the value part (finite, ≥ 0.01 mm); discovery shows
  stored numbers labelled as stored and `null` for every bound; the copy's
  rebuild — the one solve it already runs — judges both corners and the pair
  on the solved Lines before anything is published.

The rebuild checks the whole suffix: changing Fillet 1 is judged again at
Fillet 1 and at Fillet 2.

### Cache

Fillet 2's key already includes Fillet 1's. Editing Fillet 2: Extrude and
Fillet 1 Hit, Fillet 2 Miss (built on the restored Fillet 1). Editing Fillet
1: Extrude Hit, both Fillets Miss. No archive format change.

### Discovery and UI (JSON v1, additive)

`fillets[]` rows gain, for a two-Fillet history, `history_index` (1 or 2) and
`radius_edit.neighbour`: the other Fillet's UUID, index, corner, radius and,
when adjacent, the shared Line and its stored length. `radius_edit.available`
becomes `true` for both; `max_radius_mm` is the exact bound above for an
unconstrained plate and `null` for a constrained one. The result of
`edit-fillet-radius` adds `previous_feature_id` and names the selected Fillet.
The form states which Fillet it edits (order, UUID, corner, radius), the other
Fillet's radius and the shared-side rule, keeps its draft on Cancel and
refusal, and shows a worker refusal inside the form as well as in the status
line.

### Out of scope

A third Fillet, editing the base height, Sketch or constraints of a two-Fillet
history, retargeting a corner, Cut with Fillet, Chamfer, picking, preview,
in-place Save.
