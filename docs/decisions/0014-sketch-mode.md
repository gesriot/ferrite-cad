# 14. Edit a sketch drawing in the viewport as one form

**Status:** accepted design.

The viewport sketch editor is a saved-object form owned by `sketch::Editor`.
Its open state participates in `saved_object_open()`, so command availability,
tab parking, stale-draft refusal, Close and Quit use their existing rules. An
accepted session change closes the form. Cancel discards it without a transaction.
Drafts remain in memory and are not crash recovery data.

## Entry and accepted facts

Offer Edit sketch on a Sketch with a presentation in the accepted scene, and New
sketch on a DatumPlane. The Model panel also offers New sketch under the same
command availability answer. Existing editors and their recognizers remain intact.
New drawings choose a stored datum or an absent standard XY, XZ or YZ plane.
The plane may change while the draft is empty. Reuse an exactly matching fixed
plane; otherwise Finish creates the chosen standard plane before its sketch.

An existing draft begins with the accepted presentation's identities, geometry,
construction flags and resolved plane. These are the solved coordinates the
profile reader saw. Keep stored geometry separately for computing changes: an
untouched curve's solved coordinates never become a saved coordinate edit. Carry
resolved datum planes from the same rebuild to the scene; do not evaluate planes
or solve constraints in the window.

Entry and Look at sketch orient the camera along the plane normal with its Y axis
up, retaining camera target and distance. Leaving the form keeps that camera.
Intersect the pixel ray and sketch plane in f64. An almost parallel ray adds
nothing and reports `The view is edge-on to the sketch plane.` Unsnapped
coordinates are not rounded.

## Drawing and input

Hide only the edited sketch's accepted GPU drawing while the form is open.
Upload the changed drawing set on entry and exit, never on a frame or draft edit.
Other drawings and bodies remain. Paint the draft, selection, hover, rubber curve
and snap marker over the viewport in screen space, using the same curve projection
for painting and hit testing. Construction curves are thinner and paler.

Keep ViewportInput's click and drag distinction. Drags still orbit, pan and zoom.
In the form, consume clicks for its tool, suppress entity hover and context menus,
and route Delete, Backspace and history keys to the draft. Outside it, input
retains its existing routes.

Select replaces the curve selection; Shift toggles membership and an empty click
clears it. Line creates a polyline with identical adjoining f64 endpoints. A snap
to its first point closes it; its current point or Escape ends it without adding
a zero-length curve. A second Escape selects Select. Circle takes a centre and a
radius point, refusing a zero radius. Point takes one click. Existing Arcs can be
selected, deleted and have construction toggled, but have no creation tool.

Snap to the nearest projected line/arc end, circle/arc centre or Point within a
pixel threshold. Targets include draft and accepted solved coordinates. Substitute
the exact target coordinates without adding constraints. Grid and direction snaps,
dimensions, constraint creation and dragging geometry belong to later work.

## Draft history and publication

One added curve, one selected-set deletion or one construction toggle is one draft
Undo step. Redo restores it. The form's history keys never move document history.
New curves receive caller-owned UUIDv7 identities when created. A deletion records
the saved constraints referring to deleted curves and reports their count; the
kernel does not remove constraints implicitly.

Finish builds one ordinary EditRequest. For an existing sketch, list explicit
RemoveConstraint changes first, then RemoveCurve, AddCurve in creation order and
SetConstruction for changed surviving saved curves. Omit curves added then removed
and flags toggled back. An empty difference closes without applying anything.
For a new sketch, add the absent standard datum if needed, then AddSketch with all
draft curves. The name is the smallest free `Sketch n` among document object names;
an empty new sketch is valid.

The owner Sessions::apply remains the only publication path: one accepted version
and one document Undo step. A refusal keeps the draft and its history open, shows
the complete owner diagnostic and leaves the accepted file/version unchanged.
No modelling validation, transaction rule or file format changes here.

## Rejected alternatives

| Alternative | Reason rejected |
| --- | --- |
| A separate mode outside the forms | Duplicates command availability, tab parking and Close/Quit ownership. |
| Upload the draft on each frame or edit | Violates the renderer's resident-drawing contract. |
| Commit each curve separately | Creates many document versions and Undo steps for one drawing. |
| Add Coincident constraints automatically when snapping | Turns a coordinate choice into constraint intent without a request. |
