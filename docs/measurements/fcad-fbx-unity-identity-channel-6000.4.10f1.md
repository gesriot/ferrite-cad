# §22B-1e2a: measured, whether a Unity-safe identity can be separated from the visible names

Status: measurement only. The FBX writer, `ExportScene`, the document schema,
the shipped commands and the window are unchanged in this slice, and **no
policy is chosen here**. What follows is what Unity 6000.4.10f1 actually did to
five candidate identity channels, and the decision table that hands the choice
to a person.

## The two questions §22B-1e1 could not answer

§22B-1e1 measured documents containing **one** imported source. Inside such a
file the `FerriteCADDefinitionKey` the writer exports — which contains only the
source-local key returned by `definition_key`, and not the `ImportedSourceId`
stored beside it — is unambiguous, and that slice said so and stopped there.

Two STEP sources may legally both contain `step.product_definition#42`. So the
first question is whether the current property is a durable identity at all
once a document has more than one source. The second is the one §22B-1e1 named
and left open: identity today *is* the visible name, and whether it can stop
being that without turning a hierarchy into machine tokens was not measured.

Both are measured here. Neither is decided here.

## Instruments

The editor is the installed native arm64 `6000.4.10f1`, run `-batchmode
-nographics`, in **freshly created temporary projects outside the repository**
that are deleted afterwards. Nothing imported is committed: no `.fbx`, no
`.meta`, no `Library`, no Unity project.

Two editor modes, because one candidate is not a property of a file:

* **vanilla** — stock Unity, four candidates, 48 scenarios, 18 532 editor-side
  checks;
* **companion** — the same editor with a FerriteCAD companion `AssetPostprocessor`
  renaming objects from the designations the file carries, 24 scenarios, 9 179
  checks.

Each mode ran twice, in two separately created and separately deleted clean
projects, and the two canonical reports are byte-identical — including every
local file identifier. The only project-dependent values are the asset GUIDs,
which are tokenised.

Pinned `ufbx` 0.23.0 is the independent oracle over all fifty-four files: the
FBX object name exactly as the file spells it, the custom properties, the
hierarchy, the geometry sharing, and a count of how many source-local keys name
more than one geometry. Both programs hash every file with the same 64-bit
FNV-1a, and the verifier refuses a run in which they read different bytes. That
is a content check between two programs, not a security digest.

Every sub-asset identifier was examined — 5 487 in the vanilla run, 2 667 in the
companion one — and every one is exactly `GlobalObjectId_V1-1-<asset guid>-<local
file identifier as unsigned 64 bits>-0`. Not one is anything else, so the
recorded tables carry the two components rather than a third copy of them.

### Where every byte came from

| files | written by |
| --- | --- |
| `a-control` | `fbx_channel_documents` example → `write_fbx_ascii_7400`, copied unchanged |
| `b-ordinal`, `b-occurrence`, `c-property` | the same bytes, then `rewrite_channel.py --candidate …` |
| `d-companion` | **byte-identical to `c-property`**; the difference is the plugin |
| the six name probes | the same bytes, then `rewrite_channel.py --names` |

The rewriter changes exactly two things inside a production file: the name in an
object's own header line, and the custom properties inside a `Model`'s
`Properties70`. Object numbers, connections, geometry, transforms, colours and
the whole `Definitions` section are copied byte for byte. The runner refuses if
`a-control` is not byte-identical to the writer's output, and that refusal is
exercised by the mutation campaign. There is no second serializer.

## The measured document

One base document, 15 577 bytes, `fnv1a64 4d8905c0e3393c7e`, and eleven others
each one change away from it. Its counts, read by `ufbx` rather than assumed:

```text
models 11   geometries 6   materials 7
definition_key_collisions        1    two sources, one source-local key
repeated_geometry_display_names  3    definitions sharing a designation
repeated_sibling_names           4    siblings sharing a designation
placements_sharing_one_geometry  4    one geometry with two placements
repeated_material_slot_names     2    two slots of one mesh sharing a designation
structural_nodes                 2    an assembly root and a sub-frame
omitted_nodes                    1    a definition this build could not tessellate
```

