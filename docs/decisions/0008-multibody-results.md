# 8. A feature result is a set of solids; a Body owns its history

**Status:** accepted, 2026-10-31. Extends the result contract of
[ADR 0004](0004-feature-predecessor.md) under
[ADR 0006](0006-generic-modelling-core.md) D1–D3, D5 and D6.
This decision is documentation only. Implementation belongs to M1–M3 below;
the current one-solid refusals remain until those changes land.

## Evidence and current limits

Baseline: `6bfb617e`, including the face/vertex end conditions from PR #138.
`Body.tip_feature` already aliases a feature result; evaluation creates no Body
objects. A failed tip rolls the displayed Body back to its nearest built
predecessor, with a degraded status. Export records an unbuilt-body omission
instead of exporting that fallback as a successful current result.

The restrictions are at several boundaries:

- `GeometryKernel::boolean` promises one solid. `fc_occt_boolean` rejects any
  other count and extracts `solids(1)` before capturing history.
- The cold evaluator rejects a multi-region NewBody with
  `the selected regions sweep into N separate solids, and a new body must be one solid`.
- The local Fillet/Chamfer builder rejects a result containing more than one solid.
- `validate_solids` already accepts a nonempty valid result with finite positive
  total volume; it does not itself prove positive volume for each member.
  Elementary topology and `brep.generative.v2` already describe whole shapes.

### Native experiment

Run on macOS with the installed OCCT 8.0.1, without PlaneGCS. An isolated copy
of the baseline outside the repository first reproduced all four refusals.
For the second run, bypass only the boolean count guard and NewBody count guard,
and retain the complete boolean shape instead of extracting its first solid.
Leave provenance capture, naming, validation and archive code unchanged. Add
diagnostic enumeration of solid membership by native sub-shape identity.
No experiment source, binary or raw output is part of this change.

The plate is 60 × 40 × 10 mm. The slot tool spans x = 28…32, y = −2…42,
z = 0…11 mm; the detached boss is 10 × 10 × 10 mm starting at x = 70.
The two circular regions have radius 3 mm, centres (15, 20) and (45, 20),
and depth 10 mm. Intersect uses those cylinders against the plate.

| Case | Solids | Volume, mm³ | Faces / edges / vertices | Output names | Ambiguous names |
|---|---:|---:|---:|---:|---:|
| Plate split by slot | 2 | 22400 | 12 / 24 / 16 | 52 | 0 |
| Detached Add | 2 | 25000 | 12 / 24 / 16 | 52 | 0 |
| NewBody from two circles | 2 | 565.486678 | 6 / 6 / 4 | 16 | 0 |
| Intersect with two circles | 2 | 565.486678 | 6 / 6 / 4 | 16 | 0 |

Every member passes `BRepCheck_Analyzer` and has positive volume. The slot
members each have volume 11200 mm³. Every elementary output has a name.
The unmodified bridge history routine reports these relation counts:

| Case | Elementary inputs accounted for | Kept | Modified | Generated | Deleted |
|---|---:|---:|---:|---:|---:|
| Slot Cut | 52 | 18 | 20 | 28 | 22 |
| Detached Add | 52 | 52 | 0 | 0 | 0 |
| Intersect | 42 | 0 | 20 | 0 | 24 |

These are relation counts, not disjoint input categories. They include both
target and tool. Modified/Generated come from the native builder; Kept and
Deleted include the existing native result-membership correction from ADR 0007.
The two-circle sweep instead has ten generated root-to-output relations;
its curve/end/cap roots pass the unchanged complete-profile check.

For the slot, four original plate faces each have two descendants; exact
references to them refuse. Two original side faces each survive once, one in
each solid. Their producer remains the plate feature. In the measured document
their digests begin `607c595f9849b8d3` and `fe9ab08e7e5fe786`; these are examples
for those UUIDs, not reserved names. All twelve result-face names resolve once
and each face belongs to exactly one solid. Native containment, not coordinates,
therefore distinguishes the pieces. Eighteen of the plate's 26 elementary names
survive unchanged. Detached Add preserves all 26 plate names.

Two cold rebuilds, a cache fill and a warm rebuild retain identical digest sets
and binding cardinalities for each document. Every feature hits the cache in
the warm run. This establishes that the existing naming and archive machinery
can represent these compound witnesses; it does not claim that all consumers,
piece selection or a complete multibody product already work.

## Decisions

### 1. Return a nonempty set of valid solids

Choose one operation result handle containing an unordered, nonempty set of
native solids. A singleton may remain a native solid; several members use a
compound. A compound is a transport container, not a new assembly or a durable
ordering. Reject invalid members, nonfinite or nonpositive member volume, an
empty result, and free lower-dimensional remnants outside the solids. Do not
silently discard shells, faces or wires to make a result pass.

