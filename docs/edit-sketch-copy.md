# Edit saved sketch geometry

Use [the shared edit transaction](edit-transactions.md) for coordinate changes.
`inspect --json` supplies curve identities and saved geometry. Every changed curve
keeps its UUID and geometry kind. A request can update both ends of incident Lines
and combine geometry, expressions, constraints and feature changes atomically.

The evaluator rebuilds the candidate and its dependents. The kernel decides sweep
validity; there is no required document shape, profile class, prescribed axis boundary
or wall clearance. An unresolved consumed reference fails its feature. Existing
failures may be repaired, and unconsumed topology may disappear.

The window's Edit Sketch draft and Apply use this same library transaction. Undo/Redo
returns to previously accepted candidate states. Publication checks the source version
and never overwrites the source. See [generic sweep semantics](generic-sweeps.md).
