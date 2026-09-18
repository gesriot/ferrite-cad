# 4. A feature names the result it modifies; a body names its tip

**Status:** accepted, and narrow — it decides one thing and leaves the rest open.

## The problem this had to solve

Before §26A a body's history was one feature long, so nothing had ever had to
say what a feature's *input* was. The payload had a field that looked like the
answer — `Extrude.target_body` — and the validator required a `TargetBody`
dependency edge whenever it was set. A body, separately, names its
`tip_feature` and requires a `BodyTip` edge.

Put a cut in a body with both of those and the graph is:

```
cut --TargetBody--> body --BodyTip--> cut
```

That is a cycle, and `evaluation_order` cannot order it. It is not a check that
could be relaxed: there is genuinely no order in which both objects can be
evaluated after the other. Two other things were in the way as well —
`convert::extrude_request` refuses every boolean by name, and `GeometryKernel`
had no boolean at all — but those are missing implementations. The cycle is a
modelling error, and removing the refusals would not have fixed it.

## What was decided

Three facts, each stored once, in the place that owns it.

* **What a feature consumes** is a *feature*. `Extrude.previous: Option<ObjectId>`
  names the feature whose result this one modifies, recorded by a new
  `DependencyRole::Predecessor` edge from the feature to that feature. It is
  `None` exactly when the operation is `NewBody`.
* **What a body currently is** stays `Body.tip_feature`, recorded by the
  `BodyTip` edge it always was.
* **Which body owns a feature** is not stored at all. A body owns exactly the
  features it can reach from its tip by following `previous`. There is no
  second source to disagree with the first, and no edge from a feature to a
  body, so the graph is acyclic by construction:

```
plane <- profile <- extrude <- cut -> tool sketch -> plane
                        ^        ^
                        |        |
                    Predecessor  |
                                 |
                       body --BodyTip--> cut
```

The validator checks what the construction cannot: `previous` names a feature
that exists, the edge that records it is there, a feature does not name both a
predecessor and a target body, exactly one feature may consume any given
result (`feature.forked-history`), and exactly one body may expose any given
feature as its tip (`body.shared-tip`). Bodies with different tips must also
have disjoint predecessor histories (`body.shared-history`): exposing an older
feature through another body would duplicate part of the same history.
`evaluation_order` still runs over the
whole graph, so the cycle check is not bypassed — it is simply never reached by
a document this build writes.

## What happens to `target_body`

It stays in the payload and keeps its old meaning, because a stored layout is
not something to quietly redefine. Nothing in this build writes it, the payload
rule refuses a feature that names both it and a predecessor, and a document
that does name it is refused where it always would have been: the `TargetBody`
edge and the `BodyTip` edge close the loop and the document does not order.
That refusal is a test
(`naming_a_body_instead_of_a_predecessor_cannot_be_ordered_at_all`), because it
is the reason the new field exists.

## Why the layout moved with it

`previous` is an `Option` that serialises away when absent, so a build written
before it existed would decode a feature that has one, drop the field, and — on
the first rewrite — produce a feature that starts a body of its own out of
nowhere. The history would still look plausible and the solid would be wrong.

So a feature holding a predecessor is stored at **payload v2** and declares
`feature.predecessor.v1`. v2 is not in `ObjectKind::readable_schema_versions`
for any earlier build: that build preserves the object verbatim and opens the
document read-only, which is the refusal that protects the data. The capability
makes the reason legible rather than leaving a v2 feature announcing what a v1
one did. An extrusion that starts a body is still the v1 feature it always was,
and the SQLite schema did not move.

## What this does not decide

Nothing about Add or Intersect beyond refusing them; nothing about a history
that branches, which `feature.forked-history` refuses rather than models;
nothing about rolling a body back to an earlier feature, which this shape makes
expressible but which no operation offers; and nothing about attaching a sketch
to a face, which needs a durable name for the face and is a slice of its own.

## §26C: provenance of faces carried through another boolean

Decision recorded before implementation. A carried face is keyed by the UUID
of the feature that originally named it and its original cap side or profile
segment UUID. The topology map carries that pair through **each** OCCT history
step, including already carried faces and deleted names. Geometry measurements
are independent assertions, never a matching mechanism.

The second Cut's own wall/floor retain `ExtrudeSide`/`ExtrudeCap`. Its new
references to earlier faces use `OriginSide`/`OriginCap` with an explicit
`origin_feature`. Thus the plate's End, the first pocket's End and the second
pocket's End remain three distinct names. Stored references are never rewritten:
their producer still addresses the historical output it always addressed.
New references address the final output and retain the earlier face's origin.

Legacy `CarriedCap`/`CarriedSide` keep their exact meaning: the immediate
predecessor's **own** cap/side. They are resolved as aliases into the qualified
map, not as another set of geometry. The map and archive record that predecessor
explicitly. No feature-to-Body edge or duplicate ownership is introduced.

New roles require `topology.origin-face.v1`; older readers therefore preserve
the unknown references and refuse writes. Existing feature payload v2 already
expresses the predecessor chain and needs no reinterpretation or SQL migration.
Named archive format v3 includes predecessor identity and qualified ancestor
bindings; v1/v2 entries are explicitly invalidated and rebuilt. A physical face
is archived once, so provenance aliases cannot conceal two meanings in one slot.

The managed operation accepts only the existing four-object plate or the exact
six-object §26A/B frame. A second tool must be separated from the saved disk by
strictly more than `Tolerance::DEFAULT_LINEAR`, in addition to the existing
wall/depth policy. The eight-object result is neither an Add-cut target nor a
§26B edit target. Wider histories and editing either of their cuts remain future
work; the six-object editor contract is unchanged.

## §26D: editing either saved link

The exact six/eight-object history is validated as a whole. Base, selected Cut,
its immediate predecessor, Body tip and the neighboring tool are separate facts
read through links. The copy changes only the selected tool and depth; the DAG
rebuilds its dependents without moving the tip or any UUID.

[The topology policy](../edit-sequential-cuts.md) was recorded before code:
through-to-pocket at the first link adds its historical own floor and the final
producer's OriginCap(first, End); at the second it adds only its own floor.
Pocket-to-through refuses in preparation and names every protected floor UUID.
Existing producer/origin meanings, archive v3 and predecessor-qualified keys
remain unchanged. No new ownership model, feature-to-Body edge or copier exists.

## §26E: bounded linear catalogue

The same predecessor graph now admits 0–16 circular Cuts in this editor.
One validated history serves add/edit discovery from one snapshot. It checks
all stored tools and every pair, every object and the complete dependency set.
At 16 only editing is offered; longer general documents receive an explicit
editor refusal. Through-to-pocket at i names its floor once at every producer
from i through tip. Origin identity, historical scopes, archive v3 and cache
keys retain their meanings. [Policy recorded before code](../circular-cut-history.md).
