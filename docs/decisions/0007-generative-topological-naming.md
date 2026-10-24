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

Delete `fallback_signature` in 08. Reject preserving it as a nearest-neighbour
or best-score escape hatch. The prototype refuses the open slot's ambiguous
bindings even though their positions could distinguish them.

### 5. Store consumption, derive production

Choose to store only references in consuming payloads: Fillet/Chamfer edge
selections now, sketch support faces and axes later. Rebuild derives output
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
topological identity, not a geometric match. Filter history to the actual final
solid, not discarded intermediate results.

Reject reconstructing provenance from a mesh, rerunning an operation on demand
or inferring unreported history. Default trait implementations and the mock
refuse the new queries with Unsupported. Old result DTOs and tracked queries
retain their contracts so the prototype cannot change evaluator decisions.

Determinism concerns the derivation graph, not session handles. Sort and dedupe
name operands, preserve multiplicity as a set of bindings and never serialize
traversal positions. Repeated cold rebuilds of the corpus produce identical
name sets and cardinalities.

### 8. Cache the B-Rep and its naming graph atomically

Choose one derived cache entry containing the B-Rep, complete name-to-slot
sets, ancestry sets, incident-face relations and explicit removed meanings.
Archive every physical entity once, map each name to a set of slots, and
rebind all slots on restore. A name may have several slots and several names
may share a slot; the old `BoundName -> one slot` restrictions cannot be reused.
A restored geometry-only B-Rep must refuse history queries, not answer empty.
Cache typed output-name ancestry only; the prototype's raw curve/end seed
aliases support its temporary legacy translator and are not stored references.
Input roots remain operands in the reconstructed derivation graph.

In 08 replace `archive.rs`/`codec.rs`'s role-specific payload and set
`ARCHIVE_CACHE_KIND` to `brep.generative.v1`. Discard old entries and cold rebuild;
no legacy reader, migration or mixed old/new tables. Key the whole entry by
model inputs, naming algorithm and kernel build. Missing names, mismatched
hashes, invalid incidence, kinds or slots invalidate the entire entry.

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

### 10. Integration and subsequent consumers

In 08 replace `SemanticRole`, `SweptEdge`, `CarriedName`, role maps in
`FeatureNames` and the output `TopologyRef` catalogue with the derivation graph,
compact consumed references and per-feature bindings. Consume the complete
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
session handles; 08 removes this translator together with the old roles.

Fillet/Chamfer payloads consume generic edge references instead of a producer
and a pair of profile Lines. Task 09 uses complete sweep/boolean provenance for
Extrude/Revolve variants; 10 submits resolved edge sets directly to the kernel;
11 consumes a face reference for sketch support. None needs a document-shape
role, a geometric fallback, or a new naming rule.

Reject integrating only selected document combinations or retaining old roles
as a second production route. This experiment changes none of those consumers.
