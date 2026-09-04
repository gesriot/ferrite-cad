# §22B-1e2b: measured, the remaining Unity identity mechanisms and an alternative FBX graph

Status: measurement only. The FBX writer, `ExportScene`, the document schema,
the shipped commands and the window are unchanged in this slice, and **no
policy is chosen here**. What follows is what Unity 6000.4.10f1 actually did to
five FBX graphs and to three importer-side mechanisms, and the decision table
that hands the choice to a person.

## What §22B-1e2a left open

§22B-1e2a measured names, custom properties and a companion `AssetPostprocessor`
on the **flat** production graph. It closed the source-qualification question,
found that a stock editor reads identity out of the object name and nothing
else, and said in its own conclusion that it had **not** measured any other FBX
graph, the Unity `.meta` identity table, `AssetImporter.AddRemap`, or a
`ScriptedImporter` using `AssetImportContext.AddObjectToAsset`.

Those four are measured here, on the same documents, with the same rules, so
the three slices read together. None of them is decided here.

## The contract, and the run that fails it

A mechanism passes only if it does all of this at once: real serialized
`{fileID, guid, type}` references survive; the `GameObject`, `Mesh` and
`Material` names a person reads stay the designations; every placement of one
definition keeps one shared `Mesh` by object identity; identity does not depend
on traversal order, sibling ordinal, FBX object number or Unity's collision
counter; two `ImportedSourceId`s sharing one source-local key stay two things;
a removed object goes missing rather than being re-pointed; and two clean
projects agree.

The failing-first run asserts that whole list against every measured graph and
collects every clause it fails rather than stopping at the first. It refused
with **253 failing clauses**, and the refusal is kept in
[`../../tools/unity-identity-graph/evidence/failing-first.log`](../../tools/unity-identity-graph/evidence/failing-first.log)
rather than described. Its first lines are the production graph:

```
the graph g-flat cannot tell two definitions with one source-local key apart:
  key:step.product_definition#42
the graph g-flat does not give every placement of a definition one shared Mesh:
  key:step.product_definition#42
the graph g-flat identifies a placement by ordinal_in_scene_order, which moves
  when a sibling does
the graph g-flat imports with warnings: Identifier uniqueness violation …
```

and it goes on to fail every other graph too, each for its own reasons.

## Instruments

The editor is the installed native arm64 `6000.4.10f1`, run `-batchmode
-nographics`, in **freshly created temporary projects outside the repository**
that are deleted afterwards. Nothing imported is committed: no `.fbx`, no
`.meta`, no `Library`, no Unity project.

Five editor modes, each in its own project, because three of the four questions
change what the editor *is*:

| mode | what it does | checks |
| --- | --- | --- |
| `graph` | stock Unity, five FBX graphs, 60 scenarios | 25 600 |
| `meta` | edits serialized importer metadata | 374 |
| `remap` | puts external assets in the project | 783 |
| `scripted` | registers a `ScriptedImporter` for a test extension | 2 176 |
| `fbxclaim` | registers a `ScriptedImporter` that claims `fbx` | 54 |

Each mode ran **twice**, in two separately created and separately deleted clean
projects, and the two canonical reports are byte-identical — including every
local file identifier. The only project-dependent values are the asset GUIDs,
which are tokenised.

Every sub-asset identifier was examined — 7 121 in the graph run and 1 594 in the
scripted one — and every one is exactly `GlobalObjectId_V1-1-<asset guid>-<local
file identifier as unsigned 64 bits>-0`. Not one is anything else.

Pinned `ufbx` 0.23.0 is the independent oracle over all sixty files. Both
programs hash every file with the same 64-bit FNV-1a, and the verifier refuses
a run in which they read different bytes.

### Where every byte came from, and what the transformer is held to

| files | written by |
| --- | --- |
| `g-flat` | `fbx_channel_documents` example → `write_fbx_ascii_7400`, copied unchanged |
| `g-flat-id`, `g-carrier`, `g-carrier-detached`, `g-two-level` | the same bytes, then `rewrite_graph.py --variant …` |

§22B-1e2a's rewriter changed names and properties. This slice needs a
*structural* transformer, so the claim it makes is bigger and is checked rather
than believed. It says it touches exactly three named sections — the two
`Count:` numbers in `Definitions`, new `Model` blocks and two edits inside
existing ones in `Objects`, and new or re-pointed `C: "OO"` lines in
`Connections` — and pinned `ufbx` compares every variant with the control on
everything else, across all twelve documents:

