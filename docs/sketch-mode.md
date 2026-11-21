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

The draft appears over the model. Construction curves are thinner and paler;
selection and hover use distinct colours. Near line or arc endpoints, circle or
arc centres, and points, a marker shows the nearest snap target. A click copies
that target's exact coordinates. Snapping adds no constraint. Elsewhere,
coordinates are neither rounded nor snapped to a grid.

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
removing a constraint. If it cannot reproduce the preview it leaves the form open. Finishing an unchanged existing sketch closes the
form without a version; finishing a new empty sketch creates it.

If the rebuild refuses a change, the form shows the full reason and keeps the
draft and its Undo history. For example, removing a side from a profile used by a
built Extrude cannot leave that Extrude unbuilt. **Cancel** discards the draft
without changing the document. Switching tabs parks the draft; closing its tab or
quitting asks about it like any other open form. Drafts are not stored on disk.
