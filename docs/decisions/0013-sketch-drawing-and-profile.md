# 13. Build a sketch drawing and read its profile separately

**Status:** accepted design.

This extends ADR 0008's profile regions and ADR 0010's retained sketch evaluation.
ADR 0007 section 1 is unchanged: a curve end names the stored start or end, even
when a profile traverses that curve in the opposite direction. Publication and
Undo retain ADR 0005 and ADR 0006 D2/D5's existing atomic rules.

## Drawing and profile

A sketch is built when its plane and drawing are built. Empty drawings, points,
construction geometry and open chains are legitimate drawings. Missing planes,
solver refusals or conflicts, and unknown geometry still fail the sketch with
their existing diagnostics. One solve supplies the presentation, optional solve
report and profile reading; asking for a profile never solves again.

Read a profile from every main Line, Arc and Circle. Points and construction
geometry do not participate. Every participating curve must belong to a closed
boundary: a dangling line beside a valid loop invalidates the whole reading.
Branching joints identify their position and curves; open chains identify a
curve and free endpoint. Existing intersection/touch and region rules remain.

Retain either the profile or its complete typed failure in the evaluated model,
including reuse by the live model. Extrude and Revolve report the sketch identity
and that failure when consuming a drawing without a profile. The drawing remains
Built; the consumer is Failed. Cached and cold rebuilds use the same reading.
No new status or persisted field is introduced. The document format is unchanged.
The ordinary no-new-unbuilt-feature rule still refuses an edit that opens a
previously consumed contour, but permits that edit on an unconsumed drawing.

## Stored ends and traversal

Connect each segment through either endpoint without changing stored curves or
their ordering. Reverse a derived Line by exchanging its endpoints and an Arc by
exchanging its angles. The kernel profile remains a directed closed boundary and
the C++ bridge is unchanged. A derived segment records whether traversal reverses
its stored direction. Rust history adapters, including the mock, translate bridge
start/end witnesses back to the stored end before naming.

An arc runs from start_angle to end_angle with the sign of their difference.
This records the existing drawing, region and kernel interpretation; arc
validation, constraints and solver behavior do not change.

Preserve the existing walk and cache bytes for already directed contours. A
reversed segment contributes an additional tagged orientation fact to its profile
key, so geometrically equal walks with different stored ends cannot share names
through an archive. No naming scheme, format version or bridge change is needed.

## Ordered curve edits

AddCurve appends a caller-identified curve and refuses an occupied curve identity.
RemoveCurve removes exactly that identity. SetConstruction changes its saved flag;
an unchanged value follows SetSuppressed's model-content NoChange rule. All three
use the ordinary candidate and explicit CLI parsing. AddCurve and AddSketch share
the same curve parser. SetCurveGeometry continues preserving geometry kind.

Validate the complete sketch after all changes, using the existing validator.
A constraint and its curve may be removed in either order in one request, but
removing only the curve refuses. Removing and adding the same identity is legal.
Region selections, revolve axes and topology references retain their existing
validation and rebuild protections; no additional consumer policy is added.
An empty AddSketch succeeds; a consuming addition must obtain a valid profile.

## Scope and rejected alternatives

The CLI gains add_curve, remove_curve and set_construction and explains absent
profiles in rebuild output. The window gains no tools, commands or forms; existing
recognizers remain narrow and unchanged. Viewport drawing, constraints and removal
of the old forms belong to later work.

| Alternative | Reason rejected |
| --- | --- |
| Rewrite stored curve directions | Changes constraint selectors and durable end identities. |
| Make the bridge accept undirected contours | Changes C++ and provenance despite a directed Rust conversion being sufficient. |
| Add a sketch-without-profile status | The drawing is built; only its consuming sweep cannot build. |
| Select closed subsets of an incomplete drawing | Silently discards main geometry; construction flags express that choice explicitly. |
| Delete constraints implicitly with a curve | Removes saved intent; the caller must include each intended removal. |
