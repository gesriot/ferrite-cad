# 18. Retire the 2D canvas forms

**Status:** accepted design.

## Decision

Create a document with **New…**, draw on a plane with **New sketch…**, then apply
**Extrude…** or **Revolve…** to the finished sketch. This replaces the drawing window
that could create a polygon, circle or ring and its solid in one operation.

Retire **Edit vertices…**, **Edit circle…** and **Edit annulus…** in the second change.
Use **Edit sketch…** for built drawings. Keep the saved Revolve angle editor, including
its draft history and copy workflow.

Retire **Edit constraints…** in the third change. Select points, curves, markers and
dimension labels in the viewport and use the sketch constraint panel. The CLI continues
to accept versioned edit requests. Each removal includes its entry points, unused
code and form-specific tests. Shared session, publication and file checks follow the
surviving commands.

## User-visible changes

| Retired interaction | Available route |
| --- | --- |
| Repair a failed sketch from its stored rows without a drawing | Undo the breaking edit or remove the offending constraint through CLI `edit`. |
| Type arbitrary unconstrained coordinates and radii | Use the grid for placement, Distance, Radius or Diameter for dimensions, and Fix for a point. |
| Resize a saved circle by dragging its rim | Set a Radius or Diameter dimension. Without the solver these dimensions are unavailable; until rim dragging is implemented, change the radius through CLI `edit`. |
| Drag both ends at a polyline corner as one vertex | Select both endpoints and make them Coincident to move them together. |
| Offset from a midpoint or intersection using a typed temporary length | Viewport guides start at the preceding vertex or the point at the press. |
| Disable only snapping to objects | A nearby object takes priority over guides and grid. |
| Type a new X/Y when fixing a point | Fix holds the point at its current position. |
| Reverse either line vector when creating a directed angle | CLI `add_constraint` accepts directed references. Select an existing angle in the viewport to set its value or delete it. |
| Pick an obscured rule from the complete list of curves and constraints | Select geometry, markers and labels in the viewport. |
| Stage a batch containing an intermediate unsolvable set of rules | Each viewport rule edit solves immediately. Submit a complete batch through CLI `edit`. |
| Replace a dimension by removing its rule and adding another | **Set** changes the value and preserves the constraint UUID. |
| Restore all saved vertices in one undoable draft step | Undo draft steps or Cancel the draft. |
| Write a sketch copy while keeping the open tab at its original revision | Save As before editing or use CLI `edit … -o COPY`. Numeric feature editors retain their copy workflow. |
| Keep three alternative drawing drafts and publish sketch and solid together | Empty document, finished sketch and applied sweep have separate accepted versions and Undo steps. |
| Keep an independent fitted 2D view for each draft | Sketch mode uses the tab camera and offers **Look at sketch**. |
| Cancel a pan with Escape to restore the view, or end it by leaving the window or starting a job | A pan ends on release wherever the pointer is, or on focus loss; the view stays where it ended. |
| A constraints form returned on another version hid that version's drawing | Sketch mode keeps a held draft visible, like other held forms keep their values; Finish is refused with the reason, and Cancel shows the accepted sketch |
| Changing a stored dimension's value in the window solved from the stored coordinates, as a CLI edit of the value alone does | The window solves from the drawing it shows and saves that drawing (ADR 0016); CLI `edit` with the value alone still solves from the stored coordinates |

## Known gaps

A separate task covers opening an unbuilt sketch in sketch mode, Coincident
constraints at the joins of lines drawn as a polyline, and dragging a circle rim
to change its radius.

[Viewport drawing](0014-sketch-mode.md), [sweep commands](0015-sweep-commands.md),
[constraints](0016-sketch-constraints.md) and [dragging](0017-sketch-drag.md)
define the surviving interactions.