Every definition has a vertex count no other one has — 3 to 9 — so a silent
retarget is visible without trusting any name, key or position. The two
definitions that share `step.product_definition#42` have 8 and 9 vertices and
different colours.

## Failing first

Before anything was recorded, the run was made with `--expect-durable-join`,
which asserts the optimistic contract: that the identity a candidate exports
tells every definition of the document apart. The editor imported all 48
vanilla scenarios and then refused:

```text
FCAD_CHANNEL_IDENTITY_FAILURE System.InvalidOperationException: the exported
identity of a-control cannot tell two definitions apart:
key:step.product_definition#42
```

That is today's production property, on the real editor, on a document with two
sources. It is kept in
[`../../tools/unity-identity-channel/evidence/failing-first.log`](../../tools/unity-identity-channel/evidence/failing-first.log)
rather than remembered. The same assertion passes for every candidate that
carries `ImportedSourceId` beside the source-local key.

## 1 — The multi-source collision

Read from the file, without the editor: **one** source-local key names two
different geometries in every one of these documents, under every candidate.
The candidates differ in what they carry *beside* it.

| candidate | join the probe used | definitions it cannot tell apart | ambiguous anchors |
| --- | --- | --- | --- |
| `a-control` | `FerriteCADDefinitionKey` | 1 — `key:step.product_definition#42` | 144 of 432 |
| `b-ordinal` | `FerriteCADDefinitionId` | 0 | 0 |
| `b-occurrence` | `FerriteCADDefinitionId` | 0 | 0 |
| `c-property` | `FerriteCADDefinitionId` | 0 | 0 |
| `d-companion` | `FerriteCADDefinitionId` | 0 | 0 |

In the editor, the control's two `#42` definitions arrive as two meshes, two
materials and two placements that the exported identity cannot name apart. The
probe does not pick one: those anchors are reported `ambiguous_join`, which is
neither a kept reference nor a broken one. Six of the eighteen tracked anchors
are ambiguous in every one of the control's twelve scenarios.

The full identity that removes it is `ImportedSourceId` **plus** the
source-local key. Nothing weaker was tried and nothing weaker works: a path, a
display name, a traversal order, an FBX object number and a collision suffix
are all excluded by construction, and the two definitions are otherwise
identical in every respect a name could see.

A native body's `ObjectId` was **not** measured. These documents contain
imported definitions only; §22B-1b2 already establishes that a body's durable
identity is its `ObjectId`, and this slice does not extend that claim.

## 2 — Identity apart from the designation

### What Unity names each kind after

Measured directly, because these candidates give the `Model` and the `Geometry`
deliberately different names:

Each of the eight meshes an import publishes is compared with the name the file
gives its own `Model` node and with the name the file gives its `Geometry`.

| candidate | after its own node's `Model` name | after the FBX `Geometry` name | after neither |
| --- | --- | --- | --- |
| `a-control` | 8 | 0 | 0 |
| `b-ordinal`, `b-occurrence`, `c-property` | 6 | 0 | 2 |
| `d-companion` | 0 | 0 | 8 |

**Not one Mesh in any candidate is named after the FBX `Geometry`.** §22B-1e1
inferred this from a document whose geometry names happened to be unique; here
the geometry carries a different name from every `Model` that places it, and the
editor still ignores it.

The two that match neither, under the machine-named candidates, are the two
placements of the shared geometry: the mesh carries the *first* placement's
`Model` name, so it matches that node and not the other. Under `a-control` both
placements spell the same designation, so the same behaviour shows up as eight
matches rather than six. Under the companion every mesh has been renamed to a
designation and matches nothing the file spells, which is the point of that
candidate.

The imported model's own root is named after the **asset file**, in every
candidate including the companion one — `d-companion_s01-byte-identical`, not
`Assembly Root` and not the token. Its local file identifier is nevertheless
identical across all three namings, so the model root's identifier does not
depend on its name at all. That is the one object whose visible name the FBX
cannot reach.