NewBody may sweep several regions. Add may add disconnected material; Cut may
split a target; Intersect may retain several fragments. Extrude and Revolve use
the same rule. Fillet/Chamfer may consume and return several solids, retaining
unaffected members. The kernel still decides whether the requested operation
is valid. No particular contour count, profile class or contact pattern is an
application whitelist.

Cut must still remove material from the complete target. A miss remains a
refusal, even if a subsequent selection could discard a solid. Intersect with
no volumetric result and a Cut that removes everything remain refusals.
Validation and removed-volume accounting precede publication and selection.

The replacement boolean contract is:

> Return the complete nonempty set of valid solids with finite positive member
> volumes, or refuse. Preserve complete elementary provenance for both inputs.
> Cut must remove material. Never select a result member on the caller's behalf.

Reject one-solid-only results: they exclude the four measured valid cases.
Reject first/largest-solid extraction: it silently destroys material and intent.
Reject an arbitrary shape result: open shells and wires are not solid modelling.

### 2. A Body is one feature history, not one connected solid

Choose `Body` as the document object exposing the complete result of its
`tip_feature`. Its UUID, name, visibility and history persist when a result
changes from one solid to several or back. No piece inherits a privileged
"original Body" identity: the Body remains the set. A Body with no tip stays
an empty history container; a built feature may not produce an empty set.

Keep `previous` pointing to a feature and `BodyTip` pointing from Body to tip.
Keep the prohibition on forked predecessor histories and shared Body tips.
Rebuild never creates document objects. A failed tip retains the existing D2
display rollback of the whole last built set, marked degraded; it does not
publish a convenient surviving piece as the successful tip.

The next Add, Cut or Intersect operates on the complete predecessor set, using
the selected swept tool. Fillet/Chamfer operates on its explicit edge references
within that set and carries the other members through unchanged. There is no
automatic scope over other Bodies, including touching or visible Bodies.
Cross-Body feature scope is separate future work with explicit dependencies;
it is not needed for this decision or for 11c.

Separate document Bodies are unnecessary for disconnected material. If an
independent Body per piece is later needed, it must be an explicit transaction,
not a side effect of rebuild. A possible later command duplicates the history
with fresh object UUIDs, remaps its internal references and applies a piece
selection to each independent copy. It creates new identities deliberately,
keeps the original untouched, and must not share tips or rewrite outside
references. That copying command is not required by M1–M3.

| Alternative | Why rejected |
|---|---|
| One Body per solid, created on rebuild | Mutates the document and invents UUIDs during evaluation. |
| One piece keeps the old Body, others get new Bodies | Requires an unrequested winner. |
| Shared history exposed through several Body tips | Conflicts with ADR 0004 ownership and edits. |
| Independent solid lists beside `tip_feature` | Stores a second answer to what the Body contains. |

### 3. Keep all by default; select pieces through named faces

Choose **all pieces** as the default result. This makes a slot, disconnected
boss and multi-region NewBody build without a guess or a new user decision.
The tree reports the solid count; it does not ask which one is "the" body.

Explicit removal is a separate `KeepSolids` feature in the same predecessor
chain. Its payload is `previous` plus `All` or a nonempty `ContainingFaces`
list of the existing face `EntityReference`s. Each reference keeps its
diagnostic UUID. Resolve the reference in `previous`, then find the solids
containing its resolved faces by native topological membership:

1. `one` still requires exactly one face, before testing containment.
2. `all_descendants` still resolves the complete nonempty face family.
3. Every resolved face must have exactly one owning solid, and the entire
   family must lie in that same solid. Otherwise refuse this reference.
4. Retain the union of the solids selected by all references, once each.
   Several distinct face witnesses may intentionally select the same solid.

This is a containment query, not a new kind of topology name. No `solid` tag,
solid digest or per-piece UUID is introduced. Coordinates, centre of mass,
volume, native traversal positions and mesh component numbers never select a
piece. A solid ordinal may be a temporary display label only.

When a witness disappears, belongs to several solids, or has descendants in
several solids, fail `KeepSolids` with the feature UUID, reference UUID and
observed cardinality. Do not choose a neighbour or keep every candidate as a
fallback. A family split within one solid is allowed only when the stored
reference explicitly says `all_descendants`. A merge that still leaves one
owning solid keeps the stated "solid containing this face" meaning; it does
not promise to preserve a volume or an old connected-component count.

Reject a mandatory selection after every split: it would fail all four common
cases without adding information. Reject a new solid-name algebra: existing
face references and membership express the required selection. Reject a keep
field resolved against the same feature's unfinished output: it obscures the
dependency and the point at which splitting names are established.

### 4. Preserve the existing naming rules

