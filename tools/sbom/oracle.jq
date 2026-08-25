# SPDX-License-Identifier: MIT
#
# The independent answer about the Rust fragment.
#
# The generator could agree with itself about the wrong graph, so nothing here
# reads anything the generator produced except the fragment under test. What
# the product's composition ought to be is rebuilt from `cargo metadata` for
# the same target, from Cargo.lock, and from the product roots that
# tools/notices/lib.sh owns.
#
# It does not re-implement Cargo's feature resolution and does not pretend to.
# It brackets the answer from both sides instead:
#
#   * an upper bound - the target-filtered normal-dependency closure of the two
#     roots. A package outside it is not in the product on this target at all,
#     whatever features are on.
#   * a lower bound - the closure over *non-optional* normal dependencies. No
#     feature can remove one of those, so every package and every edge in it
#     must be in the fragment.
#
# Between the two bounds sit exactly the optional dependencies, and those are
# checked by identity rather than by membership. The limitation is real and is
# recorded in the implementation plan rather than papered over.
#
# Input: the fragment. Output: one `bounds` line saying how much of the answer
# each bracket actually pinned, then one line per finding. Agreement is the
# `bounds` line alone.

def lines: split("\n") | map(select(length > 0));
def rows: lines | map(select(startswith("#") | not)) | map(split("\t"));
def labelof($pkgs; $id): ($pkgs[$id] | .name + "@" + .version);
# `$list | index(.)` would ask whether $list contains $list: the pipe
# rebinds `.` before index() ever sees the value being looked for.
def among($list): . as $x | ($list | index($x)) != null;

