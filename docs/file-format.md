# Native document format

A `.fcad` document is a SQLite database tagged with application id `FCAD`.
It uses rollback journalling and full durability. The schema is created directly:
metadata, objects, dependencies, imported source bytes and checkpoints. Object
payloads carry BLAKE3 integrity hashes. Output topology names are derived during
rebuild and are not stored in the document. A file containing the removed
`topology_refs` table is refused before model payloads are read; it is neither
partially opened nor silently rewritten.

Each object has one CBOR envelope containing `type` and a nested CBOR byte string
`payload`. Each supported type has one payload structure. Imported scenes store
source definition keys and durable placement identities. An unknown type or an
unreadable payload fails the entire opening operation with the object id and type;
opening a damaged document does not modify it.

A `.fcad-cache` sidecar is tagged `FCAC`. It uses WAL and normal durability, and
binds derived blobs to a document id, kernel build and input key. Unreadable or
mismatched caches are discarded and recreated. A cached entry whose bytes no
longer match their hash is dropped; a miss rebuilds geometry.

The document format is unstable. Change its schema and payloads directly and
regenerate affected fixtures. No migration, layout version or capability contract
is stored or negotiated.

Editable model dimensions store `Expression { source, value }`: Extrude end
conditions, parameters, Fillet `radius_mm`, Chamfer `distance_mm`, the `angle`
of a partial Revolve, and sketch constraint `distance`, `radius`, `angle`, and
Fixed `x`/`y`. Values use millimetres and radians. Writers currently use canonical
numeric source text (`Expression::constant`); rebuilds consume the stored value
without evaluating source text. Partial Revolve angles and their policy bounds
use radians; UI and CLI degree input and display convert at their boundaries.

Sketch curve coordinates, circle radii and arc angles remain `f64`: they describe
solver state and the initial approximation, rather than editable dimensions.
Extrude and Revolve name their consumed feature through `previous`. `NewBody`
requires no predecessor; `Add`, `Cut` and `Intersect` require one. Extrude keeps
`Blind`, `Symmetric`, `ThroughAll`, `UpToFace` and `UpToVertex` plus `reversed`;
`ThroughAll` is valid only
for `Cut` and `Intersect`. New bodies and additive bosses have nothing to go through.
`UpToFace` stores `result: ObjectId`, `face: EntityReference` and signed finite
`offset_mm: Expression`; `UpToVertex` stores `result` and `vertex: EntityReference`.
The former requires a face kind, the latter a vertex with `selection: one`.
Each adds a `support` dependency on its result feature; missing or extra rows refuse
validation. These fields are persisted in the tagged end-condition payload. Changing
an existing condition adds/removes that row and validates new targets and graph cycles.
The signed `extrude_end_offset` expression slot edits only `UpToFace`.
Revolve stores `previous`, `axis` (`sketch_x`, `sketch_y` or `{"line": curve UUID}`),
and `FullTurn` or a partial angle expression. A Line axis follows its stored
start-to-end direction and may be construction geometry. There is no stored
axis-segment classification.

Both sweeps require `regions: Vec<StableEntityId>`. Empty selects every sketch
region; each nonempty entry identifies a curve on a region's outer boundary.
Repeated references to the same region select it once. Missing, construction
and hole-boundary curves refuse selection without retargeting. Containment
uses even/odd depth, with immediate child loops as holes. Fixtures are
regenerated when this payload changes; there is no format migration.

DatumPlane is one object type with a tagged `kind` definition: `fixed` stores
`placement`; `offset` stores `from` and `distance_mm: Expression`; `face` stores
`result`, a face `EntityReference` and `offset_mm: Expression`. Offsets are finite,
signed millimetres. An Offset depends on its source datum with role `plane`; a Face
depends on its result feature with role `support`. Definitions and dependency rows
must agree. Sketches still store only their plane UUID. Computed frames are derived,
not persisted. See [datum planes](datum-planes.md) for orientation and refusals.

The `brep.generative.v2` cache stores a feature's own derivation nodes, bindings,
ancestry and incidence with its B-Rep; model lineage shares predecessor derivations.
Scene entities carry the feature whose result holds them, the producer that
minted the name, the kind and the name digest. A name with several entities has
no exact scene selection binding.

Fillet stores `previous`, a nonempty `edges` vector and `radius_mm: Expression`;
Chamfer stores `previous`, `edges` and `distance_mm: Expression`. Each
`EntityReference` stores a diagnostic `StableEntityId` alongside its meaning.
The meaning is the CBOR array `[bstr16, uint, bstr32, uint]`: producer UUID,
kind (face 0, edge 1, vertex 2), name digest and selection (one 0, all descendants 1).
Its canonical encoding is 54 bytes. Reading checks field lengths, tags, array arity
and trailing bytes. The diagnostic UUID is outside that meaning and the cache key.
Fillet and Chamfer require unique edge meanings. Their keys contain the predecessor
key, sorted meanings and amount. Reordering references or changing diagnostic UUIDs
does not invalidate geometry.

An edit may remove an unconsumed output name. The no-new-failed-features rule protects
consumers. A known name without descendants refuses; an exact selection with several
edges refuses, while an explicit all-descendants selection consumes the full set.