| variant | geometry arrays | material colours | control object numbers | control world transforms | objects added |
| --- | --- | --- | --- | --- | --- |
| `g-flat` | equal | equal | all present | unchanged | 0 |
| `g-flat-id` | equal | equal | all present | unchanged | 0 |
| `g-carrier` | equal | equal | all present | unchanged | 71 |
| `g-carrier-detached` | equal | equal | all present | unchanged | 71 |
| `g-two-level` | equal | equal | all present | unchanged | 95 |

The runner also refuses if `g-flat` is not byte-identical to the writer's
output, and both refusals are exercised by the mutation campaign. There is no
second serializer.

One difference the oracle reports and the transformer did not intend is
recorded rather than hidden: giving a `Geometry` one more instance changes the
per-face material index `ufbx` resolves through the *node*, so
`face_material_digest` moves for the two carrier graphs while every vertex,
index, normal and face is byte-for-byte the control's. That is why the digest
of the arrays and the digest of the face-to-material mapping are separate
numbers in the oracle rather than one.

## A. The graphs

| variant | topology |
| --- | --- |
| `g-flat` | the production graph, unchanged |
| `g-flat-id` | the same graph, invisible identity properties only |
| `g-carrier` | a machine-named definition carrier `Model` at the scene root, connected to the `Geometry` first |
| `g-carrier-detached` | the same carrier with no parent connection |
| `g-two-level` | each placement becomes a `Null`; a machine-named child bears the geometry |

`g-flat-id` isolates the topology question: everything below it differs from it
in graph *shape* and in nothing else. `g-carrier-detached` is the fourth
variant, chosen after `g-carrier` turned out to cost a visible node: it asks
whether the carrier has to be visible.

### What the editor published

| variant | GameObjects | MeshFilters | MeshRenderers | Meshes | Materials | invented root | machine-named nodes | machine-named sub-assets | warnings |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `g-flat` | 11 | 8 | 8 | 6 | 7 | no | 0 | 0 | 2 uniqueness violations |
| `g-flat-id` | 11 | 8 | 8 | 6 | 7 | no | 0 | 0 | 2 uniqueness violations |
| `g-carrier` | 18 | 14 | 14 | 6 | 7 | **yes** | 6 | 30 | **2 errors** |
| `g-carrier-detached` | 11 | 8 | 8 | 6 | 7 | no | 0 | 0 | 2 uniqueness violations |
| `g-two-level` | 19 | 8 | 8 | 6 | 7 | no | 8 | 38 | **none** |

Every variant kept the tracked triangle count and every placement's world
transform exactly as the control has them; the verifier refuses otherwise.

### What each graph did, in one line each

* **`g-flat`** — the product as it ships. The source-local key names two
  definitions, so 72 of its 216 tracked anchors come back `ambiguous_join`: not
  kept, not broken, *inexpressible*. Of the rest, 16 are lost, 5 of them by
  silently resolving to a different part.

* **`g-flat-id`** — the same graph with the source-qualified identity and the
  durable occurrence identity carried as invisible properties. The ambiguity
  goes to **zero** and the shared `Mesh` becomes one object per definition. The
  reference behaviour does **not** improve: 24 of 216 anchors are still lost, 9
  of them retargeted, because the visible names did not change and Unity's
  identity is still the visible name plus its collision counter. That is
  §22B-1e2a's result, reproduced here as the control for the topology question.

* **`g-carrier`** — a machine-named carrier at the scene root does claim the
  geometry first, and one shared `Mesh` per definition survives. It is a
  negative result on four separate counts, all of them measured: Unity **invented
  a root** because the file no longer has one top-level node; the import gained
  **7 GameObjects and 6 MeshRenderers** that draw the geometry a second time at
  the origin; a person reads **6 machine-named nodes and 30 machine-named
  sub-assets**; and the import raised **errors**, not warnings — `The mesh of
  Alpha Part has 1 sub meshes but the renderer is using 2 materials` — because
  the carrier has no material connections of its own.

* **`g-carrier-detached`** — the answer to "does the carrier have to be
  visible" is that a carrier with no parent connection is **not imported at
  all**. The file carries 17 `Model` objects; the import publishes 11, the same
  11 as the control, with the same identifiers. An unparented `Model` cannot
  separate definition from occurrence because Unity never builds it.

