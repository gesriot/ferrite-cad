# 7. Derive topology names from operation provenance and named face incidence

**Status:** accepted, 2026-10-24. Implements the design work required by ADR 0006 D3;
production integration belongs to task 08.

## Evidence and scope

The experiment is `ferritecad_topology::generative::name_rebuilt`. It takes a
rebuilt document's feature handles and the same kernel session. Only tests and
`ferritecad-bench/examples/naming.rs` call it. Existing document payloads,
evaluation policies, transactions, UI and CLI use the existing names.

The native corpus includes the committed plate, 16 circular Cuts, 10 and 50
independent bodies, four constrained Fillets, partial and full Revolve, Chamfer,
an open slot, overlapping Cut tools and a pocket changing between ThroughAll
and Blind. The tests compare actual session-local sub-shapes with the current
resolver, rather than matching counts or geometric signatures.

## Decisions

### 1. Names are an interned derivation graph

Choose UUID roots for sketch curves, oriented curve ends and support planes.
An operation output is `(operation kind, feature, generated|modified, input names, entity kind)`;
a cap without a curve input is `(feature, start|end cap, plane)`. Sets are
canonical, order-independent sets. Keep names unchanged when history says
**kept**, and when it says **modified** into exactly one output: a trimmed face,
edge or vertex is the same entity. Only the pieces of a split take a `modified`
name under the splitting feature. Include all inputs when an output has several
parents.

Reject a new name on every modification. Edges and vertices are born from the
names of their incident faces, so a later hole's rim would be renamed whenever an
unrelated earlier Cut started or stopped modifying the top face. The
upstream-change test switches one hole between ThroughAll and Blind and pins the
other hole's face, edge and vertex names.

Represent a node by a domain-separated BLAKE3-256 digest of its tagged operands.
The model reconstructs the graph; a reference never embeds its ancestry.
Keep one complete graph in `ModelNaming`. Each `FeatureNaming` owns only nodes
first created along its lineage by that feature; the model graph is their union,
with no duplicate owners. A boolean's unpublished tool contributes its new nodes
to the boolean feature. Derivation reads existing nodes from the model rather
than copying predecessor graphs. Removing a feature removes its owned nodes.
Ownership is independent of unrelated body order and of cold versus restored
predecessors; unequal nodes at one digest still refuse publication.
Curve roots are qualified by their producer occurrence; sharing a sketch does
not collapse separate solids. The producer identity scopes operation nodes,
so the 50-body corpus has no
cross-body name collisions even where bodies have congruent geometry.
Curve-end orientation is the stored curve's start/end identity, not sweep order.
An Extrude and Revolve with the same feature/root identities produce different
names; the compact-reference test pins that distinction.

Reject role catalogues for document combinations, geometric signatures and
sub-shape ordinals. The prototype names every result in both the 16-Cut and
four-Fillet histories with the same derivation rules.

### 2. Boundaries use named faces and history witnesses

Choose the canonical set of incident face names as the birth identity of an
edge or vertex. Add the operation's input provenance as a witness when present.
Once born, a name survives kept and single-output modified history; a later
split of an incident face does not rename an edge that survived it.
A seam is an ordinary edge with one incident face, not an invented second face.
Curve-end history distinguishes vertices when face incidence alone repeats.

For split faces sharing provenance, refine their names with the canonical set
of historical input-boundary witnesses on each part. This separates the two
pieces of the plate wall in the open-slot test using its surviving original
corners. It does not use an already named output edge, avoiding a naming cycle.

When a sweep has several regions, their caps share the root
`(feature, start|end cap, plane)`. Refine each cap with the canonical set of
curve roots on that region's outer boundary, using the same historical-boundary
witness expression as split faces. Hole boundaries are not cap witnesses. A
sweep with one region has nothing to distinguish and retains its existing cap
names, including when that region contains holes.

If both provenance and these witnesses still coincide, retain one name with
multiple bindings. The open slot's periodic tool wall and some boundaries in
the overlapping-tool test remain such sets. Exact resolution refuses them.
Reject picking by traversal order, first match, centroid, normal, area, nearest
point or parametric position. Raw orientation from an arbitrary OCCT edge
occurrence is not an oriented durable name: the closed-circle root has no named
seam direction to anchor it. Reject using that traversal-dependent sign to split
the periodic-wall classes. No tolerance or geometry metric breaks a tie.

### 3. Splits, merges and deletion retain the complete relation

