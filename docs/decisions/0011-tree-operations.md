# 11. Tree operations use the common transaction and evaluator

**Status:** accepted design.

Deleting and suppressing history entries changes the same document used by CLI edits and live
sessions. Keep the session's publication, Undo and refusal rules from ADR 0005, ADR 0006 D2/D5
and ADR 0010. Window commands use the common catalogue;
[ADR 0012](0012-history-rollback.md) defines rollback and insertion.

## Removal

Choose one `RemoveObject { object }` change for every object kind. Remove `RemoveParameter` and
its JSON spelling; there is no compatibility path. Treat all requested removals as one set.
Apply the other requested edits, splice each surviving predecessor link past the removed
features, and move each surviving Body tip to its nearest retained predecessor. If no feature
remains, the Body is an empty history container with `tip_feature = None`.

A surviving operation that needs a predecessor cannot become a history root implicitly.
An explicit operation edit in the same transaction may make a sweep start a new Body. Never
change Add, Cut or Intersect into NewBody on the user's behalf. Removing a Body requires an
explicit removal of every feature in its history. Likewise, a presentation parent cannot
silently remove surviving children through the database's foreign-key cascade.

After splicing, every retained dependency must name a retained object. Resolve parameter text
against the candidate's parameter catalogue before removing entries, so every remaining user
of each removed parameter is reported together with all other dependency conflicts. Identify
objects by name, kind in words and UUID. Outgoing rows of explicitly removed objects disappear
with them. Preserve unrelated dependency rows and identities.

The ordinary rebuild then reports every newly failed or skipped consumer. A missing named
edge is an error at its consumer, never an opportunity to select a neighbouring edge. A
refusal publishes no candidate and leaves the source unchanged. Existing imported-source
reachability and reclamation remove bytes only after their last explicit import claim goes.

## Suppression and storage

Choose the optional boolean `suppressed` in the common CBOR object envelope, beside `type`
and `payload`. Absence means false; writers emit `suppressed: true` only for a suppressed
feature. It is covered by the envelope's existing integrity hash. No feature-specific payload
gains a flag. A present flag on a parameter, sketch, datum, Body or import is an `input` error
when opening, even if the boolean is false. Null and non-boolean values are invalid.

`SetSuppressed { object, suppressed }` applies only to history features. Keep their saved
inputs and dependency rows, so unsuppressing restores the stated operation. A suppressed node
consumes only its predecessor's evaluated result: the same shape handle and complete bindings,
with no new derivations or ownership of inherited nodes. A suppressed root produces no shape.
It invokes no kernel operation and reads or writes no B-Rep archive of its own. Its result key
includes the suppression state, evaluated payload and predecessor key. Ordinary unsuppressed
feature keys retain their meanings.

The result status is `Suppressed`, independent of `Built`, `Failed` and `Skipped`. It contributes
neither to unbuilt counts nor to new failures. A failed predecessor still prevents a usable
result and follows the ordinary dependency-failure rule. An active successor consuming an
empty result fails explicitly; suppression never invents an empty solid for the kernel.

A Body whose suppressed history produces no shape is empty and follows the existing empty
Body display/export rule. A nonempty suppressed tip exposes the full predecessor set. All
faces, edges, vertices, ancestry and name cardinalities pass through without new naming rules.
References to entities produced only by the suppressed operation fail at their consumers.

## Live evaluation and presentation

Changing suppression or splicing a chain changes that node and downstream keys. Unchanged
predecessors and independent Bodies retain their live results. Sharing a handle across a
suppressed node does not add a release obligation; accepted and pending revisions keep the
existing distinct-handle ownership rule.

Reports list suppressed objects explicitly. The model tree shows `suppressed` without a
failure warning. Existing opening, Undo and Redo consume these ordinary snapshots. This change
adds no window deletion or suppression command.

The reference for a permitted removal is the same history constructed without that feature.
Suppression must match its geometry, surviving names and per-feature volumes. Unsuppression
must restore the original result. Cold, cached and live evaluation must agree.