Name the complete geometric operation first, following ADR 0007 §§1–4.
Splitting a solid is not itself a name-producing event. Its faces, edges and
vertices follow their actual Modified/Generated/Deleted relations; split face
names use the existing historical boundary witnesses. Multi-region cap
refinement stays unchanged. Ambiguous unconsumed names remain legal.

`KeepSolids` consumes that already named result. It performs no geometric
operation: retained sub-shapes are Kept, discarded sub-shapes are Deleted,
and it generates no topology or derivation nodes. Rebind retained entities,
restrict ancestry to surviving entities and reconstruct complete incidence.
Inherited meanings with no descendants have no row; the graph still knows
them and resolution refuses. Do not rename a retained split face back to its
unsplit parent just because its sibling was discarded.

Separating selection from construction is necessary here: filtering a Cut
before naming could turn a two-output modification into a one-output one and
change which existing rule applies. The explicit subsequent feature avoids
that ambiguity without changing either rule.

#### Clarification to ADR 0007

No amendment to §§1–4, reference kind tags or existing digests is required.
The phrase "actual final solid" in §7 will mean the entire operation result
set. §8's atomic B-Rep includes that set. A later KeepSolids result has its own
archive and identity-preserving selection history. These wording updates belong
to implementation; ADR 0007 remains unchanged in this documentation PR.

### 5. Consumers use the result set explicitly

**Topology references.** Fillet/Chamfer, face datums and referenced sweep ends
keep the common resolver and exact cardinality rules. Resolve across the whole
consumed result, not a first member. An edge set can span solids; a local
builder may process each affected solid and compose complete history while
retaining untouched solids. Publish only after all requested edits succeed.
This is an adapter implementation choice, not permission to skip failing edges.
An edge shared by multiple solids needs explicit handling or a refusal, never
an arbitrary owner.

Face datums still require one plane and outward normal for every resolved
member. UpToFace retains native common-surface/orientation and full-start-cap
coverage requirements; multiple profile regions can have several corresponding
valid output solids. An unfinished region fails the entire feature. UpToVertex
retains its exact vertex and positive projected extent. Membership does not
relax any of those geometric conditions or change reference meanings.

**ThroughAll.** Compute projected bounds over every solid of `previous`, in
the chosen direction, and retain the named 1 mm far margin. It still applies
only to Cut and Intersect; there is no new direction guess or search of other
Bodies. A member behind the plane does not shorten the forward extent; if the
entire target is behind it, retain the existing refusal. The operation acts on
the complete target and uses the complete result key on cache lookup.

**Up to next (11c).** Fix the target scope now: the complete `previous` result,
including all its solids. Other Body tips, visibility and tree order are not
targets. NewBody has no implicit "next" target; without a separately designed
explicit support result it refuses. 11c must define and prove the first forward
terminating boundary with complete profile coverage, using native topology,
not the nearest solid centroid, a sampled ray or the first enumerated member.
Any ambiguity or incomplete coverage refuses. Detailed surface construction
belongs to 11c, after multibody consumers are ready.

**Scene and export.** Keep one native Body definition, one placement and one
mesh containing all of its solid components. Keep face/edge/vertex partitions
for picking; derive a temporary solid-membership index from the B-Rep. The tree
shows one Body with its count, with optional transient component rows. A saved
piece choice stores a face witness, never a mesh or tree ordinal.

FBX retains one geometry definition and occurrence keyed by Body UUID; all
disconnected components must be present once. Independent readers must check
component count, winding, bounds and total volume as appropriate. Existing
imported STEP assembly definitions/occurrences retain their own identities and
must not be flattened or duplicated by this native Body policy.

There is currently no shipped native-model STEP export command. STEP writing
exists in the OCCT smoke round trip and corpus generator; do not describe those
as a product exporter. A native STEP writer, when introduced, must write the
Body's complete compound as one product/placement with multiple solid items,
not one anonymous product per enumeration slot. Read-back must preserve every
solid, volume and Body-level structure. Extending the product export surface
is outside M; M3 verifies the native writer/read-back contract in the test path.

Reject one mesh and exported object per solid: it invents unstable external
identities and changes existing Body placement semantics unnecessarily.

**Cache.** Keep one atomic `brep.generative.v2` record per feature: the complete
B-Rep set, owned nodes, entity slots, ancestry and full incidence. The model
still has one shared derivation graph. Solid membership is reconstructed from
the restored B-Rep, not stored as durable ordinal identities or extra names.
`KeepSolids` keys include predecessor key and canonical reference meanings,
excluding diagnostic UUIDs; its All mode and selected mode are distinct.
Restore checks the same full incidence and provenance constraints, as well as
the result validity contract. Corrupt or incomplete records reject atomically.
An archive hit never fabricates raw operation history.

