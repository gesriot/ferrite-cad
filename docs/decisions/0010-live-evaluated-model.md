# 10. Live evaluated model

**Status:** accepted design.

An open model owns one kernel session and one accepted evaluation of its current private
document version. The evaluation retains node statuses, profiles, planes, sketch presentations
and solver reports, feature results and names, result keys, and the body meshes and pick tables
of its scene. A step may prepare one other revision. Both are immutable while a client decides
whether to display the candidate. Older Undo entries retain their document files.

## Evaluation and ownership

Open evaluates the whole document. Apply, Undo and Redo evaluate their target document in the
same graph order as a cold rebuild. A successful feature can reuse its accepted result under its
existing named archive key. A datum, sketch, parameter or body has a key covering its own
evaluated payload, input identities and keys, kernel identity and tolerance. A parameter binds
its current name to its verified value. Spelling alone cannot invalidate geometry or a solved
sketch. Expression integrity and reference resolution still precede reuse. A miss runs the
ordinary builder. The live session reads and writes no sidecar.

A reused feature preserves its elementary names and bindings. A rebuilt feature uses the same
naming and refusal rules as a cold feature. Each distinct live result has one kernel handle,
even when several bodies or revisions refer to it. A temporary boolean tool can be released
after the result has been named. Discard releases candidate-only shapes. Commit releases shapes
that have left the accepted revision and retains shared shapes. Closing releases the remainder.

## Display and publication

Keep native meshes, solid counts and body-local face, edge and vertex meanings under the body
result key and tessellation parameters. Assemble them in the target document's catalogue order
and assign the snapshot's pick indices there. An earlier body's changed topology can move a
later body's indices without requiring it to be tessellated again.

Apply uses the common copy transaction and compares candidate failures with the accepted
baseline. Undo and Redo use the same evaluator on their target snapshots. Commit adopts the file
and its prepared scene together. Cancellation between nodes, during a kernel operation or while
preparing a mesh discards the candidate. A failed display upload can also discard it.

The headless live document is used through a mutable owner and returns scene data, reports and
facts. Kernel handles stay inside it. The window's per-tab owner thread and incremental GPU
uploads belong to the next slice. Cold CLI rebuilds remain the reference.