* **`g-two-level`** — the only graph that removed the `Identifier uniqueness
  violation` **entirely**. It moves the `Mesh` identity onto the definition:
  mesh losses fall from 3 to 1 and the shared `Mesh` survives. It fixes nothing
  else. `GameObject` losses stay at 5 and `Material` losses stay at 16, because
  the occurrence nodes keep their designations and the `Material` objects keep
  the writer's names. The price is **8 extra GameObjects**, all machine-named
  and all visible in the hierarchy, and **38 machine-named sub-assets** — the
  meshes and the machine nodes' components.

### The reference table, per graph and per type

`kept` counts `same_semantic`, plus `missing_because_object_was_removed` in the
one transition that removes the tracked definition. `ambiguous` is an anchor the
graph's identity could not name at all.

| variant | anchors | kept | lost | ambiguous | retargeted | GameObject lost | Mesh lost | Material lost |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `g-flat` | 216 | 128 | 16 | 72 | 5 | 5 | 3 | 8 |
| `g-flat-id` | 216 | 192 | 24 | 0 | 9 | 5 | 3 | 16 |
| `g-carrier` | 216 | 194 | 22 | 0 | 8 | 5 | 1 | 16 |
| `g-carrier-detached` | 216 | 192 | 24 | 0 | 9 | 5 | 3 | 16 |
| `g-two-level` | 216 | 194 | 22 | 0 | 8 | 5 | 1 | 16 |

### Where the references actually break

Measured on `g-flat-id`, which is the flat graph with the join already
possible, so nothing below is an artefact of the ambiguity:

| transition | GameObject | Mesh | Material |
| --- | --- | --- | --- |
| byte-identical, re-export, insert sibling, remove sibling, reorder siblings | all kept | all kept | all kept |
| a designation changes | 2 missing, **1 retargeted** | 1 missing, **1 retargeted** | all kept |
| a definition is inserted | all kept | all kept | 3 missing, **2 retargeted** |
| an unrelated definition is removed | all kept | 1 removed | 3 missing, **2 retargeted** |
| two definitions swap order | all kept | all kept | 1 missing |
| the tracked definition is removed | 1 missing, **1 retargeted** | 1 removed | 1 missing, **1 retargeted** |
| a material slot changes | all kept | all kept | 1 missing |
| a designation is reused on a second slot | all kept | all kept | 1 missing, **1 retargeted** |

Two things are worth naming. First, inserting or removing an unrelated
definition breaks **material** references and nothing else, because the
production writer's material names carry a global position suffix — `Early #0`,
`Shell #1`, `Shell #2` — and inserting a definition renumbers them. Second,
every "retargeted" cell is a reference that resolved silently to a different
part, with no warning and no missing-object marker.

## B. The `.meta` identity table

Unity 6000.4.10f1 does write `internalIDToNameTable:` into a model's `.fbx.meta`
— and, for this model, it writes it **empty**. The measured facts:

| question | measured |
| --- | --- |
| does `internalIDToNameTable` exist in this version | yes, as a key |
| how many entries for this import | **0** |
| which types it covers here | none: no `GameObject` (class 1), no `Mesh` (43), no `Material` (21) |
| is there a public API that writes it | **no member of `AssetImporter` or `ModelImporter` names it** |
| what public API does name the *other* table | `AddRemap`, `GetExternalObjectMap`, `RemoveRemap`, `SupportsRemappedAssetType`, `ModelImporter.SearchAndRemapMaterials` |
| does editing the serialized metadata by hand work | **no**: two hand-written rows — one for a real `Mesh`'s identifier, one for an identifier nothing produced — survived on disk across a reimport and changed **nothing**: no visible name moved, no local file identifier moved, and no object appeared at the invented identifier |
| does the table survive a re-export | vacuously: it is empty before and after |
| does it survive a real change | vacuously, for the same reason |
| does deleting the `.meta` change the identifiers | **no**: every local file identifier came back unchanged |
| must a sidecar exist before the first import | a `.meta` written **before** the first import **is** honoured: Unity took the GUID it asked for and kept its table entry |

So the honest status is stronger than "undocumented": in this editor version,
for this asset type, **no measured path writes the identity table at all** —
not a public API, and not editing the serialized metadata by hand. The stop
condition "undocumented `.meta` editing is the only working path" is therefore
**not** met, because undocumented editing did not work either.

What the table *could* express is also worth stating, since it decides whether
the mechanism is even the right shape: an entry maps a class identifier and a
local file identifier to a **name**. It is Unity's own record of names it
already assigned, not a place to put an identity a person never sees.