### What a person reads

| candidate | visible node, mesh and material names | the editor's uniqueness warning on the base document |
| --- | --- | --- |
| `a-control` | designations (`Alpha Part`, `Shell #1`) | 2 — `Name:Alpha Part, Type:Mesh`, `Name:Twin Part, Type:Mesh` |
| `b-ordinal`, `b-occurrence` | machine tokens (`fcad~019ffc72-…~step.product_definition#100~occ~0`) | **0** |
| `c-property` | machine tokens; the designation is in `FerriteCADDisplayName` | **0** |
| `d-companion` | designations | **10** |

Candidate C **does not work without the companion**. Its bytes carry the
designation, and a stock editor never reads it: the hierarchy, the mesh list
and the material list all show the token. That is measured, not assumed — the
verifier refuses a run in which a vanilla import shows anything other than what
the file spells.

The ten warnings under the companion are worth reading, because they are not
the control's two:

```text
2 x Identifier uniqueness violation: 'Name://RootNode/root/Alpha Part, Type:GameObject'
2 x                                  '…/Alpha Part/MeshFilter, Type:MeshFilter'
2 x                                  '…/Alpha Part/MeshRenderer, Type:MeshRenderer'
2 x                                  '…/Alpha Part/Transform, Type:Transform'
1 x                                  '…/Twin Part, Type:GameObject'
1 x                                  '…/Twin Part/MeshFilter, Type:MeshFilter'
1 x                                  '…/Twin Part/MeshRenderer, Type:MeshRenderer'
1 x                                  '…/Twin Part/Transform, Type:Transform'
1 x                                  'Name:Shell, Type:Material'
1 x                                  'Name:Twin, Type:Material'
```

Stock Unity disambiguates duplicate siblings itself — the control's hierarchy
reads `Alpha Part`, `Alpha Part 1`, `Alpha Part 2`. The companion renames after
that has happened, so three objects end up literally called `Alpha Part`, and
the collision the machine names had removed comes back — on `GameObject`,
`Transform`, `MeshFilter`, `MeshRenderer` and `Material`, **five object kinds
the control never warned about, and not on `Mesh`, which is the only kind the
control did warn about**.

### Where the companion's rename lands, per kind

The decisive question for candidate D is whether Unity computes a local file
identifier before or after `OnPostprocessModel` renames the object. The same
bytes were imported with and without the plugin, and both were compared with
the control:

| kind | compared | moved by the rename | unmoved | equal to the control | verdict |
| --- | --- | --- | --- | --- | --- |
| `Mesh` | 191 | 0 | 191 | 0 | **computed before the rename** |
| `GameObject` | 239 | 239 | 0 | 168 | **computed after the rename** |
| `Material` | 191 | 191 | 0 | 0 | **computed after the rename** |

A Mesh keeps the identifier its machine name gave it while displaying the
designation. A GameObject and a Material do not: their identifiers follow the
final, human name. The 168 GameObjects that land exactly on the control's
identifier are the ones whose renamed name is unique; the other 71 differ only
because stock Unity had appended a disambiguation suffix that the companion
does not.

**One postprocessor rename is not one identity scheme for all three kinds.**
The three measured kinds give different answers to the rename question: the
Mesh identifier is fixed before it, while GameObject and Material identifiers
follow it. Machine-visible names still stabilise all three with one scheme;
what this result rules out is treating a humanising rename as if it preserved
all three kinds alike.

## 3 — Placement identity

Only inside the measurement, each placement was given a synthetic persistent
occurrence UUID. Nothing about the document schema changed. `b-ordinal` names a
placement by its ordinal among the placements of its definition — the only
occurrence identity FerriteCAD persists today — and `b-occurrence` names it by
the UUID. They differ in that one field and nothing else.

| scenario | `b-ordinal` | `b-occurrence` |
| --- | --- | --- |
| a sibling occurrence inserted | every reference kept | **one shared Mesh lost** |
| a sibling occurrence removed | every reference kept | every reference kept |
| two sibling occurrences swapped | every reference kept | every reference kept |
| a definition inserted, removed or reordered | every reference kept | every reference kept |

