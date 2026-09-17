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
