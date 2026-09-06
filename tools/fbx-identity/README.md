<!-- SPDX-License-Identifier: MIT -->
# §22B-1e3b: the identity channel, read back from outside

The gate is [`tools/check-fbx-identity.sh`](../check-fbx-identity.sh). This
directory holds the half of it that is not the gate script: the independent
implementation of the wire grammar, and the evidence.

## What the gate does

The shipped route, end to end and nothing fabricated. `import-step` publishes
an `.fcad` from the committed STEP fixture, the external STEP is deleted, and
`export-fbx` writes the FBX from the stored bytes alone — twice, in two
processes, so a value minted at export time cannot survive. Then pinned `ufbx`
0.23.0 reads the file in strict mode and prints every node's identity
properties verbatim, and [`scripts/join_identity.py`](scripts/join_identity.py)
joins them to what the document says it recorded.

The join has two sides and they come from two places. One is the file, read by
a program that has never heard of FerriteCAD. The other is the payload the
document was asked for directly, in raw fields that carry no wire encoding at
all. The grammar is implemented in the joiner from the written specification
and not from the writer's source, so agreement means that two independent
implementations of one rule produced the same string.

## The grammar, version 1

```text
value       = "fcad1" ":" domain ":" kind *( ":" field )
domain      = "def" / "occ"
kind        = "object" / "source" / "place"
field       = *( unreserved / pct-encoded )
unreserved  = ALPHA / DIGIT / "-" / "." / "_" / "~"
pct-encoded = "%" UPPER-HEX UPPER-HEX          ; one byte of UTF-8

definition, native body   fcad1:def:object:<object id>
definition, imported      fcad1:def:source:<source id>:<definition key>
placement,  native body   fcad1:occ:object:<object id>
placement,  imported      fcad1:occ:place:<occurrence id>
```

An identity a document never recorded has no property at all. There is no
spelling inside a value for "none", because a value that could say it would be
a value invented for a document that recorded nothing.

## The gate on the gate

`join_identity.py` only means something while it can still tell a wrong file
from a right one, and every way it could stop being able to compiles and runs.
[`scripts/check_join.py`](scripts/check_join.py) therefore hands it one
transcript that must be accepted and one per defect that must be refused — and
for each refusal it requires the message that names *that* defect rather than
merely a non-zero exit. Requiring the message is the point: a joiner with two
overlapping comparisons of one fact would refuse a bad transcript whichever of
the two was removed, and a gate that only looked at the exit status would call
that healthy. It did, once; the campaign found it, and both the joiner and this
gate were rewritten rather than explained away.

It needs nothing but Python and runs on every push.

## Evidence

| file | what it records |
| --- | --- |
| [`evidence/failing-first.log`](evidence/failing-first.log) | both readers refusing the shipped file for having no identity channel, after the model was proved to have arrived |
| [`evidence/byte-diff.log`](evidence/byte-diff.log) | every line that differs between the committed FBX baseline and this slice's output, and the digest that proves the writer changed nothing else |
| [`evidence/passing.log`](evidence/passing.log) | the same gate, on the same fixture, after the implementation |