def reach($edges; $start):
  {seen: (reduce $start[] as $s ({}; .[$s] = true)), frontier: $start}
  | until((.frontier | length) == 0;
      .seen as $seen
      | ([.frontier[] as $n | ($edges[$n] // [])[]]
         | map(select($seen[.] | not)) | unique) as $new
      | {seen: (reduce $new[] as $s ($seen; .[$s] = true)), frontier: $new})
  | .seen | keys;

. as $frag
| ($md[0]) as $meta
| (reduce $meta.packages[] as $p ({}; .[$p.id] = $p))           as $pkgs
| ($rootspec | rows | map({bin: .[0], pkg: .[1]}))              as $rootrows
| ($lock | rows
   | map({key: (.[0] + "@" + .[1]), source: .[2], checksum: .[3]}))
                                                                as $lockrows
| (reduce $lockrows[] as $r ({}; .[$r.key] = $r))               as $lock_by_key
| ($risk | rows | map(.[0] + "\t" + .[1] + "@" + .[2]) | unique) as $riskkeys
| ($meta.workspace_members | map(labelof($pkgs; .)))            as $wsmembers

# --- what cargo says the product is ---------------------------------------

| def edges($kinds):
    reduce ($meta.resolve.nodes[]) as $n ({};
      .[$n.id] = [$n.deps[]
                  | select(any(.dep_kinds[]; .kind as $k | ($kinds | any(. == $k))))
                  | .pkg]);

  ([$rootrows[] as $r
    | ($meta.workspace_members[] | select($pkgs[.].name == $r.pkg))]) as $rootids
| (if ($rootids | length) != ($rootrows | length)
   then ["the workspace does not hold exactly one package per declared product root"]
   else [] end) as $rootproblem

| (reach(edges([null]); $rootids) | map(labelof($pkgs; .)))              as $upper
| (reach(edges([null, "build"]); $rootids) | map(labelof($pkgs; .)))     as $normal_build
| (reach(edges([null, "build", "dev"]); $rootids) | map(labelof($pkgs; .)))
                                                                        as $everything
| ($everything - $normal_build)                                         as $devonly

# An edge Cargo cannot drop.
#
# The obvious rule - match the surviving resolve edge to the manifest entry
# whose `target` cfg it carries - is wrong, and measuring it is what showed
# that. `cargo metadata --filter-platform` prunes the *dependency*, not the
# `dep_kinds` entries under it: on x86_64-unknown-linux-gnu, rustix's edge to
# errno still lists all three of its declarations, `cfg(windows)` included.
# Matching by cfg therefore let a Windows-only, non-optional declaration make a
# Linux dependency look mandatory, and the first run of this oracle duly
# demanded errno in the Linux fragment, where cargo builds nothing of the sort.
#
# Deciding which cfg actually applies would mean evaluating cfg expressions
# here, which is a second implementation of something Cargo already owns. The
# rule used instead needs no cfg evaluation and is sound: the edge survived
# filtering, so *some* declaration of it applies; if every normal declaration
# of that package name is non-optional, then whichever one applies is
# non-optional too. A package declared optional anywhere falls between the
# bounds instead of being demanded.
| ([$meta.resolve.nodes[] as $n
    | ($pkgs[$n.id].dependencies) as $decls
    | $n.deps[] as $d
    | ($pkgs[$d.pkg].name) as $qname
    | select(any($d.dep_kinds[]; .kind == null))
    | ([$decls[] | select(.name == $qname and .kind == null)]) as $matching
    | select(($matching | length) > 0)
    | select(all($matching[]; (.optional // false) | not))
    | {from: $n.id, to: $d.pkg}] | unique)                              as $must_edges
| (reduce $must_edges[] as $e ({}; .[$e.from] = ((.[$e.from] // []) + [$e.to])))
                                                                        as $must_adj
| (reach($must_adj; $rootids))                                          as $must_ids
| ($must_ids | map(labelof($pkgs; .)))                                  as $lower
| ([$must_edges[] | select(.from | among($must_ids))
    | {from: labelof($pkgs; .from), to: labelof($pkgs; .to)}] | unique) as $lower_edges
| ([$meta.resolve.nodes[] as $n | $n.deps[] as $d
    | select(any($d.dep_kinds[]; .kind == null))
    | (labelof($pkgs; $n.id) + " " + labelof($pkgs; $d.pkg))] | unique) as $possible_edges

# --- what the fragment says ------------------------------------------------

| ($frag.components // [])                                              as $comps
| ("ferritecad:rust-fragment:" + $target)                               as $fragref
| def keyof: ."bom-ref" | split("#") | last;
  ($comps | map(keyof))                                                 as $fragkeys
| (reduce $comps[] as $c ({}; .[$c."bom-ref"] = ((.[$c."bom-ref"] // 0) + 1)))
                                                                        as $refcount
| (reduce $comps[] as $c ({}; .[($c | keyof)] = ((.[($c | keyof)] // 0) + 1)))
                                                                        as $keycount
| (reduce $comps[] as $c ({}; .[$c."bom-ref"] = ($c | keyof)))          as $key_by_ref
| ([($frag.dependencies // [])[] | select(.ref != $fragref)
    | .ref as $from | (.dependsOn // [])[]
    | {from: $key_by_ref[$from], to: $key_by_ref[.]}])                  as $rawedges
# A `dependsOn` naming no component is a broken document. Separated out so it
# is reported by name: left in, it would abort this program on the first
# attempt to print the edge, and an abort says less than a finding does.
| ([$rawedges[] | select(.from != null and .to != null)])               as $fragedges
| ([$rawedges[] | select(.from == null or .to == null)])                as $danglingrefs
| ([($frag.dependencies // [])[] | select(.ref == $fragref) | (.dependsOn // [])[]]
   | map($key_by_ref[.]) | map(select(. != null)))                      as $fragroots
| def propval($c; $n):
    (($c.properties // []) | map(select(.name == $n)) | .[0].value // null);
  def metaprop($n):
    ((($frag.metadata.properties) // []) | map(select(.name == $n)) | .[0].value // null);

# --- findings --------------------------------------------------------------

  $rootproblem

+ (if $frag.bomFormat != "CycloneDX" then ["bomFormat is not CycloneDX"] else [] end)
+ (if $frag.specVersion != $spec
   then ["specVersion is " + ($frag.specVersion | tostring) + ", not " + $spec] else [] end)
+ (if $frag.version != 1 then ["version is not 1"] else [] end)
+ (if ($frag | has("serialNumber")) then ["the fragment carries a serialNumber"] else [] end)
+ (if ($frag.metadata | has("timestamp")) then ["the fragment carries a timestamp"] else [] end)

# it has to say what it is, and what it is not
+ (if $frag.metadata.component."bom-ref" != $fragref
   then ["the fragment's own bom-ref is not " + $fragref] else [] end)
+ (if (($frag.metadata.component.description // "") | test("not a complete product SBOM"))
   then []
   else ["the fragment does not say in its own description that it is not a complete product SBOM"] end)
+ (if metaprop($ns + ":kind") != "rust-fragment"
   then ["the fragment does not declare kind=rust-fragment"] else [] end)
+ (if metaprop($ns + ":complete") != "false"
   then ["the fragment does not declare complete=false"] else [] end)
+ (if metaprop($ns + ":pending-merge") != "native-libraries-and-assets"
   then ["the fragment does not declare that the native and asset merge is still pending"] else [] end)
+ (if metaprop($ns + ":edge-kinds") != "normal"
   then ["the fragment does not declare which dependency kinds it covers"] else [] end)
+ (if metaprop($ns + ":target") != $target
   then ["the fragment declares target " + (metaprop($ns + ":target") | tostring)
         + ", not " + $target] else [] end)
+ (if metaprop($ns + ":fragment-format") != $format
   then ["the fragment does not declare fragment-format " + $format] else [] end)

# both shipped binaries, and only those, are its roots
+ [$rootrows[] as $r
   | ($comps | map(select(propval(.; $ns + ":binary") == $r.bin))) as $found
   | select(($found | length) != 1)
   | "the fragment does not hold exactly one component for the shipped binary " + $r.bin]
+ [$rootrows[] as $r
   | ($comps | map(select(propval(.; $ns + ":binary") == $r.bin)) | .[0]) as $c
   | select($c != null)
   | if (($c | keyof) | split("@")[0]) != $r.pkg
     then "the component for " + $r.bin + " is not built from package " + $r.pkg
     elif $c.type != "application"
     then "the component for " + $r.bin + " is not of type application"
     elif (($fragroots | index($c | keyof)) == null)
     then "the fragment does not depend on its own root " + $r.bin
     else empty end]
+ (if ($fragroots | length) != ($rootrows | length)
   then ["the fragment names " + ($fragroots | length | tostring) + " roots, not "
         + ($rootrows | length | tostring)]
   else [] end)

# every component is a cargo package this Cargo.lock pins, said the same way
+ [$comps[] as $c
   | ($c | keyof) as $k
   | if (($lock_by_key | has($k)) | not)
     then "Cargo.lock does not hold " + $k
     elif $c.version != ($k | split("@")[1:] | join("@"))
     then "the component " + $k + " states version " + ($c.version | tostring)
     elif ((propval($c; $ns + ":source") // "")
           != (if $lock_by_key[$k].source == "" then "workspace"
               else $lock_by_key[$k].source end))
     then "the component " + $k + " states a source Cargo.lock does not"
     elif ($lock_by_key[$k].checksum != "")
          and (([($c.hashes // [])[] | select(.alg == "SHA-256") | .content] | .[0])
               != $lock_by_key[$k].checksum)
     then "the component " + $k + " does not carry Cargo.lock's SHA-256"
     elif ($lock_by_key[$k].checksum == "") and ((($c.hashes // []) | length) > 0)
     then "the workspace component " + $k + " carries a crate digest"
     elif (($c.purl | startswith("pkg:cargo/")) | not)
     then "the component " + $k + " is not identified by a cargo purl"
     elif (($c."bom-ref" | test("^(registry|path|git)[+]")) | not)
     then "the component " + $k + " has a bom-ref naming no cargo source: " + $c."bom-ref"
     elif ($lock_by_key[$k].source | startswith("git+"))
          and (((propval($c; $ns + ":git-commit") // "") | test("^[0-9a-f]{40}$")) | not)
     then "the git component " + $k + " is not identified by an exact commit"
     else empty end]

# one package is one component
+ [$refcount | to_entries[] | select(.value > 1)
   | "the bom-ref " + .key + " is used by " + (.value | tostring) + " components"]
+ [$keycount | to_entries[] | select(.value > 1)
   | "the package " + .key + " appears as " + (.value | tostring) + " components"]

# membership, bracketed
+ [$fragkeys[] | select(among($upper) | not)
   | "the fragment holds " + . + ", which the product cannot reach on " + $target]
+ [$lower[] | select(among($fragkeys) | not)
   | "the fragment is missing " + . + ", which the product reaches without any optional feature"]
+ [$fragkeys[] | select(among($devonly))
   | "the fragment holds " + . + ", which is reachable only as a dev-dependency"]
+ [$wsmembers[] | select(among($upper) | not)
   | select(among($fragkeys))
   | "the fragment holds the workspace package " + . + ", which neither shipped binary reaches"]

+ (if ($danglingrefs | length) > 0
   then ["the fragment has " + ($danglingrefs | length | tostring)
         + " dependency references that name no component"]
   else [] end)

# edges, bracketed the same way
+ [$lower_edges[] as $e
   | select(([$fragedges[] | select(.from == $e.from and .to == $e.to)] | length) == 0)
   | "the fragment is missing the edge " + $e.from + " -> " + $e.to]
+ [$fragedges[] as $e
   | select(($possible_edges | index($e.from + " " + $e.to)) == null)
   | "the fragment claims an edge cargo does not have: " + $e.from + " -> " + $e.to]

# nothing floats: every component hangs off one of the two binaries
+ ((reach((reduce $fragedges[] as $e ({}; .[$e.from] = ((.[$e.from] // []) + [$e.to])));
          $fragroots)) as $connected
   | [$fragkeys[] | select(among($connected) | not)
      | "the fragment holds " + . + " with no path from either shipped binary"])

# ADR 0003: an unresolved licence question is recorded, and recorded accurately
+ [$comps[] as $c
   | ($c | keyof) as $k
   | ($lock_by_key[$k].source // "") as $src
   | (($riskkeys | index($src + "\t" + $k)) != null) as $listed
   | ((propval($c; $ns + ":licence-risk")) != null) as $marked
   | if $listed and (($marked) | not)
     then "the component " + $k + " is on the known licence risk inventory and is not marked"
     elif (($listed) | not) and $marked
     then "the component " + $k + " is marked as a licence risk but is not on the inventory"
     else empty end]

# How much the two brackets actually pinned. A lower bound that turned out to
# be nearly empty would make most of the checks above vacuous, and that would
# be invisible in a green run unless it is printed.
| ["bounds\t" + ($lower | length | tostring) + " packages and "
   + ($lower_edges | length | tostring) + " edges are demanded outright, out of "
   + ($upper | length | tostring) + " the product could reach"] + .

| .[]