Choose a set-valued graph. A split has one parent and several descendants,
each `modified` from the parent under the splitting feature; historical boundary
witnesses may distinguish them as above. A merge has the
canonical union of all parent terms, not one arbitrarily preferred parent.
Keep ancestry sets so a deliberately requested descendant family reaches all
its members. A removed input has an explicit deleted survival relation; it can
still have generated descendants of a different meaning.

Store non-empty ancestry for every meaning with descendants in the result.
Do not retain inherited meanings that have lost their last descendant. Keep an
empty row only for an owned `generated`, `boundary` or `split-boundary-history`
output that disappears within its own feature: the caps of a Cut's temporary
tool belong to that Cut and may already be gone in its result. These empty rows
are not carried into later features. A name known by the lineage graph with no
descendants, whether absent or locally empty, refuses resolution with the same
diagnostic. Build an inverse entity-to-ancestor index once for each input instead
of scanning all its ancestry rows for every history relation.

Reject manufacturing suffixes for split pieces or treating a generated child
as survival of its parent. In ThroughAll → Blind → ThroughAll, the pocket floor
appears and disappears under the Cut's end-cap ancestry; a wall is not chosen
in its place. Unaffected upstream names retain their bindings.

### 4. Resolution is exact and has no geometric fallback

Choose a reference with a producer, entity kind, name digest and an explicit
selection: **one** or **all descendants**. One requires exactly one matching
entity of that kind. All descendants requires the complete non-empty ancestry
set, never an arbitrarily selected subset. Validate kind and selection when
reading the reference. Missing, deleted and ambiguous references identify the
reference and fail its consuming feature under D2/D3.

The producer is the feature that minted the name. The consumer resolves the
digest in the result it consumes, through ancestry: the entities of that result
that descend from the name by kept or modified history. A feature inserted
between them leaves the reference valid while the entity survives; a split
makes **one** refuse, and a deletion or a consuming operation such as a Fillet
of that edge leaves nothing to resolve.

Delete `fallback_signature` in 08. Reject preserving it as a nearest-neighbour
or best-score escape hatch. The prototype refuses the open slot's ambiguous
bindings even though their positions could distinguish them.

### 5. Store consumption, derive production

Choose to store only references in consuming payloads: Fillet/Chamfer edge
selections from task 10, sketch support faces and axes from 11. Rebuild derives output
names and ancestry in memory. A reference UUID identifies the consumption for
diagnostics; it is not a UUID assigned to every output entity.

In 08 delete the `topology_refs` table, its output catalogue readers/writers,
`output_names.rs`, `candidate_output_references` and `same_meaning`. Remove the
transaction's catalogue-wide lost-name comparison. Resolve each consumed
reference while building its feature instead: the existing no-new-failed-feature
rule then protects it. Unconsumed ambiguous outputs do not fail a model.

Reject continuing catalogue reconciliation alongside the generative path. The
open-slot experiment can describe ambiguous outputs without requiring every
unconsumed piece to have a unique stored catalogue entry.

### 6. Stored references stay constant size through long histories

Choose this CBOR array for the reference's meaning:

```text
[producer: bstr(16), kind: uint, digest: bstr(32), selection: uint]
kind: 0 face, 1 edge, 2 vertex
selection: 0 one, 1 all descendants
```

It occupies 54 bytes for these tags. Keep the consumption's diagnostic UUID in
its owning payload beside this value. Fifty generations still encode to 54
bytes in the CBOR test. Validate field lengths, tags and trailing data; there
are no document migration or compatibility branches (D6).

Reject recursive embedded expressions, cached kernel handles and a per-output
UUID registry. Derivation nodes and operand graphs are reconstructed from the
model and may be interned during rebuild; the reference holds only the digest.
A collision detected between unequal derivations must refuse the rebuild,
never merge their bindings. Cryptographic equality is not permission to ignore
such a detected structural mismatch.

### 7. The kernel reports complete elementary topology and provenance

Choose `GeometryKernel::shape_topology` for every face, edge and vertex and
its incident face set, and `naming_history` for complete operation relations.
Enumerate inputs independently of legacy `track`. Preserve every generated and
modified output belonging to the final result, every kept input by topological
identity and every deleted input. Account for all elementary input entities,
including a tool's entities; profile curves and their oriented endpoints are
roots for sweeps. Caps are explicit operation outputs.