## C. `AssetImporter.AddRemap`, per type

`AddRemap` does not give an imported sub-asset a durable identity. It
*replaces* an imported sub-asset with an external asset of the same type, so
what a project references afterwards is a file the project owns. That is said
on the report's own first line, and no row below is behaviour of an exported
FBX.

The key is a `SourceAssetIdentifier`: a **type plus a visible name**. On the
control document, **6** such keys already name more than one object —
`Mesh:Alpha Part`, `Mesh:Twin Part`, `GameObject:Beta Part`, and the
`Transform`, `MeshFilter` and `MeshRenderer` of the same node. The key cannot
address those apart, so a durable identity cannot be reconstructed into it.

| | `Mesh` | `Material` | `GameObject` |
| --- | --- | --- | --- |
| `AddRemap` threw | no | no | no |
| appears in `GetExternalObjectMap` | yes | yes | yes |
| **the import honoured it** | **no** | **yes** | **no** |
| what the scene points at | the imported sub-assets | `external-material` | the imported sub-assets |
| external assets required | 1 | 1 | 1 |
| sub-assets of this type after | 6 (unchanged) | **6, from 7** | 11 (unchanged) |
| human names kept | yes | yes | yes |
| one shared `Mesh` kept | yes | yes | yes |
| a reference stored before the remap | same object | **missing** | same object |
| mapping survives a re-export | yes | yes | yes |
| mapping survives a designation change | the entry stays in the map, still keyed on the old name | same | same |
| mapping survives removing the definition | yes | yes, now **dangling** | yes |
| the measured transitions change this object | no | **yes** | no |
| external content after the FBX changed it | unchanged, but nothing uses it | **unchanged**: the project shows a colour the file no longer has | unchanged, but nothing uses it |

Two results decide this row of the decision table. `Mesh` and `GameObject`
remaps are **accepted and ignored** — a report that stopped at "the setting was
accepted" would have called that a working mechanism. And the one type the
model importer does honour, `Material`, makes the previously stored reference
**missing** and then keeps showing content the FBX no longer has, because an
external asset is a copy the importer never updates.

Whether the measured transitions really change the remapped object is itself
measured, from the documents and **before** any remap exists, because an
honoured remap also removes the sub-asset it replaces and a probe that inferred
"the file changed it" from "the name is gone" would confuse the two. Only the
`Material` row is measured on an object the documents change, which is exactly
why it is the row the stale-content result comes from — and the verifier now
refuses a remap report where no row is.

## D. `ScriptedImporter` and `AddObjectToAsset`, per type

The probe importer owns a **test** extension, `.fcadsyn`. It is not a FerriteCAD
importer, it reads no FBX, and it is not a package. It reads a synthetic
document carrying the same confusions as the FBX ones — two `ImportedSourceId`s
sharing `step.product_definition#42`, two definitions sharing a designation, one
definition placed twice, several material slots, a structural node and an
omitted one — and it builds every object with
`ctx.AddObjectToAsset(identifier, object)` where the identifier is derived from
the durable identity alone: `fcad|mesh|<source>/<key>`,
`fcad|material|<source>/<key>|<slot>`,
`fcad|object|<source>/<key>|<occurrence uuid>`. The designation goes to `name`
and nowhere else. The verifier refuses any identifier that does not match that
shape or that contains a designation.

It published 11 `GameObject`s, 6 `Mesh`es and 7 `Material`s, one shared `Mesh`
per definition, **zero** machine tokens in any visible name, and **zero**
ambiguous definitions. Then a designation changed, and the local file
identifiers were joined on the identifier the importer passed:

| type | identifiers compared | local file identifiers that moved | the identifier alone decides the identifier |
| --- | --- | --- | --- |
| `Mesh` | 6 | **0** | **yes** |
| `Material` | 7 | **0** | **yes** |
| `GameObject` | 11 | **3** | **no** |

That is the central finding of part D. For `Mesh` and `Material`,
`AddObjectToAsset` really does put the identity where the caller says. For a
`GameObject` inside the imported hierarchy it does not: Unity's own uniqueness
warning names the object as `fcad|root/<transform path>`, so a `GameObject`'s
identity under a `ScriptedImporter` is the **hierarchy path**, not the
identifier. Three of eleven placements moved when a designation moved, and
across the twelve transitions 4 references were lost — 2 missing, 1 retargeted
and 1 whose object was genuinely removed — every one of them a `GameObject`.

