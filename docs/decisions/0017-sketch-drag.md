# 17. Drag sketch points and snap targets in the viewport

**Status:** accepted design.

## Who owns the gesture

In Select, use the existing point-selection hit test at the primary press position.
A line end, circle centre or standalone point captures the press. One decision at
press time gives either the sketch or the view ownership of the gesture. Moving
beyond the viewport's click threshold starts a drag and preserves the selection.
Releasing within the threshold performs the usual selection click.

A press on a curve body, circle rim, arc, constraint label or empty space keeps
the existing click and orbit behaviour. Line, Circle and Point keep their
drawing gestures. Middle and secondary button gestures retain their bindings.

## What moves

With no constraints, move only the named point to the snapped target. Two ends
can share coordinates and still move independently. A Coincident constraint
expresses the intent to keep them together.

With constraints, the evaluator exposes a sketch drag that takes the sketch,
its object identity and a document point reference. It uses the same translation
of points, curves, constraints, solutions and refusals as the ordinary sketch
solve. Start one solver gesture when the pointer first crosses the click
threshold. For each frame with movement, submit the most recent target once.
Hover and frames without movement do no solver work.

On success, show the solved coordinates of all curves, including neighbours of
the grabbed point. Retain the draft's degrees of freedom and redundant constraint
report throughout the drag. On refusal, keep the last successful drawing and
show the full diagnostic, using the constraint labels for a conflict. A later
success clears the diagnostic. A refusal at the start leaves the drawing and
history intact. The evaluator uses the same missing-component error as the
ordinary solve. The form explains it with
`This build has no sketch solver, so constrained drawings cannot be dragged.`

## The drag target in PlaneGCS

Give the gesture one temporary pin with the PlaneGCS temporary constraint tag.
The point moves as close as possible to the target in a least-squares solve
subject to the sketch constraints. A horizontal line with one fixed end lets
the other end slide along the line. A fixed point stays where it is and the
gesture succeeds without a history step. Caller constraints keep their tags.
Only their residuals determine whether the result is a solution. The pin
contributes neither a degree of freedom nor a conflict or redundancy to the
sketch diagnosis.

## History and Finish

Keep the pre-press drawing for cancellation. Escape or loss of focus restores
it. Releasing a changed drawing records one draft history step. Returning the
drawing to its starting state costs no step. Undo and Redo restore the recorded
drawing and report directly. Finish uses the stored-coordinate prediction from
ADR 0016. Only Finish publishes a document edit.

## Choosing coordinates

For a drawing click or drag sample, take the first available target in this order.

1. Use the existing object snap targets and their exact sketch coordinates.
   Exclude the grabbed point by identity during its own drag.
2. Project onto the held guide. Line uses its current segment start as the
   anchor. A drag uses the point's position at press time. Circle and Point
   have no guide anchor. Share the canvas's guide acquisition, including
   aperture, minimum reach, competition and retention. Measure screen points
   per millimetre at the anchor with the projection used to draw the curves.
   A horizontal guide preserves the anchor's Y exactly and a vertical guide
   preserves its X. Draw the held direction through the anchor.
3. Round the sketch-plane coordinates to the selected grid step with the
   canvas's shared rounding rule. Round halves away from zero. For tenths of
   a millimetre, multiply by ten, round, then divide by ten.
4. Use the unrounded point on the sketch plane.

Grid offers Off, 0.1 mm, 1 mm, 5 mm and 10 mm. Guides offers Off, Orthogonal
and Polar 45°. Both start Off each time the form opens. The snap marker shows
where the next click will land. The viewport draws the held guide and marker
without a grid overlay or guide-length lock. Snapping changes coordinates only.

## Rejected alternatives

- Solving at every pointer event repeats work for intermediate positions the
  viewport never displays.
- Creating constraints from snaps or guides silently makes a transient
  coordinate choice govern future edits.
- Welding coincident endpoints gives independent stored points the behaviour of
  a Coincident constraint the user never added.
- Translating sketch entities a second time in the window would give diagnostics
  and solved coordinates two different mappings back to the document.
- Moving constrained geometry without a solver could display a drawing that
  violates its own constraints.
- A hard pin makes off-line targets infeasible for a point limited to a line.
- Projecting the target in the window would duplicate the solver’s constraint
  interpretation and would only handle selected geometries.