The durable occurrence identity did not improve placement stability, and it
made one thing worse. The mechanism is exact, and it is the important result of
this section:

* A shared Mesh takes its name from **the first Model node that references it**.
* Under `b-ordinal` that node's name ends in `occ~0` whichever placement is
  first, so inserting an earlier sibling leaves the mesh's name unchanged — by
  accident, because the ordinal of the first placement is always zero.
* Under `b-occurrence` the first node's name ends in the inserted placement's
  UUID, so the mesh's name, and its identifier, moved:

```text
before  node/4  …occ~…005   mesh …occ~…005   -1732955004291903615
after   node/3  …occ~…00d   mesh …occ~…00d     623818506449910774
        node/5  …occ~…005   mesh …occ~…00d
        node/11 …occ~…00b   mesh …occ~…00d
```

So **none of the flat, production-shaped name rewrites measured here makes a
shared Mesh's identity a function of the FerriteCAD definition.** Unity derives
it from a Model node in these files, and those Model names distinguish
placements. This does not rule out a different FBX graph topology or a Unity
remapping/importer API; neither was measured. A durable occurrence identity is
necessary for a *placement* to be nameable at all: the ordinal is positional,
and §22B-1e1 already showed positional keys move for no reason. But it does
not fix, and here slightly worsens, the geometry it is attached to. Both
candidates keep every surviving placement reference in every scenario, and
the ordinal's stability under insertion is a coincidence of "first is always
zero" rather than a measured property of ordinals in general.

`b-ordinal` is therefore not evidence that the ordinal is durable. It is
evidence that the two schemes cannot be told apart by placement references
alone in these transitions, and that they *can* be told apart by what happens
to a shared mesh.

## 4 — What the name channel does to a token

Six documents, one naming question each, imported once. All fifty-one
sub-assets are accounted for in each, and every outcome is named — there is no
catch-all row.

| probe | longest name in the file | non-ASCII object names | what the editor did |
| --- | --- | --- | --- |
| the source-qualified token as it stands | 110 bytes | 0 | every name identical |
| the same token, 160 characters longer | **271 bytes** | 0 | every name identical |
| a Cyrillic designation inside the token | 130 bytes | 24 | every name identical |
| a 16-hex-digit FNV-1a token | 21 bytes | 0 | every name identical |
| one designation precomposed, another decomposed | 110 bytes | 6 | every name identical; the two forms stay two objects |
| **two durable identities given one token** | 21 bytes | 0 | see below |

No truncation at 271 bytes. No normalisation: a precomposed `й` and a
decomposed `и` + combining breve stayed two distinct objects with two distinct
identifiers, so the editor does not fold them and a designation cannot be
smuggled through that door either. No mangling of Cyrillic.

The collision case is the one that matters, and it is why no hash token can be
called safe without it. Two FBX objects — object numbers 25769803781 and
25769803782, two `Material`s with different colours — were given one token:

```text
1 x Identifier uniqueness violation: 'Name:fcad~c011~occ, Type:Mesh'
subassets 50, not 51
one GameObject silently disambiguated to 'fcad~c011~occ 1'
the two Materials collapsed onto ONE local file identifier
```

**A token collision merges two distinct materials into one object.** It does not
refuse, it does not error, and one of the two colours is gone. Any scheme that
hashes a durable identity into a name therefore needs a stated collision policy
and a typed refusal before the first byte is written, in the same way the writer
already refuses an unrepresentable transform and an out-of-range colour. The
control probe — the same token scheme without a deliberate collision — merges
nothing, so the merge above is a property of the collision and not of the
scheme's shape.

## 5 — The twelve reimport scenarios

Each candidate tracks the same eighteen references — six meshes, five materials
and seven placements — across the same twelve document changes. `.` is a kept
reference; `x` is honestly missing because the document dropped the object; `R`
is a **silent retarget**; `M` is a null reference to an object the document
still exports; `O` is the same definition with a changed material; `?` is an
anchor the candidate's identity could not express.

