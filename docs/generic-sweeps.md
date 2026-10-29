# Extrude and Revolve

Both sweeps support NewBody, Add, Cut and Intersect. NewBody has no predecessor;
other operations consume the saved `previous` feature. The kernel combines the target
and the swept tool with Fuse, Cut or Common. A result must contain exactly one valid
solid. Empty intersections, disconnected additions and cuts splitting the target are
feature failures. A Cut that removes no material also fails.

Extrude supports Blind and Symmetric in either normal direction for all four operations.
Symmetric extends the specified distance on each side of the sketch plane.
ThroughAll applies to Cut and Intersect: the target's bounds projected onto the sweep
direction determine the far end plus a named 1 mm margin. The sketch plane may differ
from the body's first plane. A target entirely behind the plane refuses with “nothing
ahead”. NewBody and Add refuse ThroughAll in validation. Cached rebuilds use the target
key and saved intent without asking the kernel for bounds.

Revolve supports full and partial turns around SketchX, SketchY or any main or
construction Line in its profile sketch. A Line axis is infinite, directed from start
to end; positive partial angles follow the right hand rule. Profiles contain closed Line/Arc
chains and Circles, grouped into regions by even/odd nesting. The kernel decides axis contact
and rejects invalid geometry. There is no persisted axis segment or profile class.
A profile edge collapsing on the axis produces no face and receives no invented face
name. Complete sweep and boolean provenance follows ADR 0007; ambiguity refuses an
exact consumed selection.

[Edit requests](edit-transactions.md) add sweeps, create bodies and change operation,
direction, axis or extent through the shared transaction. An edit is accepted only
when no previously built feature becomes failed or skipped. Undo/Redo uses this same
candidate and rebuild path. Unconsumed topology may disappear; references are never
silently retargeted. [Fillet and Chamfer](fillets-and-chamfers.md) consume these names without profile
classification or corner policies.

The existing create commands are convenience templates. `create-sketch-revolve` uses
local SketchY; the generic `edit` request supports all axes. Existing window forms
continue to create and edit their saved features through the shared library; there are
no additional generic sweep forms. Constraint recipes remain in
[Revolve profiles](revolve-profile-constraints.md) and
[axis-contact profiles](axis-closed-revolve-constraints.md).

The file format is unstable. Revolve stores `previous`, `operation`, `axis` and `extent`;
fixtures must be regenerated. There are no migrations or compatibility fallbacks.

## Sketch regions

Every main curve belongs to one closed boundary. Construction geometry is excluded.
Disjoint boundaries use even/odd nesting: an outer boundary and its immediate holes
form a region; an island inside a hole forms another region. Intersecting, touching,
self-intersecting and open boundaries fail with the offending curve identities.
Line/Arc/Circle intersections and ray parity are analytic, without tessellation or
sampled containment. The existing endpoint join tolerance is unchanged.

Extrude and Revolve store `regions: Vec<StableEntityId>`. Empty selects every region.
Each entry identifies a curve on a selected region's outer boundary; multiple entries
on one outer boundary select it once. Missing or construction curves and curves on
holes refuse by identity. Reordering, moving or resizing a contour does not retarget
its selection. The sweep produces one face per selected region, with complete curve
history. NewBody and the final boolean result must still contain exactly one solid.
A Cut or Intersect tool may contain several solids.

The edit JSON feature for `add_feature` or `add_body` accepts `"regions": ["curve-uuid"]`
for either sweep. Omitting this CLI field selects all regions. To replace an existing
selection use `{"kind":"set_regions","object":"feature-uuid","regions":["curve-uuid"]}`;
`regions: []` selects all again. The stored field is required: regenerate fixtures,
without a document migration or version branch. Cache keys include the selected
profile; the `brep.generative.v1` archive layout is unchanged. Existing window forms
select all regions and continue through the shared transaction.
