# SPDX-License-Identifier: MIT
#
# The independent answer about the merged product SBOM.
#
# The merge program could agree with itself about the wrong document, so
# nothing here reads anything the merge produced except the document under
# test. What the product SBOM ought to say is asked of the two inputs directly:
# the Rust CycloneDX fragment for this target and the native/assets inventory.
# Neither of those is generated here and neither is modified by the slice that
# reads them.
#
# It is not a second merge. It never builds the document it expects; it asks
# the finished one a list of questions that the two inputs answer between them,
# and every answer it disagrees with is a line of output.
#
# Input: the product SBOM. Output: one `bounds` line saying how much was
# actually compared, then one line per finding. Agreement is the `bounds` line
# alone.

def among($list): . as $x | ($list | index($x)) != null;
def propvals($c; $name): [($c.properties // [])[] | select(.name == $name) | .value];
def propval($c; $name): (propvals($c; $name) | first);

def reach($edges; $start):
  {seen: (reduce $start[] as $s ({}; .[$s] = true)), frontier: $start}
  | until((.frontier | length) == 0;
      .seen as $seen
      | ([.frontier[] as $n | ($edges[$n] // [])[]]
         | map(select($seen[.] | not)) | unique) as $new
      | {seen: (reduce $new[] as $s ($seen; .[$s] = true)), frontier: $new})
  | .seen | keys;

. as $doc
| $fragment[0] as $frag
| $inventory[0] as $inv
| ($ns + ":") as $p
| ("ferritecad:rust-fragment:" + $target) as $fragroot
| ("ferritecad:product:" + $target) as $root

| ($doc.components // [])                                   as $comps
| ([$comps[]["bom-ref"]])                                   as $refs
| (reduce $comps[] as $c ({}; .[$c["bom-ref"]] = $c))       as $byref
| ($doc.dependencies // [])                                 as $deps
| (reduce $deps[] as $d ({}; .[$d.ref] = ((.[$d.ref] // []) + ($d.dependsOn // []))))
                                                            as $graph
| ([$frag.components[]["bom-ref"]] | sort)                  as $fragrefs
| ([$inv.components[] | select(.targets | index($target))]) as $mine
| ([$mine[] | .id] | sort)                                  as $mineids
| (reduce $inv.productRoots[] as $r ({}; .[$r.binary] = $r.rustFragmentRef))
                                                            as $ref_of_binary
| ([$inv.productRoots[].rustFragmentRef] | sort)            as $rootrefs
| ([$mine[] | select(.role == "build-input") | .id])        as $buildinputs
| ([$inv.components[] | .source.sha256 // empty] | unique) as $sourcedigests
| ([$comps[] | propvals(.; $p + "staged-file")[]] | unique) as $staged

# The delivered graph: the same edges with every build input taken out. A build
# input is a thing a build reads, not a thing the product loads, and asking
# whether a binary reaches planegcs has to be asked over what is delivered. On
# Windows the import library is a real path from the crate that links planegcs
# back to planegcs itself, and reading that as delivery would be reading a
# linker's input as a runtime dependency.
| (reduce ($graph | to_entries)[] as $e ({};
     if ($e.key | among($buildinputs)) then .
     else .[$e.key] = [$e.value[] | select(among($buildinputs) | not)] end))
                                                            as $delivered

# --- what the document says about itself -----------------------------------

| [if ($doc.bomFormat // "") != "CycloneDX"
   then "the document does not call itself CycloneDX" else empty end,
   if ($doc.specVersion // "") != $spec
   then "the document claims specVersion " + ($doc.specVersion // "none")
        + " and " + $spec + " is what this repository pins" else empty end,
   if ($doc.metadata.component["bom-ref"] // "") != $root
   then "the product root is " + ($doc.metadata.component["bom-ref"] // "absent")
        + " and it must be " + $root else empty end]

+ [(propval($doc.metadata; $p + "kind")) as $kind
   | if $kind != "product"
     then "the document calls itself '" + ($kind // "nothing") + "' and not a product SBOM"
     else empty end]
+ [(propval($doc.metadata; $p + "complete")) as $c
   | if $c != "true"
     then "the merged document says complete=" + ($c // "nothing")
          + ", and the completed thing is this merge"
     else empty end]
+ [if (propvals($doc.metadata; $p + "pending-merge") | length) > 0
   then "the merged document still carries a pending-merge property" else empty end]
+ [if (propvals($doc.metadata; $p + "fragment-format") | length) > 0
   then "the merged document carries the fragment's format number, which describes an input"
   else empty end]
+ [(propval($doc.metadata; $p + "product-format")) as $f
   | if $f != $format
     then "the product format is '" + ($f // "absent") + "' and " + $format + " is current"
     else empty end]
+ [(propval($doc.metadata; $p + "target")) as $t
   | if $t != $target
     then "the document says its target is '" + ($t // "absent") + "'" else empty end]
+ [(propval($doc.metadata; $p + "roots")) as $r
   | ([$inv.productRoots[].binary] | sort | join(",")) as $want
   | if $r != $want then "the document names the roots '" + ($r // "absent")
                         + "' and the inventory names '" + $want + "'"
     else empty end]

# The inputs stay inputs. Neither may be claimed as complete by this document,
# and both must be the ones actually read.
+ [if $inv.complete != false or $inv.isProductSbom != false
   then "the native/assets inventory has been rewritten to claim it is a product SBOM"
   else empty end]
+ [(propval($frag.metadata; $p + "complete")) as $c
   | if $c != "false"
     then "the Rust fragment no longer says it is incomplete, and it is" else empty end]
+ [if (propvals($frag.metadata; $p + "pending-merge") | length) == 0
   then "the Rust fragment no longer says what it is pending" else empty end]

# --- the synthetic fragment root ------------------------------------------
#
# It exists so an incomplete document has something to hang two binaries off.
# It is not a component of the product and must not have survived the merge.

+ [if ($fragroot | among($refs)) then "the synthetic fragment root is a component of the product"
   else empty end]
+ [if $graph[$fragroot] != null then "the synthetic fragment root still owns edges" else empty end]
+ [$deps[] | select((.dependsOn // []) | index($fragroot))
   | "the edge " + .ref + " -> " + $fragroot + " points at the synthetic fragment root"]

# --- nothing declared twice, nothing dangling ------------------------------

+ [$refs | group_by(.)[] | select(length > 1)
   | "the bom-ref " + .[0] + " is declared by " + (length | tostring) + " components"]
+ [[$deps[].ref] | group_by(.)[] | select(length > 1)
   | "the dependency graph holds " + (length | tostring) + " entries for " + .[0]]
+ [$deps[] | .ref as $r | (.dependsOn // []) | group_by(.)[] | select(length > 1)
   | "the entry for " + $r + " names " + .[0] + " " + (length | tostring) + " times"]
+ ([$refs[], $root] as $known
   | [$deps[] | select((.ref | among($known)) | not)
      | "the dependency graph has an entry for " + .ref + ", which nothing declares"]
   + [$deps[] | .ref as $r | (.dependsOn // [])[]
      | select(among($known) | not)
      | "the entry for " + $r + " depends on " + . + ", which nothing declares"])
+ [$refs[] | select(($graph[.] == null))
   | "the component " + . + " has no entry in the dependency graph"]
+ [if $graph[$root] == null then "the product root has no entry in the dependency graph"
   else empty end]

# --- the Rust half, object for object and edge for edge --------------------

+ [$frag.components[] | . as $f | ($f["bom-ref"]) as $r
   | if $byref[$r] == null then "the merge lost the Rust component " + $r
     elif $byref[$r] != $f then "the merge changed the Rust component " + $r
     else empty end]
+ [$refs[] | select(test("^(registry|path|git)\\+")) | select(among($fragrefs) | not)
   | "the product SBOM invents the Rust component " + .]
+ [$frag.dependencies[] | select(.ref != $fragroot)
   | .ref as $r | (.dependsOn // []) as $was
   | ($graph[$r] // []) as $now
   | ([$was[] | select(among($now) | not)]) as $lost
   | ([$now[] | select(among($was) | not)]) as $gained
   | (if ($lost | length) > 0
      then "the merge dropped the Rust edge " + $r + " -> " + ($lost | join(", "))
      else empty end),
     ([$gained[] | select(test("^(native|asset)\\+") | not)]
      | if length > 0
        then "the merge added a non-native edge " + $r + " -> " + (join(", "))
        else empty end)]

# --- the target filter -----------------------------------------------------

+ (([$refs[] | select(test("^(native|asset)\\+"))] | sort) as $have
   | [$mineids[] | select(among($have) | not)
      | "the inventory says " + . + " belongs to " + $target + " and the merge left it out"]
   + [$have[] | select(among($mineids) | not)
      | "the merge carries " + . + ", which the inventory does not give to " + $target])

# --- what each added component says ---------------------------------------

+ [$mine[] | . as $m | ($byref[$m.id]) | select(. != null) | . as $c
   | (if (propval($c; $p + "role")) != $m.role
      then "the component " + $m.id + " is given the role '"
           + ((propval($c; $p + "role")) // "none") + "' and the inventory says " + $m.role
      else empty end),
     (if (propval($c; $p + "staged-runtime-file")) != ($m.stagedRuntimeFile | tostring)
      then "the component " + $m.id + " disagrees with the inventory about being a staged runtime file"
      else empty end),
     (if $c.name != $m.name or $c.version != $m.version
      then "the component " + $m.id + " is named " + $c.name + " " + $c.version
           + " and the inventory names it " + $m.name + " " + $m.version
      else empty end)]

# A source archive digest is the digest of an archive. Carrying it as the
# component's own hash would say the built library hashes to it, which is a
# claim about bytes nothing in this repository has ever measured.
+ [$comps[] | . as $c | ($c.hashes // [])[] | .content
   | select(among($sourcedigests))
   | "the component " + $c["bom-ref"] + " carries a source archive digest as its own artifact hash"]

# An asset digest is the digest of the asset, and belongs where it is.
+ [$mine[] | select(.role == "embedded-asset") | . as $m
   | ($byref[$m.id]) | select(. != null)
   | [(.hashes // [])[] | select(.alg == "SHA-256") | .content] as $h
   | if $h != [$m.asset.sha256]
     then "the asset " + $m.id + " does not carry its own SHA-256 as its hash"
     else empty end]

# --- loadedBy, and only where the measurement put it ------------------------

+ [$mine[] | select(.role == "runtime-native") | . as $m
   | ([$m.loadedBy[] | $ref_of_binary[.]] | sort) as $want
   | ([$rootrefs[] | select(($graph[.] // []) | index($m.id))] | sort) as $have
   | (if $want != $have
      then "the runtime component " + $m.id + " is depended on by " + ($have | join(", "))
           + " and the inventory says " + ($want | join(", "))
      else empty end),
     (if (propvals($byref[$m.id]; $p + "loaded-by") | sort) != ($m.loadedBy | sort)
      then "the loaded-by properties of " + $m.id + " do not say what the graph says"
      else empty end),
     ((($m.stagedFilenames[$target] // []) | sort) as $files
      | if (propvals($byref[$m.id]; $p + "staged-file") | sort) != $files
        then "the staged files named on " + $m.id + " are not the ones the inventory gives "
             + $target
        else empty end)]

# --- embeddedIn and containedBy, kept apart --------------------------------

+ [$mine[] | select(.role == "embedded-asset") | . as $m
   | ([$m.embeddedIn[] | $ref_of_binary[.]] | sort) as $want
   | ([$rootrefs[] | select(($graph[.] // []) | index($m.id))] | sort) as $have
   | (if $want != $have
      then "the asset " + $m.id + " is embedded in " + ($have | join(", "))
           + " and the inventory says " + ($want | join(", "))
      else empty end),
     (if (($graph[$m.containedBy] // []) | index($m.id)) == null
      then "nothing connects the asset " + $m.id + " to " + $m.containedBy
           + ", which is what contains it"
      else empty end),
     (if (propval($byref[$m.id]; $p + "contained-by")) != $m.containedBy
      then "the asset " + $m.id + " does not say which component contains it"
      else empty end),
     (if (propvals($byref[$m.id]; $p + "embedded-in") | sort) != ($m.embeddedIn | sort)
      then "the embedded-in properties of " + $m.id + " do not say what the graph says"
      else empty end),
     (if ($m.containedBy | among($rootrefs)) and (($m.embeddedIn | length) > 0)
        and ((propval($byref[$m.id]; $p + "contained-by")) == ($m.embeddedIn[0] | $ref_of_binary[.]))
      then "the asset " + $m.id + " has had containment and embedding collapsed into one relationship"
      else empty end)]

# --- build inputs, and which way they point --------------------------------

+ [$mine[] | select(.role == "build-input") | . as $m
   | ([$m.buildInputOf[]] | sort) as $want
   | ([($refs[], $root) | select(($graph[.] // []) | index($m.id))] | sort) as $have
   | (if $want != $have
      then "the build input " + $m.id + " is depended on by " + ($have | join(", "))
           + " and the inventory says " + ($want | join(", "))
      else empty end),
     (if (propvals($byref[$m.id]; $p + "build-input-of") | sort) != $want
      then "the build-input-of properties of " + $m.id + " do not say what the graph says"
      else empty end),
     (if $m.producedBy != null and ((($graph[$m.id] // []) | index($m.producedBy)) == null)
      then "nothing connects " + $m.id + " to " + $m.producedBy + ", which produces it"
      else empty end),
     (if (propval($byref[$m.id]; $p + "produced-by")) != $m.producedBy
      then "the produced-by property of " + $m.id + " does not say what the inventory says"
      else empty end),
     (if (propvals($byref[$m.id]; $p + "staged-file") | length) > 0
      then "the build input " + $m.id + " names a staged runtime file"
      else empty end)]

# A build input's own artefact must not turn up in any runtime component's file
# inventory. That is the shape a build input takes when it is delivered by
# accident: the name arrives inside a list nobody reads twice.
+ [$mine[] | select(.role == "build-input") | select(.artifactFilename != null)
   | select(.artifactFilename | among($staged))
   | "the build input " + .id + " is staged as " + .artifactFilename]

# --- what a delivered binary can actually reach ----------------------------

+ ((reach($delivered; [$root])) as $fromroot
   | [$rootrefs[] | select(among($fromroot) | not)
      | "the product root does not reach the shipped binary " + .])
+ [$mine[] | select(.role == "runtime-native") | . as $m
   | [$inv.productRoots[]
      | . as $r
      | ((reach($delivered; [$r.rustFragmentRef])) | index($m.id)) as $reached
      | (($m.loadedBy | index($r.binary)) != null) as $declared
      | if ($reached != null) != $declared
        then $r.binary + (if $declared then " does not reach " else " reaches " end) + $m.id
        else empty end]
   | if length > 0 then "the delivered graph disagrees about who loads what: " + join("; ")
     else empty end]

# Nothing floats: every component in the document hangs off the product root.
+ ((reach($graph; [$root])) as $connected
   | [$refs[] | select(among($connected) | not)
      | "the product SBOM holds " + . + " with no path from the product root"])

# How much of the answer was actually compared. A run in which the inventory
# gave this target nothing, or the fragment held no components, would agree
# with every question above and say so nowhere else.
| ["bounds\t" + ($fragrefs | length | tostring) + " Rust components and "
   + ([$frag.dependencies[] | select(.ref != $fragroot) | (.dependsOn // []) | length] | add // 0
      | tostring) + " Rust edges carried across, " + ($mineids | length | tostring)
   + " native and asset components merged in for " + $target] + .

| .[]
