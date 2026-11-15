# 15. Create Extrude and Revolve from a saved sketch in the window

**Status:** accepted.

## Problem

A viewport drawing can contain any supported sketch geometry, while the older
creation forms only make their own templates. The existing transaction API already
creates bodies and adds general sweeps. The window needs to expose that API without
adding a second evaluator, publication path or rule for unfinished forms.

## Decisions

The saved-object editor owns one sweep form. Its entry command fixes Extrude or
Revolve; there is no kind selector. The form participates in `saved_object_open`,
tab parking, Close, Quit and accepted-version dismissal just like the other saved
object forms. Its draft exists only in memory. Apply uses the owning session and
Cancel drops the draft without a transaction or question.

A sketch with an accepted drawing offers Edit sketch, Extrude and Revolve, in
that order, before its older editors. A failed sketch offers none of these three.
A built drawing without a profile keeps both sweep commands visible but disabled.
Their explanation names the sketch and repeats the complete profile failure from
the accepted rebuild. The scene transports that presentation text alongside its
drawings and datum planes; the window neither reads the profile nor solves again.

The form offers New body, Add, Cut and Intersect. The latter three select a body
with a shown feature and name that feature under the selector. The document's
existing insertion rule chooses the predecessor, including at a rollback marker.
Without such a body these operations are disabled with an explanation. Geometric
validity remains the kernel's decision; ordinary refusals stay visible in the form.

Extrude offers Blind, Symmetric, Through all and Up to next, with a Reversed
checkbox. Blind and Symmetric read a constant distance in millimetres; Symmetric
uses that distance on each side. Revolve offers Sketch X, Sketch Y and every Line
of the sketch, including construction lines, in document order. Line labels use
the solved coordinates of the accepted drawing. A turn is Full turn or a constant
angle entered in degrees and stored in radians, with the existing partial-angle
validation. Both sweeps use all closed regions, represented by an empty region list.

New body sends exactly one `AddBody`; Add, Cut and Intersect send exactly one
`AddFeature` to the selected body. New identities are UUIDv7. Sketch, Body, Extrude
and Revolve share the same smallest-free-number naming function over all document
object names. A successful Apply produces one version and one Undo step and closes
the form only when that version is accepted. A refusal preserves the complete
message, draft, accepted version and file. The viewport has the same behaviour as
with other saved-object forms, without a preview.

## Rejected alternatives

- Reuse the new-document form: it depends on the older canvas and creates a new
  document instead of adding to the current one.
- Switch between sweep kinds inside the form: separate entry commands explain the
  operation and avoid changing the meaning of fields already filled in.
- Compute missing-profile reasons in the window: this creates a second profile
  reader whose answer could differ from the accepted rebuild.
- Edit existing sweeps here too: expression fields and operation changes require
  a separate editing contract.

## Consequences

The existing kernel, file format, CLI, recognizers and transaction rules remain
unchanged. Publication and refusal follow ADR 0005 and ADR 0006; body operations,
insertion, drawing/profile separation and form ownership follow ADR 0008 and
ADR 0012–0014. Region selection, referenced end conditions, expressions and a
preview are outside this form.