Two further facts:

* A deliberate identifier collision — every material slot given
  `fcad|material|collision` — was **not** merged and **not** refused. All 7
  materials were still published, and Unity emitted `Identifier uniqueness
  violation` and disambiguated them itself. That is a different answer from
  §22B-1e2a's FBX-side collision, which folded two materials with different
  colours into one object; on this path a collision costs order-dependence, not
  a lost object. Neither is safe without a declared collision policy.
* A `ScriptedImporter` **cannot own** `fbx`. In a project where one claiming
  `fbx` compiles, the asset still gets `ModelImporter`, the scripted importer
  never runs, and the model still publishes its 6 meshes and 7 materials. So
  this mechanism needs its own extension or a sidecar, and therefore a
  FerriteCAD Unity package the user installs.

## E. The two questions the brief asks by name

**Shared `Mesh` and human names together.** Two FBX graphs keep both:
`g-flat-id` and `g-carrier-detached` — which are, measurably, the same import.
The `ScriptedImporter` keeps both as well. **None of the three keeps every
reference**, so no measured mechanism keeps a shared `Mesh`, human names and
durable references at once.

**Multi-source identity and the occurrence.** The source-qualified identity
carried as an invisible property takes the control's 72 ambiguous anchors to
zero on every other variant, and the `ScriptedImporter` has zero as well. A
durable occurrence identity is still **required and still absent**: FerriteCAD
persists none today, every occurrence identity in this measurement is
synthetic, and every mechanism above needs one. It is necessary and, on its
own, not sufficient.

## The decision table

Six mechanisms. Every cell is measured or is marked as not measured. **Nothing
here is chosen.**

| | 1. pure FBX graph | 2. `.meta` table | 3. `AddRemap` + external assets | 4. `ScriptedImporter` | 5. machine-visible names | 6. human names, no guarantee |
| --- | --- | --- | --- | --- | --- | --- |
| stable `GameObject` references | no | no | no (not honoured) | **no** (path, not identifier) | yes | no |
| stable `Mesh` references | no | no | no (not honoured) | **yes** | no | no |
| stable `Material` references | no | no | yes, by replacement | **yes** | yes | no |
| human names | yes (`g-flat-id`) | yes | yes | yes | **no** | yes |
| one shared `Mesh` | yes (`g-flat-id`) | not applicable | yes | yes | yes | no |
| tells two sources apart | yes, with the property | no | no (6 colliding keys) | yes | yes | no |
| needs a persisted occurrence id | yes | yes | yes | yes | yes | no |
| needs a companion package | no | no | no | **yes** | no | no |
| needs external assets | no | no | **yes**, one per object | no | no | no |
| public API | the FBX format | **none found** | `AddRemap`, public | `ScriptedImporter`, public | the FBX format | the FBX format |
| user workflow | unchanged | a sidecar in version control | one external asset per object, re-pointed by hand on every rename | install a package, export a different extension | tokens in the hierarchy and asset lists | unchanged |
| production cost | writer change + occurrence id | no measured path writes it | a project-side convention and a tool to maintain it | a package, an importer and a file format | writer change + occurrence id | none |
| best measured variant | `g-flat-id` — the graph that keeps the most, which is the flat one | — | `Material` only | `Mesh` and `Material` | — | ships today |
| what it costs to get that | nothing visible, and it still loses 24 of 216 references | — | the old reference goes missing, and stale content | a Unity package and a non-`.fbx` extension | every visible name | — |
| the graph that keeps the most *references* | `g-two-level` and `g-carrier`, 194 of 216 — and only by moving the `Mesh`, for 8 visible machine-named nodes or an invented root | — | — | — | — | — |

## Stop-and-report

* **A pure FBX graph does not solve it.** Five graphs measured; none keeps
  every reference, and the two that keep a shared `Mesh` and human names are
  the flat graph.
* **No measured mechanism meets the whole contract.** Not one graph, not the
  `.meta` table, not `AddRemap`, not the `ScriptedImporter`.
* **A companion package would be required** for the `ScriptedImporter` path,
  and that path would also need **its own extension**, because `ModelImporter`
  keeps `fbx`.
* **External remapped assets would be required** for the one type `AddRemap`
  honours, and they bring a missing reference and stale content with them.
* **Undocumented `.meta` editing is not the only working path — it is not a
  working path.** Nothing measured writes that table.