Columns are `Mesh | Material | GameObject`.

| scenario | `a-control` | `b-ordinal` | `b-occurrence` | `c-property` | `d-companion` |
| --- | --- | --- | --- | --- | --- |
| byte-identical export | `....?? \| ...?? \| ...??..` | `...... \| ..... \| .......` | `...... \| ..... \| .......` | `...... \| ..... \| .......` | `...... \| ..... \| .......` |
| an unchanged document exported again | `....?? \| ...?? \| ...??..` | `...... \| ..... \| .......` | `...... \| ..... \| .......` | `...... \| ..... \| .......` | `...... \| ..... \| .......` |
| **a designation changes** | `.R.M?? \| ...?? \| RMM??..` | `...... \| ..... \| .......` | `...... \| ..... \| .......` | `...... \| ..... \| .......` | `...... \| ..... \| RMM....` |
| a definition inserted | `....?? \| MRM?? \| ...??..` | `...... \| ..... \| .......` | `...... \| ..... \| .......` | `...... \| ..... \| .......` | `...... \| ..... \| .......` |
| a definition removed | `x...?? \| RMM?? \| ...??..` | `x..... \| ..... \| .......` | `x..... \| ..... \| .......` | `x..... \| ..... \| .......` | `x..... \| ..... \| .......` |
| definitions reordered | `....?? \| ..M?? \| ...??..` | `...... \| ..... \| .......` | `...... \| ..... \| .......` | `...... \| ..... \| .......` | `...... \| ..... \| .......` |
| **a sibling inserted** | `....?? \| ...?? \| ...??..` | `...... \| ..... \| .......` | `..M... \| ..... \| .......` | `..M... \| ..... \| .......` | `..M... \| ..... \| .......` |
| a sibling removed | `....?? \| ...?? \| ...??..` | `...... \| ..... \| .......` | `...... \| ..... \| .......` | `...... \| ..... \| .......` | `...... \| ..... \| .......` |
| siblings reordered | `....?? \| ...?? \| ...??..` | `...... \| ..... \| .......` | `...... \| ..... \| .......` | `...... \| ..... \| .......` | `...... \| ..... \| .......` |
| **a definition removed outright** | `...x?? \| ...?? \| .MR??..` | `...x.. \| ..... \| ..x....` | `...x.. \| ..... \| ..x....` | `...x.. \| ..... \| ..x....` | `...x.. \| ..... \| ..x....` |
| a material changes | `....?? \| .x.?? \| ...??..` | `...... \| .O... \| .......` | `...... \| .O... \| .......` | `...... \| .O... \| .......` | `...... \| .x... \| .......` |
| a material designation reused | `....?? \| ...?? \| ...??..` | `...... \| ..... \| .......` | `...... \| ..... \| .......` | `...... \| ..... \| .......` | `...... \| ..... \| .......` |

Read across the rows:

* **The control reproduces §22B-1e1 on a larger document.** Renaming a
  designation while every durable key stays put moves a Mesh onto a different
  part and breaks two placements; inserting or removing any earlier definition
  renumbers and moves materials; removing one of two same-designation twins
  promotes the survivor into the vacated identifier, and the reference now
  denotes a part that renders correctly and is the wrong part.
* **A machine-named channel removes every one of those breaks.** The only
  things that break under `b-ordinal`, `b-occurrence` and `c-property` are
  objects the document actually dropped, plus the shared-mesh case in §3, plus
  a material whose colour genuinely changed. The `Identifier uniqueness
  violation` disappears entirely.
* **The removed object never retargets under any machine-named candidate.** It
  becomes a null reference — `missing_because_object_was_removed` — which is
  what the §22B-1e1 contract requires.
* **The companion restores the human names and the instability together.**
  Under `d-companion` a designation change breaks three placement references in
  exactly the way the control does, and a material whose designation changed
  becomes missing rather than changed. The Mesh column is unaffected, because a
  Mesh's identifier was already fixed before the rename.

