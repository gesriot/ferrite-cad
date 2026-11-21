# 16. Solve viewport sketch constraints as draft edits

**Status:** accepted design.

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
Rebuild solves from stored coordinates. Predict its input with saved coordinates on surviving
curves, draft coordinates on new curves, and the final construction flags and constraint
list, including final dimension values. Use the same evaluator as the draft. Without
constraints the input itself is the prediction.

Write SetCurveGeometry only for surviving stored curves whose predicted position differs
from the preview by more than 1e-6 mm in any coordinate or radius. Solve again with those
writes if needed. Check every curve, including new curves, against the preview at the same
tolerance. If the solver settles elsewhere, keep the draft and its history; send no request.
The final prediction and the committed drawing have the same input and must agree within
1e-9 mm. Prediction runs on Finish only. Drop cancelled changes; an empty difference costs
no document version.

## Rejected alternatives

- Solving during painting or hover repeats work without changing the sketch.
- Waiting until Finish to solve hides both movement and conflicts while editing.
- Translating constraints to solver IDs in the window would duplicate the evaluator.
- Writing all solved coordinates at Finish would modify unrelated stored geometry.
- Turning a drawing snap into a constraint silently introduces design intent.
- Writing only directly released points misses curves connected through other constraints.