### 6. Store selections in the feature chain; publish atomically

Keep the existing Body, Extrude, Revolve, Fillet and Chamfer payload meanings.
Add `KeepSolids { previous, selection }` and its `Predecessor` edge. References
resolve only in that predecessor's lineage: they need no self-edge and do not
implicitly add dependencies on other Bodies. `ObjectPayload::dependencies`,
type registration, canonical hashing, validation and editing gain this case.
D6 applies: regenerate affected fixtures directly; add no migration, legacy
reader or format-version negotiation.

The shared library transaction adds or edits this feature and advances the
Body tip once. UI and CLI use the same operation. A slot and a KeepSolids
selection can be added together with any required repairs to consumers. Undo
and Redo restore payloads, tip and references together. Editing selection to
All restores the complete predecessor result without bypassing its feature.

Multiple solids alone are no longer a D2 failure. A concrete selection failure
appears in the ordinary refusal list, for example:

> KeepSolids FEATURE: reference REF resolves to faces in 2 solids; one containing
> solid is required. Replace this reference or choose All.

An exact face split retains the existing face-count refusal before membership
is tested. A deleted reference reports deletion. Collect all failed references
and newly failed consumers; refuse the candidate atomically. A broken saved
document opens for repair, with its failed selection and skipped dependents.
Independent Bodies still build. Never publish a partial selected set.

Implementation must replace the one-solid comments/refusals in
`GeometryKernel::boolean`, `fillet_edges`, `chamfer_edges`, the Body doc comment,
the cold evaluator's NewBody guard, the OCCT boolean/local-operation guards,
and their descriptions in generic sweeps and Fillet/Chamfer documentation.
Audit referenced-end coverage wording separately: remove cardinality limits,
not complete-cap or valid-solid requirements. Retire only tests asserting the
replaced policy; preserve geometry, history, naming and atomicity coverage.

### 7. Deliver vertical slices before the live session

Keep the owner's order: **M (this ADR, then M1–M3) → 11c → 15a → 12 → 13 …**.
This places 11c after its result-set foundation without moving 15a or task 12.
Each estimate below is added/changed non-test lines, including documentation;
tests are separate. If a slice approaches 3000 lines, split it before proceeding.

| Task | Goal | Estimated non-test lines |
|---|---|---:|
| M1 | Result sets and transactional KeepSolids, usable through CLI | 1800–2600 |
| M2 | Local edge operations and referenced ends on result sets | 900–1800 |
| M3 | Window selection, scene/export evidence and complete regression audit | 900–1800 |

**M1 acceptance.** A slot splits the measured plate into two valid solids; All
keeps 22400 mm³, a face witness keeps one 11200 mm³ member. A later Cut operates
on the selected result. NewBody with two circles, detached Add, split Intersect
and multiregion Revolve build complete sets. Empty results, no-material Cut,
invalid members and lower-dimensional debris refuse. Missing and cross-piece
witnesses refuse with reference identity; no geometry ranks pieces. Exercise
CLI edit, whole-transaction failure, Undo/Redo, cold/warm selection and corrupted
archive rejection. Verify existing singleton names exactly against main. This
first slice delivers real geometry and selection, not only an API or codec.

**M2 acceptance.** Fillet/Chamfer edges on either or both members; unaffected
members and names survive. A failed requested edge prevents all publication.
Exercise planar/curved face families and offsets, multiple swept regions,
UpToVertex, datum attachment and changed/deleted references. ThroughAll covers
separated forward members at different heights with identical bounds, extent,
topology and named results across Linux, macOS and Windows. Keep all provenance
and valid-face/solid checks. Test selected pieces merging and splitting again.

**M3 acceptance.** The existing window displays one Body with two components,
selects a piece by a face witness through the shared edit path, and repairs a
lost witness without retargeting. Check the full GUI suite under the memory
watchdog, mesh picking and Undo/Redo. FBX's independent reader sees both
components once under one Body identity; native STEP write/read-back retains
both solids without changing imported assembly identity. Audit every retired
one-solid expectation with a replacement. Regenerate affected fixtures and
document the actual CLI/window interaction.

Every slice runs the three configurations (stub, OCCT without solver, OCCT with
PlaneGCS), appropriate independent readers, all-target clippy and every CI
workflow on the three operating systems. Compare existing names and measure
ordinary corpus cold/cache results against fresh main, using complete document
time. Keep new multibody cases as separate rows and report their solid counts
and naming cost. Full naming remains eager. No archive optimisation or naming
rule change is hidden in this work.

These are implementation estimates, not measured diff sizes. Stop and return
with a witness if native elementary history is incomplete, existing naming
rules or digests must change, selection requires a geometric tie-break, or
the work cannot be delivered in these bounded slices before task 12.