Two scenarios also made the editor's own import-determinism guard fire —
`Importer(FBXImporter) generated inconsistent result for asset(...)` — on
reordering siblings and on changing a material, for every machine-named
candidate. That is Unity saying it imported one asset twice and got two
different artefacts; §22B-1e1 saw the same guard on reordering.

## Repeatability

* Two runs per mode, in four freshly created and separately deleted temporary
  projects, produced byte-identical canonical reports — including every local
  file identifier.
* `a-control` was measured in both modes and its reports agree exactly, so the
  companion postprocessor is inert on a document that carries no designation
  and the two modes may be compared.
* The independent `ufbx` reader and the editor agree on byte count and FNV-1a
  for every file side of every scenario and every name probe.
* The recorded vanilla report carries 18 532 editor-side checks, the companion
  one 9 179, and the join verifier adds 16 810.

## Mutations

Twenty-five semantic mutants are applied to copies of the recorded measurement
and fed to the real join verifier: a gate that only checks non-null; the
`ImportedSourceId` deleted from a candidate's files; the source-local key
reported as global; a companion result labelled vanilla; a companion candidate
planned into the vanilla run; the visible names never recorded; a machine-named
candidate reported as human-named; the rename timing measured on one side only;
a Mesh result reported as a Material result; the ordinal reported as a durable
occurrence identity, and declared as one in the plan; the token collision
ignored; a retarget of a removed object accepted; the oracle reading a different
file; a prewritten report; a mandatory scenario skipped; the probe's importer
setting allowed to move identifiers; a run that tracked nothing; the two
resolutions allowed to disagree; the stored pair no longer being the object's
identity; the companion allowed to change the control; the multi-source
collision removed from the document; a sub-asset identifier that is not the
stored pair; an ambiguous join reported as a kept reference; and the shared-mesh
break reported as a kept reference. All twenty-five are killed.

Eight more are put into the real generator, the real rewriter, the real runner
and the real Unity probe, compiled where they are code, and run against the real
editor. Each is judged with the two byte-for-byte comparisons against the
recorded measurement switched off, so a mutant is killed by a check that
understands what is wrong with it rather than by "these bytes are not the
recorded bytes":

| mutant | what killed it |
| --- | --- |
| the `ImportedSourceId` removed from the channel | the candidate does not carry the definition identity it claims |
| the control rewritten instead of copied | the control is not the production writer's bytes |
| the oracle given a different file | the oracle reported one file twice |
| a verdict that only asks whether something resolved | a reference was called kept while its meaning changed |
| an ambiguous join tracked as if it were not | the candidate's ambiguity count and its tracked anchors disagree |
| the visible names never recorded | the recorded visible node names are not the names the import published |
| the companion rename left out | a candidate that needs the companion was planned into a vanilla run |
| the project GUID left untokenised | **two clean Unity projects produced different canonical reports** |

The last one is killed by nothing except the second clean project, which is how
this harness shows that second project is load-bearing rather than decorative.

**One mutant genuinely survived.** `the_visible_names_never_recorded`, in its
first form, emptied only the node-name list; the verifier still classified the
candidate from the mesh and material lists and saw nothing wrong. That is a
survivor, and it is named as one: the gate did not understand the defect. The
verifier now rebuilds all three lists from the report's own node table and
refuses a summary that disagrees with them, and the mutant is killed by that
check. No other mutant survived, and no survivor was renamed into a kill.

The harness separately refuses an anchor that matches nothing, an anchor that
matches twice, a stale `.mutbak`, a probe that does not compile, a prewritten
report with no execution anchor, a zero-check run, and an imported file left in
the repository. The non-compiling probe is explicitly not counted as a kill, and
a missing editor returns its own exit code that is neither a kill nor a
survivor.

The half that needs no editor and no kernel — re-joining both recorded runs to
the independent `ufbx` reading, rebuilding the decision record and running the
twenty-five semantic mutants — is
`tools/unity-identity-channel/scripts/check_channel_record.sh`, and CI runs it
on every push.