The OCCT bridge captures these relations while each builder is alive.
Use the sweep's direct `Shape`/`FirstShape`/`LastShape` associations for Extrude
and Revolve, and the boolean/local-operation generated/modified lists plus
`IsSame` result membership for Cut, Fillet and Chamfer. Membership corrects the
known local builder `IsDeleted` answers about surviving edges/vertices; it is
topological identity, not a geometric match. Filter history to the complete actual final
result set, not discarded intermediate results.

Reject reconstructing provenance from a mesh, rerunning an operation on demand
or inferring unreported history. Default trait implementations and the mock
refuse the new queries with Unsupported. Old result DTOs and tracked queries
retain their contracts so the prototype cannot change evaluator decisions.

Determinism concerns the derivation graph, not session handles. Sort and dedupe
name operands, preserve multiplicity as a set of bindings and never serialize
traversal positions. Repeated cold rebuilds of the corpus produce identical
name sets and cardinalities.

### 8. Cache the B-Rep and its naming graph atomically

Choose one derived cache entry containing the complete result set B-Rep, only the feature's owned
derivation nodes, complete name-to-slot sets, ancestry sets and incident-face
relations. Inherited ancestry rows must be non-empty. Each owned `generated`,
`boundary` and `split-boundary-history` node requires an ancestry row; that row
may be empty when the output disappears in its own feature, including tool caps.
An inherited deleted meaning needs no row because its derivation remains in the
lineage graph. No separate tombstone field or codec section is needed.
Archive every physical entity once, map each name to a set of slots, and
rebind all slots on restore. A name may have several slots and several names
may share a slot; the old `BoundName -> one slot` restrictions cannot be reused.
A restored geometry-only B-Rep must refuse history queries, not answer empty.
Cache typed output-name ancestry only; the prototype's raw curve/end seed
aliases support its temporary legacy translator and are not stored references.
Input roots remain operands in the reconstructed derivation graph.

Set `ARCHIVE_CACHE_KIND` to `brep.generative.v2`. Discard old entries and cold rebuild;
no legacy reader, migration or mixed old/new tables. Key the whole entry by
model inputs, naming algorithm and kernel build. Missing names, mismatched
hashes, invalid incidence, kinds or slots invalidate the entire entry.
Restore requires the predecessor chain to have been restored or rebuilt first.
The constructors (`decode` and `archive_feature`) check local digests, node forms,
tags and ancestry rules exactly once. The immutable public record is valid by
construction; encoding and restoration do not repeat that local check. Live
insertion trusts digests computed by `intern`. Before publication, validate each
owned node's operands and every result name against the owned nodes plus the prepared model
graph. A missing operand, foreign lineage, duplicate owner, or invalid empty row
rejects the whole entry. No repeated validation walk of an inherited graph is
needed. Geometry and names are published together after the full incidence check and validation
of every solid member, including finite positive volume and absence of free lower dimensions.
Record determinism covers naming and incidence sections; B-Rep bytes may differ in
numeric representation, such as signed zero, while remaining geometrically equal.

Reject restoring geometry while silently omitting naming. The archive test
round-trips every plate entity and its complete name sets through actual OCCT
slots; restored B-Reps correctly refuse operation provenance. This establishes
the cache contract without adding a cache implementation to this experiment.

### 9. Cost determines the integration strategy

The benchmark separates cold rebuild, complete generative derivation and their
sum on the same persisted corpus, machine and session, with five samples and
no concurrent build or test. The comparison against fresh main includes the
bridge's additional provenance capture, not just Rust hash construction.

Choose eager naming of every elementary entity in each rebuilt feature, reusing
already named or restored predecessors. The largest corpus (250 feature results,
50 bodies) adds 83.639 ms of derivation; a plate adds 0.206 ms. Complete provenance
capture changes the 50-body rebuild from 552.880 to 560.937 ms; rebuild plus
naming is 644.836 ms (+16.6%). The 16-Cut history adds 18.247 ms of naming and
finishes in 100.984 ms against 82.566 ms on main (+22.3%).

Reject lazy naming in 08: opaque digest lookup must establish the complete
candidate set for exact cardinality, and archival needs complete typed bindings.
Avoid a second partially populated cache state for these measured costs.
A cache hit restores names together with geometry; it does not run the cold
naming pass. Dirty-feature scheduling remains task 12.

The paired release experiment uses fresh main `1adb65c`, the same persisted
corpus and native OCCT/PlaneGCS, five samples per version after one warmup, with
no overlapping builds/tests. The after measurement includes existing naming
plus this isolated prototype; it does not claim the cost of the future removal
of legacy maps. Median milliseconds:

