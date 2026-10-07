# §30G — edit an existing Fillet radius inside the open document

Marker `FCAD_30G_FILLET_RADIUS_SESSION_APPLY`.
[Verification and real-window recipe](fillet-radius-session-apply-verification.md).
The existing [radius edit](edit-fillet-radius.md) and its current
[1–4 Fillet history](rectangular-fillet-history.md) join the
[document session](document-session.md).

After unsaved height, base vertices or constraints Apply, **Edit Fillet radius
… — UUID…** opens on the accepted snapshot. **Apply radius** reads the current
valid text and changes that selected existing feature without a file dialog or
prior confirmation. Every supported Fillet is one step in the common history.
Undo/Redo crosses base and different radius edits; exports read the accepted
unsaved snapshot. Only Save changes the logical file. Save As keeps its prior
file. A successful new step after Undo truncates Redo.

**Confirm draft number** only confirms the text for **Save radius copy…**. The
radius form acquires no draft history. Clean copy remains available, including a
numerical no-op; dirty copy is disabled with the reason in the form and guarded
again before the dialog. Add Fillet remained the previous copy operation in this slice;
since [§30J](add-fillet-session.md) its form has its own document route, **Add fillet**,
and never goes through this Apply radius. Chamfer remained copy-only in this slice;
[§30H](chamfer-distance-session-apply.md) later applies an existing Chamfer distance.

`StepTicket::edit_fillet_radius` checks the form's `DocumentVersion`, then calls
`edit_fillet_radius_copy` using the existing `EditFilletRadiusRequest`, current
accepted snapshot, exact chosen feature UUID and next private path. Its worker
owns the kernel. The existing copier, evaluator, lease, cleanup and two-phase
scene acceptance are reused. No second history, writer, schema, capability,
request/CLI/JSON version or geometric algorithm is introduced.

One production predicate controls the button and handler. The radius form permits
its own Apply; Add Fillet, any other form, load/export/edit and session work exclude
it. A numerically unchanged radius, including another string spelling, offers no
Apply and creates no history step. Model comparison independently discards a
worker no-op and releases the unaccepted private file without losing Redo or the
checkpoint. The CLI still publishes its no-op copy.

Domain/solver refusal, cancellation, stale form/answer and failed scene/GPU
preparation keep the accepted scene, history/Redo, dirty/checkpoint, file and
exact typed draft. The document outcome is readable inside the open form.
Successful scene acceptance closes the form about the previous version.

The radius operation changes only the selected Fillet payload/hash and
`meta.modified_at`. Body, Sketch, Curve and every Fillet UUID, previous/BodyTip,
all refs including every `OriginFilletFace`, other radii/corners, stored constrained
approximation and every other SQL cell remain unchanged. UI/CLI comparison maps
no identities. Selection follows UUID, including when names repeat and SQL rowids
and ordinals disagree with feature history.

All bounds stay in the existing domain/evaluator: minimum, selected corner,
every earlier/later adjacent Fillet, history-ordered `pair_bound` /
`pair_bound_of_first` and both neighbours of the fourth corner. Constraints use
the real solved rectangle with the saved Line UUIDs, never the stored guess's
length. Rounding order, strict refs and typed UUID-bearing refusals are unchanged.

No new Fillet through the session, Chamfer Apply, Add Cut, feature removal or
reordering, arbitrary edges, new profile class, Fillet+Cut, preview, picking,
autosave/recovery or tabs. §30 and Milestone 5C remain open; no next slice starts.
