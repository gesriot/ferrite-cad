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

For a ring, draw two circles with a common centre. Select both centres and choose
**Coincident** to keep them concentric. Extruding the finished sketch gives a solid
with a hole.

Turn a finished sketch into a body with [Extrude… and Revolve…](extrude-and-revolve.md).

## Tools and selection

- **Line** starts a polyline. Each further click adds a side. Click its snapped
  start to close it, or its current endpoint to finish without another side.
  Escape ends the polyline; a second Escape returns to Select.
- **Circle** takes a centre and a point on its circumference. Escape cancels a
  pending circle. A zero radius adds nothing.
- **Point** adds a point at each click.
- **Select** picks a curve, point, or constraint label. Near a line end, circle centre
  or standalone point, the point takes priority over the curve. Shift-click adds to or
  removes from the selection. At a shared corner, Shift-click can pick the other line's
  end. Clicking empty space clears the selection.
- The **Construction** checkbox controls new curves. The **Construction** button
  makes the selected curves construction curves, or makes them main curves when
  all are already construction curves.
- **Delete**, Delete and Backspace remove the selected curves and constraints. The form lists how
  many of their constraints Finish will explicitly remove as well.

Existing arcs can be selected, deleted and switched between main and construction
geometry. There is no arc drawing tool. A click in the drawing does not select a
body entity or open its context menu while the form is open.

The draft appears over the model. Construction curves are thinner and paler. Selection and
hover use distinct colours.

## Dragging and snapping

In **Select**, drag a line end, circle centre or standalone point. A click still selects it.
Dragging elsewhere orbits the view. Without constraints, only the handle you pressed moves,
even if another curve has an endpoint at the same coordinates. To move a corner as one vertex,
select both endpoints and make them **Coincident**. With constraints, the solver
moves the drawing to the reachable position closest to the pointer. A horizontal line with
one fixed end lets its other end slide horizontally. A fixed point stays still. If a sample
fails, the form shows the reason and keeps the last successful position. Dragging a constrained
sketch requires a build with the sketch solver.

Release to record one Undo step. Escape or loss of window focus restores the drawing at the
press. Returning to the starting drawing leaves the history intact. Finish saves the displayed
drawing using the same prediction as other sketch edits.

The snap marker shows the coordinate used for drawing or dragging. A nearby endpoint, centre or
standalone point has first priority. **Guides** can align a line to its last vertex or a drag to
the position at the press. Choose **Orthogonal** for horizontal and vertical directions or
**Polar 45°** to include diagonals. Away from an object or guide, **Grid** rounds both sketch
coordinates to the selected step. With both controls off, a point away from objects uses the
pointer's exact sketch coordinates. Snaps add no constraints. Closing the form clears both choices.

## Constraints and dimensions

Select two points for **Coincident**, or a single point for **Fix**. **Horizontal**,
**Vertical** and **Distance** accept either two points or one line. Select two lines
for **Parallel**, **Perpendicular**, **Equal**, **Collinear** or **Angle**. The angle runs
counterclockwise from the first selected line to the second. Select a circle for **Radius**
or **Diameter**. A disabled button explains the selection it needs. Arc ends and centres
can be snapped to but cannot be used as constraint points.

A new dimension starts at its measured value. Click its label to select it. Type a
**Value** in millimetres or degrees and press Enter or **Set**. Diameter shows the full
width; the stored rule is a radius. A dimension driven by an expression shows its
source instead: edit it in **Parameters**. Click a marker to select a geometric rule.
The selected marker or dimension is highlighted like a selected curve.
A new value moves the drawing from its displayed position, so a sketch with free degrees of
freedom can settle differently from a value-only CLI edit that starts at the stored coordinates.

To change a radius without retaining a dimension, set **Radius**, then delete the
dimension before **Finish**; this requires the sketch solver.

Each rule change solves the draft immediately. The panel reports remaining degrees of
freedom and redundant rules. If the solver refuses, the message and any conflicting
rules appear below the controls; the drawing and its Undo history stay as they were.
Without a sketch solver, the constraint buttons are unavailable. Drawing and deleting
curves, including their referenced rules, still work.

## Finish, Cancel and Undo

**Undo** and **Redo**, including the usual document history shortcuts, change only
the draft while the form is open. One curve addition, deletion of the selection,
construction toggle, constraint addition or value change is one draft step.

**Finish** sends all changes as one ordinary document transaction and one document
Undo step. Finish predicts the rebuilt sketch from its saved coordinates. It writes
coordinates only where needed to keep the shown drawing, including curves freed by
removing a constraint. If it cannot reproduce the preview, it leaves the form open
and shows `Finish would not keep the drawing as shown; the solver settles elsewhere.`
Finishing an unchanged existing sketch closes the form without a version; finishing a
new empty sketch creates it.

If the rebuild refuses a change, the form shows the full reason and keeps the
draft and its Undo history. For example, removing a side from a profile used by a
built Extrude cannot leave that Extrude unbuilt. **Cancel** discards the draft
without changing the document. Switching tabs parks the draft. If the tab has accepted another
version when you return, the draft stays visible with **Finish** disabled until you **Cancel**
it and reopen the sketch. Closing the tab or quitting asks about the draft like any other open
form. Drafts are not stored on disk.