* **A new persisted occurrence identity is still required**, by every
  mechanism, and is still not sufficient.
* **Two clean projects did not diverge**, in any of the five modes.

The choice between these six is not made here.

## The mutation campaign

**38 semantic mutants** against the real join verifier, applied to copies of the
recorded canonical measurement. All 38 killed. They include: a gate that only
checks non-null; the `ImportedSourceId` removed from the file; the source-local
key treated as global; the multi-source collision removed from the document;
the ordinal reported as durable; each of the three types dropped from the
tracking; the shared mesh replaced by copies; a carrier's extra renderer not
counted; an extra node not counted; a wrong transform accepted; an `AddRemap`
outcome called vanilla FBX behaviour; stale external content accepted;
undocumented `.meta` editing called a public API; the remap measured on an
object the transitions never change; a `ScriptedImporter`
identifier built from a display name; a deterministic identifier replaced by an
ordinal; an identifier collision accepted; a removed object retargeted;
identifiers compared only before the rename; the oracle reading a different
file; a prewritten report; a transition skipped; a run that tracked nothing; a
zero-check run; the two resolutions allowed to disagree; the transformer moving
a vertex, renumbering an object or recolouring a material; and the `fbx`
extension verdict inverted.

**11 compiled and scripted mutants** in the real transformer, the real probes
and the real runner, compiled and run against the real editor, with the
byte-for-byte comparison against the recorded measurement switched off so a
mutant dies from a check that understands the defect rather than from "these
bytes are not the recorded bytes". Plus seven harness controls: an anchor that
matches nothing, an anchor that matches twice, a stale backup, a probe that
does not compile, a prewritten report, a zero-check run, and an imported file
left in the repository — each refused.

### Three honest survivors, and one equivalent mutant

`the_added_node_given_a_transform_of_its_own` **survived** the first edition of
this harness. It gives the geometry-bearing child the two-level graph adds a
translation of its own: the *placement* still lands exactly where the control
puts it, so the placement-transform comparison saw nothing, while the part is
drawn somewhere else. It is named as a survivor rather than quietly fixed.

The fix is a check that understands the defect: every placement now records the
world transform of the geometry **under** it, not only its own, and the
verifier refuses when that moves. The same run also records, per variant, how
much geometry is drawn outside any placement at all — 6 nodes for `g-carrier`,
0 for every other graph — which is counted rather than refused, because for a
carrier graph that is the measured price and not a fault of the harness.

`a_scripted_identifier_that_is_not_the_durable_identity` **survived** the
second edition, and for a different reason: the defect was real and the check
that understands it exists, but the campaign ran that mutant with `--mode
scripted`, and the join those checks live in only runs when all five reports
exist. A mutant aimed at a check the invocation never runs is a hole in the
campaign, not a hole in the verifier, and it is recorded as one. It now runs
the whole measurement, and dies.

`the_remap_measured_on_an_object_the_document_never_changes` **survived** the
third edition, and it found a real defect in this slice's own measurement. It
points the `Material` remap at an object the transitions leave alone, so the
stale-content result becomes a statement about nothing. The first attempt to
kill it did not work, and the reason is the interesting part: the probe was
inferring "the file changed this object" from "its name is no longer among the
imported objects", which is *also* true when an honoured remap removes that
sub-asset. The two were being confused. The probe now measures whether the
documents change the object from the documents themselves, before any remap
exists, and the verifier refuses a remap report where no measured type is an
object the transitions change.

One further mutant was found to be **observationally equivalent** rather than
surviving: aiming "the extra renderer was not counted" at a graph that adds no
renderer changes nothing. It is now aimed at the graph that does add one, and
it dies.

## Honest limits

* Five FBX graphs were measured. Others exist; this slice does not claim they
  cannot work. It claims what these five did.
* One editor version, `6000.4.10f1`, on macOS arm64. The `.meta` result in
  particular is a fact about this version.
* The occurrence identities are synthetic and live only inside the measurement.
  No schema, no capability and no writer change.
* The `ScriptedImporter` proves a Unity identity mechanism on a test extension.
  It is not a FerriteCAD importer and reads no FBX.
* `AddRemap` was measured with external assets this probe created. Remapping to
  assets a project already owns was not measured.
* Binary FBX was not measured, and no importer setting other than the hierarchy
  sort was varied.

The harness, the raw reports and the reproduction commands are in
[`../../tools/unity-identity-graph`](../../tools/unity-identity-graph).