| Document | Main rebuild | Full-history rebuild | Naming | Rebuild + naming |
|---|---:|---:|---:|---:|
| Plate | 0.189 | 0.193 | 0.206 | 0.400 |
| 16 Cuts | 82.566 | 82.826 | 18.247 | 100.984 |
| 10 bodies | 109.812 | 110.833 | 16.600 | 127.406 |
| 50 bodies | 552.880 | 560.937 | 83.639 | 644.836 |
| Four constrained Fillets | 9.818 | 10.030 | 1.558 | 11.628 |
| Partial Revolve | 0.745 | 0.748 | 0.202 | 0.950 |
| Chamfer | 2.382 | 2.323 | 0.429 | 2.748 |
| Open slot | 4.437 | 4.622 | 0.661 | 5.271 |
| Overlapping tools | 7.732 | 8.058 | 0.917 | 8.981 |
| Pocket | 2.009 | 2.030 | 0.453 | 2.482 |

#### Shared derivation graph measurements

Measure the shared graph against main `9f982b3e` in one session, with three samples
per completed long case and a 6144 MiB watchdog. The complete before/after tables,
archive sections and growth exponents are in
[PR #135](https://github.com/gesriot/ferrite-cad/pull/135). Each cell below is
main → shared graph; times are medians in milliseconds.

| History | Stored nodes | Naming ms | Cached ms | Archive MiB |
|---|---:|---:|---:|---:|
| pockets_100 | 93,930 → 1,780 | 142.572 → 68.370 | 200.229 → 131.298 | 18.674 → 10.149 |
| pockets_300 | 791,630 → 5,180 | 2323.485 → 653.507 | 1798.244 → 1142.470 | 158.447 → 86.210 |
| pockets_1000 | incomplete | incomplete | incomplete | incomplete |
| rework_100 | 93,930 → 1,780 | 27.920 → 9.396 | 96.564 → 16.941 | 10.865 → 0.943 |
| rework_300 | 791,630 → 5,180 | 211.337 → 27.356 | 769.404 → 50.097 | 87.700 → 2.802 |
| rework_1000 | 8,588,580 → 17,080 | 2355.368 → 96.949 | 9239.845 → 167.812 | 935.505 → 9.306 |

The rework history keeps constant live topology. Owned nodes, surviving ancestry
and archive bytes grow linearly; its 1000-Cut cache rebuild is faster than cold
(167.812 ms versus 5425.652 ms). In pockets, successive results contain more holes:
entity, binding, non-empty ancestry and incidence tables still accumulate with
live topology. Retaining intermediate B-Reps remains the evaluator policy.
At 1000 pockets, main stops during cold feature 955 and the shared graph stops
during naming feature 1000 at the memory limit; neither has a complete three-sample
result. Do not infer timing or growth exponents from those incomplete cases.

### 10. Integration and subsequent consumers

In 08 replace `SemanticRole`, `CarriedName`, role maps in `FeatureNames` and
the output `TopologyRef` catalogue with the derivation graph and per-feature
bindings. Consume the complete
kernel query API immediately after each operation and remove `track`, the old
face-only history DTOs and the cap/sweep output maps and queries. Operation
results become the common shape result; complete provenance remains a query
owned by that live kernel session. Replace `TopologyMap`
resolution/archival with the generic graph and set resolver. Delete the old
role codecs, `output_names.rs`, `same_meaning`, catalogue contracts and their
superseded tests in the same change. Regenerate fixtures directly.

Translate a swept edge by its two curve-generated faces, qualified at the
consumed predecessor. Translate caps/sides by their root ancestry, cap edges
and vertices by face intersections, Fillet/Chamfer faces by generated ancestry
of the consumed edge, and carried roles by the original producer's ancestry.
The experiment's independent translator compares these meanings with existing
session handles; 08 removes this translator together with the old roles, except the
corner-to-edge translation of `SweptEdge`.

Task 09 uses complete sweep/boolean provenance for Extrude/Revolve variants.
Task 10 makes Fillet/Chamfer payloads consume generic edge references instead of
a producer and a pair of profile Lines, submits resolved edge sets directly to
the kernel, and deletes `SweptEdge`, its corner translation and the corner-only
Fillet/Chamfer policies together. A `SweptEdge` names only a profile corner and
those policies read corners, so replacing the payload in 08 would need a reverse
translation from references back to corners. 11 consumes a face reference for
sketch support. None needs a document-shape
role, a geometric fallback, or a new naming rule.

Reject integrating only selected document combinations or retaining old roles
as a second production route. This experiment changes none of those consumers.
