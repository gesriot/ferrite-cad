# §30F — edit an existing circular Cut inside the open document

Marker `FCAD_30F_CUT_SESSION_APPLY`. [Verification and real-window recipe](cut-session-apply-verification.md).
The existing [Cut parameter edit](edit-circular-cut-copy.md), with its later
[1–16-link](circular-cut-history.md), [explicit end](circular-cut-through-all.md)
and [Line-polygon](polygon-cut-history.md) contracts, joins the
[document session](document-session.md).

After Apply height or Apply vertices, open **Edit cut … — UUID…** on the accepted
unsaved model. **Apply cut** reads the current valid centre, radius and Blind depth
or explicit ThroughAll without a Save dialog or draft confirmation. The selected
feature and tool Circle are named by UUID. Any supported link is one step in the
common document history. Undo/Redo can cross base and different Cut edits; exports
use the accepted unsaved snapshot. Only Save writes the user's file; Save As keeps
its previous file. An accepted new step after Undo drops the Redo branch.

**Confirm draft numbers**, and the form's Undo/Redo, affect only draft checkpoints.
The existing **Save cut copy…** workflow requires confirmed fields and a clean
document. Its disabled state explains unsaved changes. Add Cut remains the same
copy workflow and has no document Apply route.

`StepTicket::edit_circular_cut` pins the form version, reads the current accepted
snapshot and calls `edit_circular_cut_copy` with the existing
`EditCircularCutRequest` / `CircularCutEdit` / `CutExtent` / `ExtentVocabulary`.
The worker uses the existing lease, cleanup and two-phase scene acceptance. There
is no second writer, evaluator, serializer, SQLite route or history. One production
predicate serves the button and handler: the Edit form allows its own Apply;
Add/other forms, load/export and session work exclude it.

An unchanged numerical request is no document step, including alternate number
spellings. Unused depth text cannot change ThroughAll semantics. The independent
model comparison removes the unaccepted private file and keeps Redo/checkpoint.
The CLI still publishes its no-op copy.

Refusal, cancel, stale form/answer and failed scene/GPU preparation keep the accepted
scene, history/Redo, dirty/checkpoint, source bytes and exact draft. The document
outcome is readable inside the form. A successfully accepted scene dismisses the
form about its previous version.

All geometry and identity policy remains in the existing document/job owners:
real finite Line boundaries, all disk clearances, complete permitted history,
protected floor and Origin names and explicit ThroughAll intent. ThroughAll follows
height; Blind depths remain absolute. Blind-through ↔ ThroughAll and through →
pocket retain their rules. Pocket → through refuses with the protected UUIDs.
New floor/Origin refs belong only to the accepted snapshot; Redo restores precisely
those UUIDs without another job. Feature, Sketch, Curve and Body UUIDs, predecessors,
directions, other Cut parameters and other SQL cells retain the operation contract.

No new Cut through the session, feature deletion/reordering, Fillet/Chamfer Apply,
geometry class, format/capability/CLI/JSON change, plane, preview, autosave or tabs.
§30, Milestone 5C and the product remain open. The next slice has not started.
