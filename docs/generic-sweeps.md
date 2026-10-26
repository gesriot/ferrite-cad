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
to end; positive partial angles follow the right hand rule. Supported profiles are the
existing closed Line/Arc contour, Circle or annulus. The kernel decides axis contact
and rejects invalid geometry. There is no persisted axis segment or profile class.
A profile edge collapsing on the axis produces no face and receives no invented face
name. Complete sweep and boolean provenance follows ADR 0007; ambiguity refuses an
exact consumed selection.

[Edit requests](edit-transactions.md) add sweeps, create bodies and change operation,
direction, axis or extent through the shared transaction. An edit is accepted only
when no previously built feature becomes failed or skipped. Undo/Redo uses this same
candidate and rebuild path. Unconsumed topology may disappear; references are never
silently retargeted. Fillet, Chamfer and SweptEdge retain their current policies.

The existing create commands are convenience templates. `create-sketch-revolve` uses
local SketchY; the generic `edit` request supports all axes. Existing window forms
continue to create and edit their saved features through the shared library; there are
no additional generic sweep forms. Constraint recipes remain in
[Revolve profiles](revolve-profile-constraints.md) and
[axis-contact profiles](axis-closed-revolve-constraints.md).

The file format is unstable. Revolve stores `previous`, `operation`, `axis` and `extent`;
fixtures must be regenerated. There are no migrations or compatibility fallbacks.
