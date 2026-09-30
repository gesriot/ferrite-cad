# §28I — change the height of a plate rounded twice, in a new copy

[Executed verification and limitations](edit-two-fillet-base-height-verification.md).
[The height edit under one Fillet](edit-fillet-base-height.md);
[two sequential Fillets](sequential-edge-fillets.md);
[their radii](edit-sequential-fillet-radii.md).

A person opens a plate that §28G rounded twice — Extrude → Fillet 1 →
Fillet 2 → Body tip — changes the plate's Blind height in the existing **Edit
extrusion** form, and saves a new `.fcad`. Both rounded edges keep their
Fillets, radii, corners and names; only the height changed. An agent does the
same with the existing `edit-extrude`. There is no new command, request
format, copy pipeline, solver or geometric route: this is §28C's edit on
§28H's reading of the history.

## Contract recorded before implementation

### The supported source

Exactly the §28G class, read again from the saved history on every
discovery, preparation and write by the reader the radius edit uses for two
Fillets (`fillet_radius::saved_fillet_for_radius` →
`saved_sequential_fillet`, over `cut_edit::saved_history_under_fillets`):

* one untransformed XY datum and one Sketch whose stored Lines are an
  axis-aligned rectangle — unconstrained, or carrying the constraint editor's
  managed Line family;
* one forward literal Blind `Extrude`/`NewBody` whose distance is not a
  formula or parameter (`editable_extrude`, unchanged);
* **Fillet 1**: `previous` = `edge.feature` = that Extrude;
* **Fillet 2**: `previous` = Fillet 1, `edge.feature` = that Extrude, another
  corner; the Body's tip;
* no Cut, no other object; exactly the plane, profile, two predecessor and
  one body-tip dependencies; Fillet 1's seven §28A names and Fillet 2's
  eight §28G names, by meaning, and no other name owned by either.

The selected feature must be the base Extrude (Fillet 1's `previous`). Both
saved Fillets are read and carried as they are; their radii, and the pair
rule between them, are the ones §28H checks. A plate with one Fillet takes
§28C's path unchanged (`saved_fillet`, the same frame as before).

The guard that the Sketch-coordinate, constraint and add-Fillet editors apply
(`fillet_over_plate`, exactly one Fillet) is **not** widened: with two
Fillets they keep refusing, now saying that the radii and the height can be
edited.

### Typed context

The height preparation and the extrusion catalogue carry the Fillets over the
plate as they stand in the history: `fillet` is the Fillet whose `previous`
is the base (Fillet 1, as in §28C), and a new `second_fillet` is the Fillet
whose `previous` is Fillet 1 (`history_index` 2). Fillet 2 is never described
as a direct child of the base. JSON `features[].fillet_base` keeps its shape
and still describes the Fillet on the base; a new additive
`fillet_base.second_fillet` describes Fillet 2 (`fillet_feature_id`,
`previous_feature_id`, `history_index`, `edge`, `corner_mm`, `radius_mm`),
and is `null` for a plate with one Fillet.

### Height policy

§28C's, unchanged: a finite, positive literal distance; the domain adds no
bound of its own. A height the kernel cannot round is refused by the copy's
strict rebuild (`kernel`), nothing published. For a constrained plate the
rebuild solves the Sketch as it always does and judges both corners and the
pair on the solved Lines; the height does not enter those checks.

### SQL allowlist

* `objects`: the base Extrude row's `payload` and `payload_hash`;
* `meta.modified_at`.

Nothing else: not the Sketch (its stored coordinates stay the solver's
starting guess, never replaced by solved ones), not either Fillet row, not
dependencies, names, capabilities, `schema_version`, the Body tip or the
document id. Nothing is minted, squashed, retargeted or re-created. The
writer re-derives the whole preparation inside its transaction — both Fillets
included — against the document version it was prepared from; a forged
payload, a stale version or a changed Fillet is refused.

### Rebuild, names and cache

The copy's strict cold rebuild must resolve every stored name, including
Fillet 1's cylinder as carried into the final Body (`origin_fillet_face`) and
Fillet 2's own. The cache keys chain through the predecessor, so a new
height misses the Extrude and both Fillets; a second rebuild hits all three.

### Refused, nothing written

* the Sketch, either Fillet or any other feature as `--feature`;
* a two-Fillet history outside the class above (a Cut, a third Fillet,
  forged names, a same-corner pair, a formula height);
* a zero, negative or non-finite height (`input`);
* a height the kernel cannot round (`kernel`, at the rebuild).

### Out of scope

A third Fillet, arbitrary edges, Cut with Fillet, Chamfer, editing the Sketch
or constraints of a two-Fillet history, in-place Save.
