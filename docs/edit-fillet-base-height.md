# §28C — change the height of a rounded plate, in a new copy

[Executed verification and limitations](edit-fillet-base-height-verification.md).
[The Fillet this keeps](single-edge-fillet.md);
[its radius edit](edit-fillet-radius.md).

A person opens a saved rounded plate — the §28A Fillet on one vertical edge —
changes the plate's Blind height in the existing **Edit extrusion** form, and
saves a new `.fcad`. The rounded edge is the same edge with the same radius;
only the height changed. An agent does the same with the existing
`edit-extrude`. There is no new command, request format or copy pipeline.

## Contract recorded before implementation

### The supported source

Exactly the frame §28B edits, read again from the saved history on every
discovery, preparation and write:

* one untransformed XY datum;
* one unconstrained Sketch of four Lines forming an axis-aligned rectangle;
* one forward literal Blind `Extrude`/`NewBody` whose distance is not a
  formula or parameter;
* one Fillet whose `previous` and `edge.feature` are that Extrude, whose joint
  is a corner of that rectangle and whose radius answers to §28A's policy;
* one Body whose tip is the Fillet, and no Cut;
* exactly the plane, profile, predecessor and body-tip dependencies;
* exactly the seven names §28A gives the Fillet, by meaning, and no other
  name owned by it.

This is checked by the same function the radius edit uses
(`fillet_radius::saved_fillet`, over `cut_edit::saved_history_under_fillet`
and the §28A `fillet_references`). There is no second reader, validator or
copier. The selected feature must be that Fillet's `previous`, the base
Extrude; the existing literal-Blind check (`editable_extrude`) still applies
to it.

Only the height becomes editable on this frame. Every other editor keeps
refusing a filleted part by naming the Fillet: the Sketch coordinates and
constraints, circle and annulus edits, Cut add and edit, Revolve edits and a
second Fillet. The radius edit is unchanged.

Refused, each with its reason and nothing written:

* the Fillet itself, the Sketch or any feature other than the base Extrude
  (`--feature`);
* a document whose Fillet is outside the frame above (two Fillets, a Cut, a
  foreign or extra name, a non-rectangle, a constraint, a formula height);
* a zero, negative or non-finite height (`input`, as today).

### Height policy — measured first

Probed through the product's own `GeometryKernel::fillet_edge` on pinned
OCCT 8.0.1: the 37.5 × 12.25 mm plate at (−4.5, 3.25), one corner, radii
0.01, 3.0625 and 6.125 mm (the §28A maximum), with heights from 1e-7 to
1e5 mm.

| height (mm) | every radius |
| --- | --- |
| 2e-5 … 1e5, including every h < r | valid, 7 faces, `Cylinder{r}` on a vertical axis r inward of the corner; B-Rep volume against `(W·D − (1 − π/4)·r²)·h` within 2.5e-16 relative |
| 1e-5 and below | OCCT refuses to round the edge (`kernel`) |

The limit does not depend on r, and r's own bound (`½ · min(adjacent
Lines)`) does not depend on h. So the domain adds no height bound of its own:
the policy stays the existing `edit-extrude` rule, a finite, positive
distance. A height OCCT cannot round is refused by the kernel during the
copy's strict rebuild, typed as `kernel`, and nothing is published. Nothing is
clamped.

### Identity — what may change

The only model change is the base Extrude's Blind distance, which changes
that row's `payload` and `payload_hash`, plus the established
`meta.modified_at` stamp.

Everything else is preserved byte for byte: the document id; every object
row other than that one, including the Fillet's payload (its UUID,
`previous`, edge producer and joint, and radius); every dependency; every
topology reference UUID and meaning; object names, ordinals and parents; the
Body's tip; every capability row. Nothing is minted, deleted or recreated.

### The exact SQL allowlist

| table | allowed to differ |
| --- | --- |
| `objects` | the base Extrude row's `payload` and `payload_hash`; nothing else in it or in any other row |
| `meta` | `modified_at` |
| every other table, including `deps`, `topology_refs`, `capabilities` | nothing |

Row counts are equal in every table. Asking for the saved height is accepted
and publishes an identical payload; only `modified_at` may differ, as for
every other height edit.

### References

Every saved name must resolve after the copy's cold rebuild, under its
original meaning: the base Extrude's historical names, the Fillet's
`OriginCap`/`OriginSide` and its `EdgeFilletFace`. The legacy standalone
extrusion edit keeps its weaker promise (an already unresolved name may stay
unresolved); that exemption is named for a height edit with neither a Cut
history nor a Fillet, and does not extend to this one.

### Writer

`Document::write_extrude_height` takes the checked-history branch: one exact
`UPDATE objects SET payload, payload_hash` of the base row and the
`modified_at` stamp, with no rewrite of capabilities and no new names.
Inside the transaction it re-derives the whole prepared value from the
current document: the same content version, the same frame, the same Fillet
facts and the same row. A stale version, a forged row or another frame is
refused.

### Copy job, cache and evaluation

The existing `edit_extrude_copy` over `edit_object_copy`, for UI and CLI
alike: read-only snapshot and `DocumentVersion`, no-clobber, source and alias
refusal, baseline cold rebuild, the write, strict cold rebuild, the version
checked again, atomic publication, cancellation boundaries, exit 7 on a lost
report without rollback.

No new evaluation, history, cache or FFI route. The base Extrude's key
covers its distance, and `eval.fillet.named` is keyed by the predecessor's
key, so a changed height misses on both; unchanged inputs (datum, Sketch)
are not invalidated.

### Clients

* **`inspect --json`**, additive only:
  * `edit_extrude.available` becomes `true` on a supported rounded plate, and
    stays `false` with the Fillet's reason otherwise.
  * The base Extrude's `features[]` row is `editable` and gains
    `fillet_base`: `fillet_feature_id`, `body_id`, `edge` (`feature_id`,
    `joint`), `corner_mm` and `radius_mm`. It is `null` on every other row
    and in every other document. It is not a Cut history, and the
    `base_height_edit*` fields stay `null`.
  * No existing field, type or value changes meaning.
* **`edit-extrude`**: unchanged arguments, text and JSON result, envelope and
  exit codes (0 published, 2 refused, 7 report lost).
  * **Order of checks in a stub build:** with `--json`, UTF-8 paths first;
    then the source is read (an unreadable one is refused as today); then the
    kernel is asked for. So every well-formed request, whatever its height or
    feature, is `unsupported` (no Open CASCADE). Discovery works there.
* **UI:** the existing **Edit extrusion** form and job, and async Open. The
  base row shows the Fillet as context ("Rounded by Fillet … at (x, y),
  r … mm; the Fillet keeps its edge and radius"). The draft survives a
  cancelled Save, a worker refusal and a stale reply.

### Compatibility

No capability, schema, payload version or archive tag is added. A copy
carries the same capability rows as its source, so any reader that opens a
§28A copy opens this one the same way.

### Out of scope

* editing the profile coordinates, the radius here, the edge, constraints or
  any other parameter;
* a second Fillet, cap edges, a Cut with a Fillet, a Revolve, an arbitrary
  plane, deleting or retargeting the Fillet;
* live preview, mouse picking and in-place Save.

The broader Fillet/Chamfer milestone stays open.
