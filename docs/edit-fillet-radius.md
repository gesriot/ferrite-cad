# §28B — change the radius of the saved Fillet, in a new copy

[Executed verification and limitations](edit-fillet-radius-verification.md).
[The Fillet this edits](single-edge-fillet.md).

A person opens a saved rounded plate — the §28A Fillet on one vertical edge —
changes the radius, and saves a new `.fcad`. An agent discovers the same
editable Fillet in `inspect --json` and gets the same copy from
`edit-fillet-radius`. The feature stays the same feature, rounding the same
named edge; only its radius changes.

* **UI:** **Edit Fillet radius — <UUID>…** opens a form that shows the
  Fillet, the edge it rounds (the base Extrude and the two Lines of the
  corner, and the corner's coordinates), the saved radius and the bounds.
* **CLI:**

  ```text
  ferritecad edit-fillet-radius <source.fcad> --feature UUID --expect-version HASH \
      --request <request.json> -o <copy.fcad> [--json]
  ```

  The request is v1 `{"request_version":1,"radius_mm":3.0625}`. The edge is
  the saved one and is not part of the request.

## Contract recorded before implementation

### The supported source

Exactly the §28A result, read again from the saved history on every
discovery, preparation and write — never from a cached label and never by
finding a Fillet object somewhere in the document:

* one untransformed XY datum;
* one unconstrained Sketch of four Lines forming an axis-aligned rectangle;
* one forward literal Blind `Extrude`/`NewBody`;
* one Fillet whose `previous` and `edge.feature` are that Extrude, and whose
  joint is a corner of that rectangle;
* one Body whose tip is the Fillet;
* exactly the plane, profile, predecessor and body-tip dependencies;
* exactly the seven names §28A gives the Fillet, by meaning, and no other
  name owned by it;
* every row losslessly rewritable by this reader.

The frame is checked by the same reader the Cut editors and §28A use
(`cut_edit::saved_history`). It is asked about the history *under* the
Fillet: the Fillet row and its two edges are set aside, and the Body's tip is
read as the Fillet's predecessor. It then requires zero Cuts. There is no
second frame reader.

Each of these is refused with its reason:

* a document with no Fillet, or with more than one;
* a Fillet that is not the Body's tip;
* a Cut, a Revolve, a circle or annulus, constraints, a placement, a formula
  height, a non-rectangle, several Bodies;
* a Fillet whose names are not exactly the seven §28A gives;
* an unknown field or a payload this reader could not rewrite byte for byte;
* a feature UUID that is not that Fillet.

### Identity — what may change

The only model change is `radius_mm` in the selected Fillet's payload, which
changes that row's `payload` and `payload_hash`, plus the established
`meta.modified_at` stamp.

Everything else is preserved: the document id; the UUIDs of the Body,
Extrude, Sketch, Lines and Fillet; the producer and `ProfileJoint` the Fillet
rounds; every topology reference UUID and meaning; object names, ordinals and
parents; dependencies; the Body's tip; and every capability row. Nothing is
reminted, deleted or recreated.

### The exact SQL allowlist

Between the source and the copy:

| table | allowed to differ |
| --- | --- |
| `objects` | the selected Fillet row's `payload` and `payload_hash`; nothing else in that row or any other row |
| `meta` | `modified_at` |
| every other table, including `deps`, `topology_refs`, `capabilities` | nothing |

Row counts are equal in every table.

### Radius policy

It is §28A's, unchanged and shared: `0.01 mm ≤ r ≤ ½ · min(adjacent Lines)`
of the saved corner. Nothing is clamped. A radius outside it is refused
(`input`) with the numbers, in discovery validation, in preparation, in the
writer's re-derivation and in the evaluator.

**Same radius.** Asking for the radius already saved is accepted, not
refused. It publishes a copy whose Fillet payload is byte-identical (the
same `payload_hash`); only `meta.modified_at` differs. Nothing is deleted or
recreated. This is the same rule `edit-revolve-angle` follows for an
unchanged angle.

### Writer

`Document::write_fillet_radius(prepared)` runs one checked transaction:

* It re-reads the document and re-derives the prepared value inside the
  transaction: the same content version as prepared, the same frame, the same
  Fillet and a radius the policy accepts.
* It then compares the whole prepared row with the one derived. A stale
  version, a forged payload (another edge, another predecessor, another name,
  an out-of-policy radius) or another row is refused.
* It updates exactly one `objects` row's `payload`/`payload_hash` and stamps
  `modified_at`.

### Copy job

`edit_fillet_radius_copy` runs through the existing `edit_object_copy`, for UI
and CLI alike. That gives:

* a read-only source snapshot and the expected `DocumentVersion`;
* no-clobber publication, and refusal of source and alias destinations;
* a baseline cold rebuild;
* the write;
* a strict cold rebuild in which every saved reference resolves under its
  original meaning;
* the version checked again, SQLite closed, and atomic publication;
* cancellation boundaries throughout.

The reference check covers:

* every historical name at the base Extrude;
* every `OriginCap`/`OriginSide` at the Fillet;
* the `EdgeFilletFace`.

No name is minted, so none is added to the check. A published copy whose
report is lost stays published, and the CLI exits 7. It is never retried or
rolled back.

### Cache

Nothing new is keyed. `eval.fillet.named` already includes the radius bits,
so a changed radius misses and rebuilds the Fillet, while the unchanged base
Extrude keeps its key and is reused. Cold, Miss and Hit agree.

### Clients

* **`inspect --json`** gains an additive top-level `fillets[]`, one row per
  saved Fillet, read from the existing pinned snapshot with no kernel. Each
  row has:
  * `feature_id` and `name`;
  * `body_id` and `previous_feature_id`;
  * `edge` (`feature_id`, `joint`), `corner_mm` and `radius_mm`;
  * `radius_edit`: `available`, `refusal`, `document_refusal`,
    `min_radius_mm` and `max_radius_mm`.

  No existing field, type, operation or schema changes.
* **`edit-fillet-radius`**: JSON v1, strict and bounded (65536 bytes).
  Unknown, duplicate and escape-duplicate keys, a non-object request and a
  non-number radius are refused as `input`. Another `request_version` is
  `unsupported`.
  * **Result:** `destination`, `document_id`, `body_id`, `feature_id`, the
    saved `edge`, `corner_mm`, `previous_radius_mm` and `radius_mm`. All of
    it comes from the job's own facts, with no second read or rebuild.
  * **Exit codes:** 0 published, 2 refused, 7 report lost.
  * **Order of checks in a stub build:** the request is read and parsed
    first, then the kernel is asked for. So a malformed request is `input`
    in a stub build too, and a well-formed one is `unsupported` (no Open
    CASCADE) before any structural check. Discovery works in a stub build.
* **UI:** the same discovery row and request/job.
  * The form shows the Fillet UUID, the edge meaning, the corner, the saved
    radius and the bounds.
  * Apply validates with the domain's rule; Save asks for a destination.
  * The draft survives a cancelled Save, a worker refusal and a stale reply.
  * Publication goes through the existing async Open, and geometry runs off
    the window thread.

### Out of scope

* a second Fillet, or retargeting to another edge;
* radius zero as deletion;
* cap-edge fillets, chains and a variable radius;
* Chamfer;
* editing the upstream Sketch or height of a filleted part;
* live preview, mouse picking and in-place Save.

The broader Fillet/Chamfer milestone stays open.
