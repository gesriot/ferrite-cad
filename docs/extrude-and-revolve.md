# Extrude and Revolve in the window

Select a built sketch in the Model tree and choose **Extrude…** or **Revolve…**.
Both commands use all closed regions of the sketch. A drawing without a profile
keeps the commands visible but disabled; their explanation gives the complete
reason from the displayed rebuild. Repair the drawing with **Edit sketch…**.

## Operation and body

**New body** creates a body and its first feature. **Add** adds material to the
selected body, **Cut** removes material, and **Intersect** keeps their common
material. These three choices require a body with a shown feature. The form names
that feature below the body selector. With a rollback marker, the new feature is
inserted after the shown feature and before the remainder of its history.

A body may contain several solids. The operation applies to its complete result;
other bodies do not participate.

## Extrude

- **Blind** extends by the entered **Distance (mm)**.
- **Symmetric** extends by that distance on each side of the sketch plane.
- **Through all** reaches through the selected body in the chosen direction. The
  existing modelling rules accept it for Cut and Intersect.
- **Up to next** ends at the next complete boundary of the selected body ahead of
  the profile. A missing or incomplete boundary produces a refusal.
- **Reversed** reverses the extrusion direction.

The distance is a number in millimetres, not an expression. It is unused for
Through all and Up to next. The form offers the end conditions without predicting
whether a particular operation can build; Apply reports the modelling refusal.

## Revolve

Choose **Sketch X**, **Sketch Y**, or a Line of the sketch, including construction
lines. Line labels show their solved start and end coordinates in millimetres, in
sketch order. The line defines an infinite directed axis.

**Full turn** stores a complete revolution. **Angle (°)** stores a partial turn;
enter a number in degrees, from 0.01° through 359.99°. Partial turns follow the
right-hand rule about the chosen axis. A crossing profile or another invalid
result is refused by the modelling operation.

## Apply, Cancel and Undo

**Apply** submits one transaction. New objects receive the smallest free numbered
names, such as `Body 1`, `Extrude 1` or `Revolve 1`. An accepted version closes the
form and displays the result; one document Undo reverses the addition.

A refusal leaves the draft open, displays the complete reason and changes neither
the accepted version nor the file. Correct the fields and apply again, or choose
**Cancel** to discard the draft without an edit. Switching tabs keeps the draft;
closing its tab or quitting asks about the unfinished form. Save writes accepted
changes to the document file. There is no preview while the form is open.
