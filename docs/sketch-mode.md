# Draw a sketch in the viewport

Choose **Edit sketch…** on a built sketch in the Model tree, or **New sketch…** on a
plane. The **New sketch…** button above the tree also opens an empty drawing. For a
new sketch, choose any document plane or an available XY, XZ or YZ plane. The plane
can change while the draft is empty. Finish creates a missing standard plane with
the sketch, using the smallest unused name `Sketch n`.

The camera looks along the sketch normal with its Y axis upwards. **Look at
sketch** restores this view; orbit, pan and zoom work as usual. Drawing from an
edge-on view does not add a curve and shows an explanation. Leaving the form
keeps the camera where it is.

## Tools and selection

- **Line** starts a polyline. Each further click adds a side. Click its snapped
  start to close it, or its current endpoint to finish without another side.
  Escape ends the polyline; a second Escape returns to Select.
- **Circle** takes a centre and a point on its circumference. Escape cancels a
  pending circle. A zero radius adds nothing.
- **Point** adds a point at each click.
- **Select** picks a curve or point. Shift-click adds or removes it from the
  selection; clicking empty space clears the selection.
- The **Construction** checkbox controls new curves. The **Construction** button
  makes the selected curves construction curves, or makes them main curves when
  all are already construction curves.
- **Delete**, Delete and Backspace remove the selected curves. The form lists how
  many of their constraints Finish will explicitly remove as well.

Existing arcs can be selected, deleted and switched between main and construction
geometry. There is no arc drawing tool. A click in the drawing does not select a
body entity or open its context menu while the form is open.

The draft appears over the model. Construction curves are thinner and paler;
selection and hover use distinct colours. Near line or arc endpoints, circle or
arc centres, and points, a marker shows the nearest snap target. A click copies
that target's exact coordinates. Snapping adds no constraint. Elsewhere,
coordinates are neither rounded nor snapped to a grid.

## Finish, Cancel and Undo

**Undo** and **Redo**, including the usual document history shortcuts, change only
the draft while the form is open. One curve addition, deletion of the selection,
or construction toggle is one draft step.

**Finish** sends all changes as one ordinary document transaction and one document
Undo step. Unchanged curves retain their saved coordinates, even when the drawing
shows their solved coordinates. Finishing an unchanged existing sketch closes the
form without a version; finishing a new empty sketch creates it.

If the rebuild refuses a change, the form shows the full reason and keeps the
draft and its Undo history. For example, removing a side from a profile used by a
built Extrude cannot leave that Extrude unbuilt. **Cancel** discards the draft
without changing the document. Switching tabs parks the draft; closing its tab or
quitting asks about it like any other open form. Drafts are not stored on disk.