## Honest limits

* **The synthetic occurrence identities are synthetic.** They exist inside this
  measurement only. FerriteCAD persists no occurrence identity, and this slice
  changed no schema to give it one.
* **The two "export again" scenarios are byte-identical to their base**, because
  the writer is a function of the scene. They confirm rather than add.
* **The real AP203 assembly was not measured here.** §22B-1e1 measured it, and
  it contains one source; the multi-source question cannot be asked of it
  without editing a STEP corpus, which is not this slice. What that assembly
  contributes is the Cyrillic designation used verbatim in the name probes.
* **A native body's `ObjectId` was not measured.** Every definition in these
  documents is imported.
* **The companion postprocessor here is a probe, not a package.** It is thirty
  lines that read three custom properties and assign three names. A shipped
  FerriteCAD Unity package would have to be versioned, distributed, installed
  before the first import, and kept working across editor versions — none of
  which is measured. What *is* measured is that its rename works and what it
  costs.
* **The rename is measured on `OnPostprocessModel` only.** Other importer hooks,
  a `ScriptedImporter`, and a `.meta`-file `internalIDToNameTable` were not
  tried, and this measurement says nothing about them.
* **The model root's visible name could not be changed from the FBX**, and the
  companion did not change it either. Whether some other hook can was not
  measured.
* **The inferred name rules are measured models, not Unity source.** They
  explain every one of the 1 296 recorded transitions and the 54 files the
  oracle read. They do not prove no other importer path contributes.
* **One editor version, one platform**: `6000.4.10f1` on arm64 macOS.
* **No prefab or scene was authored.** The reference holder is a
  `ScriptableObject`, which stores references in the same `{fileID, guid, type}`
  form a prefab uses; a prefab's own additional remapping was not exercised.
* **The separator `~` was not itself a variable.** Every candidate uses it, and
  no measurement here says anything about a different one.

## Stop and report

Five of the brief's seven stop conditions are met, which is already enough to
stop. The token was neither truncated nor normalised, and the clean projects
did not disagree. The collision row below is an additional unsafe outcome, not
an eighth stop condition from the brief. No product policy is chosen here.

| condition | met | what was measured |
| --- | --- | --- |
| the measured vanilla FBX channels do not separate a stable identity from the visible name | **yes** | a vanilla import shows exactly the FBX object name in every measured candidate; a candidate that stabilises references shows machine tokens for every node, mesh and material; other FBX graph topologies were not tested |
| a FerriteCAD Unity companion postprocessor would be needed | **yes** | the designation reaches a person only through `OnPostprocessModel`; the custom property alone is invisible in a stock editor |
| the postprocessor's rename moves local identifiers | **yes, for two kinds of three** | `GameObject` 239/239 moved, `Material` 191/191 moved, `Mesh` 0/191 moved |
| one postprocessor rename cannot preserve Model, Mesh and Material alike | **yes** | three different answers to the rename question; in the measured flat graph a Mesh is named after a Model node rather than its own Geometry |
| a new durable occurrence identity or a schema change is needed | **yes, and it is not sufficient** | a placement has no persisted identity at all today; adding one makes placements nameable and does not fix a shared mesh |
| a source-qualified token is truncated or normalised | **no** | 271 bytes and Cyrillic survive intact; NFC and NFD stay two objects |
| a token collision is safe | **no** | two materials with different colours merged onto one identifier, with one warning and no refusal |
| two clean Unity projects disagree | **no** | four clean projects, byte-identical canonical reports |

## The decision table

Four options, what each is proved to do, what it is not proved to do, what a
user would see, and what it would take. **The choice is the user's.**

### A — stable Unity references, machine-visible names

`b-occurrence` or `c-property`, shipped as they are measured.

* **Proved**: the source-qualified identity removes the multi-source collision;
  all references required to survive do so except one shared Mesh when an
  earlier sibling is inserted; the `Identifier uniqueness violation`
  disappears entirely; a removed object becomes a null reference and never
  retargets; a 271-byte and a Cyrillic token pass through the editor unchanged;
  two clean projects agree byte for byte.
