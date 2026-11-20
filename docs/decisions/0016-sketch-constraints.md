# 16. Solve viewport sketch constraints as draft edits

**Status:** proposed; the Finish rule for released geometry needs a decision.

## Selection and rules

Select points, curves and constraints in the drawing. Within the curve pick radius, give
points priority: a stored line start or end, a circle centre, or a standalone point. Arc
ends and centres are snap targets but cannot name solver points. Shift toggles membership;
retain selection order because an angle from A to B is not an angle from B to A.

Use one answer per button for both the proposed rule and the reason it is unavailable.
Coincident needs two points; Horizontal, Vertical and Distance need two points or one
line's ends. Parallel, Perpendicular, Equal and Collinear need two lines. Fix pins one
point at its current solved position. A circle supplies Radius and Diameter; two lines in
selection order supply a counterclockwise DirectedAngle in (−180°, 180°].

## Dimensions and feedback

Measure new dimensions on the current draft. Show lengths in mm and angles in degrees;
store a diameter as half its displayed value. Set and Enter validate the number with the
existing expression-slot rules. Show a saved parameter expression as read-only with
`This value is an expression; edit it in Parameters.` Never replace its source with a literal.

Start at the accepted drawing. Solve a proposed constraint addition, removal or value change,
or deletion of a curve with constraints, through the evaluator's existing sketch translation
and solver. Change draft geometry and append one history step only on success. Keep the
solved drawing and report in history: Undo and Redo restore them without solving. Report the
degrees of freedom and redundant rules. A refusal keeps the previous state, history and
selection; show the complete error and the named constraints of a typed conflict. Without
a solver, disable constraint and dimension buttons; drawing and explicit removal of a deleted
curve's constraints remain possible.

Draw markers and dimension labels through the curve projection using the existing canvas
vocabulary and box styles. A click on a marker or label selects its constraint. Highlight it
like a selected curve. Delete, Backspace and the Delete button remove selected constraints
and curves in one step, including any rules that reference a deleted curve. The form reports
how many constraints remain and how many were added, removed and changed.

## Finish

Use the existing session transaction. For a new sketch send one AddSketch with solved curves
and rules (after adding a datum if necessary). For an existing sketch, remove constraints,
remove curves, add curves, write moved surviving curves, set construction flags, add rules,
then set changed constant expression slots. Additions retain creation order and identities.
Compare each surviving curve with its **accepted solved geometry** using a 1e-9 mm tolerance;
leave an unmoved curve's saved coordinates alone. Drop cancelled changes. An empty difference
costs no document version. The accepted drawing must agree with the preview within 1e-6 mm.

## Open decision: releasing a constraint without moving a curve

A stored point at (0, 0) may have been shown at (10, 0) because of a Fixed rule. Remove
that rule while leaving an independent Fixed rule on another point. Starting the draft
solve at the accepted positions leaves the released point at (10, 0). It has not moved
relative to the accepted drawing, so the rule above emits only RemoveConstraint. On
rebuild the free point starts from its stored (0, 0) and stays there. Both solves succeed.

Writing the released point's draft geometry alongside RemoveConstraint preserves the preview
in this example. That would require an exception to the accepted-position comparison. The
alternative is to preview the actual stored-geometry request and show the point moving to
(0, 0) as soon as the rule is removed. These give different editing behaviour; neither is a
change to the solver or the file format.

## Rejected alternatives

- Solving during painting or hover repeats work without changing the sketch.
- Waiting until Finish to solve hides both movement and conflicts while editing.
- Translating constraints to solver IDs in the window would duplicate the evaluator.
- Writing all solved coordinates at Finish would modify unrelated stored geometry.
- Turning a drawing snap into a constraint silently introduces design intent.
