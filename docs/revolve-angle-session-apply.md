# §30E — apply a saved partial Revolve angle inside the open document

Marker `FCAD_30E_REVOLVE_ANGLE_SESSION_APPLY`. [Verification and executable window
recipe](revolve-angle-session-apply-verification.md). This joins the existing
[angle editor](edit-revolve-angle.md) to the [document session](document-session.md).

Open a supported partial Revolve, optionally Apply vertices or Apply constraints,
then open **Edit Revolve angle** on that accepted, possibly unsaved document.
**Apply angle** uses the current valid field without a Save dialog. It is one step
in the same document history as the profile and constraints. Undo/Redo and unsaved
STL/FBX exports read the accepted version. Only Save changes the user's file;
Save As leaves the previous file intact.

**Confirm draft numbers**, **Undo draft** and **Redo draft** affect only the form's
bounded draft history. Document Apply needs no prior draft confirmation. Document
Undo/Redo wait until the form closes. **Save edited Revolve copy…** retains its old
workflow on a clean document, after draft confirmation; with unsaved changes it
is disabled with an explanation. Creation of Revolve still creates a document.

One production predicate, `can_apply_angle`, governs both the button and its
handler. The open angle form allows its own Apply. No session, no angle form,
wrong kind, another form, loading, export (including its configuration and
replacement question), creation worker or session worker blocks Apply.

The request carries the version the form was opened from. `StepTicket::edit_revolve_angle`
checks it against the current snapshot and calls **the existing**
`edit_revolve_angle_copy` through `StepTicket::run`. The worker uses the existing
`begin_apply` → `finish_apply` → scene preparation → commit/show route. There is
no second copier, evaluator, writer, serializer, SQLite route or history.

An unchanged numerical angle, including another spelling of the same number,
is no document step. The UI offers nothing to Apply; the session independently
compares model content, removes the temporary produced file and preserves Redo.
The CLI's unchanged no-op contract still publishes a copy.

A refusal, cancellation, stale form/answer or failed scene/GPU preparation leaves
the accepted model, scene, history/Redo, dirty/checkpoint, file and draft intact.
The common document status is drawn **inside the form**. A successfully accepted
replacement scene dismisses every form about the previous version.

All previous geometry rules remain: partial angles 0.01–359.99°, no normalization,
360° refused, full turn has no partial-angle editor. Radial-clear and axis-closed
profiles retain their existing class checks. Unconstrained edits need OCCT but
no solver; constrained edits require PlaneGCS and refuse honestly without it.
Feature, Sketch, Line and Body UUIDs, direction, axis, `axis_segment`, both
RevolveCap names, all other refs, payload schemas and capabilities stay intact.
Stored constrained coordinates remain the solver's initial approximation.

No full/partial conversion, new document class, live preview, Cut/Fillet/Chamfer
session actions, autosave, tabs or format/CLI/JSON changes. §30, Milestone 5C and
the product remain open.