* **Proved against it**: the shared Mesh does not survive that sibling insertion
  under `b-occurrence` or `c-property`.
* **Not proved**: that anything works on a document with a native body; that a
  different FBX graph, `.meta` remapping or another importer hook could recover
  the designations and the missing shared-Mesh stability.
* **What a user sees**: a hierarchy of
  `fcad~019ffc72-2996-7000-8000-0000000000a1~step.product_definition#100~occ~019ffc72-…`
  in the Project window, the Inspector, the scene, and every prefab they build.
  The designation is in the file and nowhere a person looks.
* **What it takes**: a writer change to emit `ImportedSourceId` in the name and
  a canonical export form for it; a stated collision policy with a typed refusal;
  a durable occurrence identity in the document schema.

### B — human names, unstable references

Today's behaviour, `a-control`, left alone.

* **Proved**: names read correctly; and renaming a designation while every
  durable key is unchanged silently moves a Mesh reference onto a different
  part, breaks two placement references, and makes the warning *disappear* as it
  happens. Inserting or removing any unrelated definition moves materials.
  Removing one of two same-designation twins hands its identifier to the
  survivor. On a document with two sources, six of eighteen anchors cannot be
  named at all.
* **Not proved**: nothing needs proving; this is the measured status quo.
* **What a user sees**: correct names, and a project that silently repoints at
  the wrong part after an edit that changed no identity.
* **What it takes**: nothing, and it leaves §22B-1e1's finding unfixed.

### C — a FerriteCAD Unity companion package

`d-companion`: `c-property`'s bytes plus an `AssetPostprocessor`.

* **Proved**: the rename works — every node, mesh and material shows its
  designation; it is deterministic and identical in two clean projects; it is
  inert on a document carrying no designation; a Mesh's identifier is fixed
  before the rename and therefore survives it.
* **Proved against it**: a `GameObject`'s and a `Material`'s identifier are
  computed *after* the rename, so a designation change breaks three placement
  references exactly as the control does, a changed material's reference goes
  missing rather than changing, and the uniqueness warning returns on four
  object kinds the control never warned about — because the rename creates
  duplicates that Unity had already disambiguated. The model root keeps the
  asset's name regardless.
* **Not proved**: that a shipped, versioned, installable package behaves as this
  thirty-line probe does; that any user would have it installed before their
  first import; that a different hook (`.meta` name table, `ScriptedImporter`)
  avoids the identifier move.
* **What a user sees**: correct names, and the same instability as B for
  placements and materials, with meshes fixed.
* **What it takes**: a real Unity package, a distribution and version policy, a
  "what happens before it is installed" answer, and the writer and schema work
  of option A underneath it.

### D — a new durable occurrence identity and schema change as a prerequisite

* **Proved**: a placement has no persisted identity today; the ordinal used in
  its place is positional, and §22B-1e1 measured positional keys moving for no
  reason. A durable occurrence identity makes a placement nameable without an
  ordinal, and every surviving placement reference survives every scenario
  under it.
* **Proved against it as a complete answer**: it does not fix a shared Mesh, and
  in the one scenario that separates the two schemes it loses a mesh reference
  the ordinal keeps — because Unity names a Mesh after the first Model node that
  references it, and that node's name must distinguish placements.
* **Not proved**: that any FBX-side naming can give a shared Mesh an identity
  that is a function of its definition. Nothing measured here does.
* **What a user sees**: nothing directly; this is a prerequisite, not a feature.
* **What it takes**: a document schema change, persistence, migration of
  existing documents, and a decision about what an occurrence identity means
  when a STEP source is re-imported.

### What no option achieves

None of the four satisfies the whole reference-stability and human-name
contract. In particular, the machine-name candidates that fix the other
identity failures lose one shared Mesh when an earlier placement is inserted;
the human-name control remains unstable elsewhere. That is what the measured
flat FBX graph does. A different graph or Unity remapping/importer mechanism
remains an unmeasured route rather than a proven impossibility.
