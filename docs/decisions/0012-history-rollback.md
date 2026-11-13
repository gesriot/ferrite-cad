# 12. Persist a Body's displayed history position and insert there

**Status:** accepted design.

This decision extends [ADR 0011](0011-tree-operations.md) and clarifies ADR 0008's
description of a Body as the complete result of its `tip_feature`. The tip still
owns the whole history; an optional earlier position chooses the displayed result.
Publication, refusal, Undo and live-result ownership retain ADR 0005, ADR 0006 D2/D5
and ADR 0010's rules.

## Stored position and displayed result

Choose `Body.rollback: Option<ObjectId>`. The name distinguishes a deliberate,
persisted position from `DegradedBody.rolled_back_to`, the evaluator's fallback
after a failure. Omit the CBOR field when absent. A present marker must be a UUID
naming a feature in this Body's predecessor chain, strictly before its tip. An
empty Body cannot have a marker. Opening rejects violations as input errors,
identifying the Body and the offending feature; no compatibility path is needed.

`Body::shown_feature` is the single answer for the displayed result: the marker,
otherwise the tip. Geometry, picking, solid counts, exports and form-addition
targets use this answer. History validation, tree ordering, deletion and dependent
closure still traverse from the tip. No dependency role is added: `body_tip` and
the complete predecessor chain already keep every possible marker reachable.

Every feature is still built, including those after the marker. There is no new
feature status and an edit introducing failures still refuses in full. A Body
whose tip fails remains degraded under the existing rule, but its search for the
last built result begins at the shown feature. Its existing failure sentence and
export refusal do not change.

The scene's Body result key comes from the shown feature. The Body node key still
includes its own payload and the tip's key, so changing the marker or a later
feature invalidates that node. Retessellating the unchanged picture after a later
edit is allowed. Moving the marker reuses live feature results, invokes no
modelling operation, and may tessellate only this Body's newly shown result.

## Transaction and insertion

`SetRollback { object, feature }` changes the Body's optional marker through the
common transaction. CLI spelling is `set_rollback` with `object` and `feature`, or
`clear_rollback` with only `object`; neither uses null. Reject a non-Body target,
a feature outside its history and its tip with complete named input errors. The
tip refusal directs the caller to `clear_rollback`. Setting an unchanged marker
or clearing an absent one follows `SetSuppressed`'s existing no-change rule.
Dirty still compares model content with the saved checkpoint. Returning to that
exact model clears dirty, although an accepted change has its own document version
and Undo step.

`AddFeature` without a marker appends as before. With a marker, set the new
feature's predecessor to that marker, splice its former successor to the new
feature, and move the marker to the inserted feature. Keep the tip. Persist the
changed predecessor dependency together with the payload. Two successive
insertions therefore retain request order.

Changes see earlier changes in the same candidate. CLI insertion is the ordered
batch `set_rollback`, `add_feature`, `clear_rollback`, without an additional
insertion-position field. A future-result input creates a cycle and is refused
by the existing graph validator, not a second policy check.

Keep existing entity references unchanged. Their producer is the feature that
minted the name, which may precede the result consumed by the successor. Resolve
in the new predecessor's result under ADR 0007. A lost or split consumed entity
causes the ordinary full refusal, never a nearby substitute. The reference is a
document constructed in the final order with the same UUIDs: payloads, dependency
rows, per-feature volumes, geometry and names agree under cold, cached and live
evaluation.

## Deletion

When removing the marker feature, use the nearest retained predecessor as the
new marker, or clear it if none remains. After history splicing, clear a marker
that equals the new tip. Removing a Body still requires explicit removal of its
entire history. No retained reference or presentation child disappears implicitly.

## Window and scope

The stored-only tree projection marks the Body as `rolled back`, reports the shown
feature and number of later features, and marks those later features `not shown`.
Evaluation warnings retain precedence over the stored note and keep the suffixes.
The shared command catalogue offers Roll back to here and Roll to end through its
existing surfaces, without a form or shortcut. Existing ownership, Parameters and
operation guards remain. Cut, Fillet, Chamfer and KeepSolids additions use the
shown result; editing an existing later feature retains its original route.

This does not reorder features, provide rollback before the first feature, keep
an unsaved per-tab display position, or temporarily show an old feature while
editing it. The next separate task is the viewport sketcher.

## Rejected alternatives

| Alternative | Reason |
| --- | --- |
| Rollback as tab-only presentation | Cannot define a persisted insertion point or agree with CLI and export. |
| Stop building after the marker or add a not-built-after-marker status | Could accept a document that cannot roll to its end without a refusal. |
| An after-feature field on AddFeature | Duplicates the Body's insertion position and the ordered transaction. |
| A new marker dependency role | The existing tip and predecessor chain already own the complete history. |
| Rewrite successors' references | Silently changes their meaning instead of preserving exact resolution. |
