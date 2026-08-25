# SPDX-License-Identifier: MIT
#
# Builds the normalized CycloneDX 1.5 Rust fragment.
#
# What a component *is* comes from cargo-cyclonedx. Which components there are,
# and which depends on which, comes from cargo's own feature-aware resolver.
# A package's source and digest come from Cargo.lock. Nothing here invents any
# of the three, and where two of them disagree this refuses rather than
# choosing between them.

def lines: split("\n") | map(select(length > 0));
def rows: lines | map(select(startswith("#") | not)) | map(split("\t"));
def die($msg): ($msg | halt_error(1));
def nameof($key): $key | split("@")[0];
def versionof($key): $key | split("@")[1:] | join("@");

# The purl is the only field on a cargo-cyclonedx record that always carries
# the *package* name: the document describing a binary names its own component
# after the binary.
def keyof: .purl | split("?")[0] | ltrimstr("pkg:cargo/");

($nodes | lines | unique)                                as $members
| ($edges | rows | map({from: .[0], to: .[1]}) | unique)  as $graph
| ($roots | rows | map({bin: .[0], key: .[1]}))           as $rootrows
| ($paths | rows | map({key: .[0], dir: .[1]}))           as $pathrows
| ($lock  | rows
   | map({key: (.[0] + "@" + .[1]), source: .[2], checksum: .[3]}))
                                                          as $lockrows
| ($risk  | rows | map(.[0] + "\t" + .[1] + "@" + .[2]) | unique)
                                                          as $riskkeys

# A key that named two packages would let a component describe one of them and
# be checked against the other.
| (reduce $lockrows[] as $r ({}; .[$r.key] = ((.[$r.key] // 0) + 1))
   | to_entries | map(select(.value > 1) | .key))          as $dupes
| (if ($dupes | length) > 0
   then die("Cargo.lock holds more than one package called " + ($dupes | join(", ")))
   else . end)
| (reduce $lockrows[] as $r ({}; .[$r.key] = $r))          as $lock_by_key
| (reduce $pathrows[] as $r ({}; .[$r.key] = $r.dir))      as $dir_by_key

# An edge whose endpoint is not a member would leave a null in `dependsOn`,
# which is a broken document rather than a smaller one. Found by a mutation
# that removed a package from the membership and left its edges behind: the
# output still parsed, and it took the schema to notice the nulls.
| ([$graph[] as $e | select((([$e.from, $e.to]) - $members) != [])]) as $dangling
| (if ($dangling | length) > 0
   then die("the graph has edges whose endpoints are not members: "
            + ($dangling | map(.from + " -> " + .to) | join(", ")))
   else . end)

# --- the records cargo-cyclonedx produced ----------------------------------

| ([$boms[] | [.metadata.component] + (.components // [])]
   | add | map({key: keyof, rec: .}))                      as $records
| (reduce $records[] as $r ({}; .[$r.key] = ((.[$r.key] // []) + [$r.rec])))
                                                           as $by_key

# A package reached from both binaries is one package. Two records that
# disagree about it are a defect in the merge, not a choice to be made.
| ([$by_key | to_entries[]
   | select((.value | map(tojson) | unique | length) > 1) | .key]) as $conflicts
| (if ($conflicts | length) > 0
   then die("the two binaries describe the same package differently: "
            + ($conflicts | join(", ")))
   else . end)
| ($by_key | with_entries(.value = .value[0]))             as $rec_by_key

| ([$members[] | select($rec_by_key[.] == null)])          as $undescribed
| (if ($undescribed | length) > 0
   then die("cargo's resolver reaches packages cargo-cyclonedx did not describe: "
            + ($undescribed | join(", ")))
   else . end)

# --- stable identity -------------------------------------------------------
#
# The reference of a registry or git package is its Cargo.lock source and its
# exact version. The reference of a workspace package is its path inside this
# repository, taken from cargo metadata's own workspace root rather than from
# whatever absolute path the generating host happened to have.

| def refof($key):
    ($lock_by_key[$key] // die("Cargo.lock does not hold " + $key)) as $l
    | if $l.source == "" then
        (($dir_by_key[$key] // die("no workspace path for " + $key)) as $d
         | if ($d | test("^([A-Za-z]:)?/")) then
             die("the workspace package " + $key + " resolved to an absolute path: " + $d)
           else "path+" + $d + "#" + $key
           end)
      else $l.source + "#" + $key
      end;

  def purlof($key):
    ($lock_by_key[$key].source) as $src
    | if ($src | startswith("git+")) then $rec_by_key[$key].purl
      else ($rec_by_key[$key].purl | split("?")[0])
      end;

# A git dependency is identified by its commit or not at all. This Cargo.lock
# holds none today; the rule is enforced rather than assumed so that the first
# one cannot arrive as a branch name.
  ([$members[] | select(($lock_by_key[.].source // "") | startswith("git+"))
    | select(($lock_by_key[.].source | test("#[0-9a-f]{40}$")) | not)]) as $vague
| (if ($vague | length) > 0
   then die("a git dependency is not pinned to a commit: " + ($vague | join(", ")))
   else . end)

# --- properties ------------------------------------------------------------

| def prop($n; $v): {name: ($ns + ":" + $n), value: $v};

  def component_props($key):
    ($lock_by_key[$key]) as $l
    | ([ prop("source"; (if $l.source == "" then "workspace" else $l.source end)) ]
    + (if ($l.source | startswith("git+"))
       then [prop("git-commit"; ($l.source | capture("#(?<c>[0-9a-f]{40})$").c))]
       else [] end)
    + [$rootrows[] | select(.key == $key) | prop("binary"; .bin)]
    + [$rootrows[] | select(.key == $key) | prop("cargo-package"; nameof($key))]
    # ADR 0003: an unresolved licence question is recorded, never a refusal.
    + (if ($riskkeys | index($l.source + "\t" + $key)) != null
       then [prop("licence-risk"; "KNOWN LICENCE RISK"),
             prop("licence-risk-inventory"; "tools/notices/declared-only.tsv")]
       else [] end))
    | sort_by(.name, .value);

# --- components ------------------------------------------------------------

  def hashes_for($key):
    ($lock_by_key[$key].checksum) as $sum
    | ([($rec_by_key[$key].hashes // [])[] | select(.alg == "SHA-256") | .content]) as $given
    | if $sum == "" then
        (if ($given | length) > 0
         then die("cargo-cyclonedx gave a crate digest for the workspace package " + $key)
         else null end)
      else
        (if ($given | map(select(. != $sum)) | length) > 0
         then die("cargo-cyclonedx and Cargo.lock disagree about the digest of " + $key)
         else [{alg: "SHA-256", content: $sum}]
         end)
      end;

  def component($key):
    $rec_by_key[$key] as $r
    | ({
        type: $r.type,
        "bom-ref": refof($key),
        name: $r.name,
        version: versionof($key),
        purl: purlof($key),
        scope: ($r.scope // "required"),
        properties: component_props($key)
      }
      + (if $r.author then {author: $r.author} else {} end)
      + (if $r.description then {description: $r.description} else {} end)
      + (if $r.licenses then {licenses: ($r.licenses | sort_by(tojson))} else {} end)
      + (if (($r.externalReferences // []) | length) > 0
         then {externalReferences: ($r.externalReferences | sort_by(.type, .url))}
         else {} end)
      + (hashes_for($key) as $h
         | if $h == null then {} else {hashes: $h} end));

  ("ferritecad:rust-fragment:" + $target)              as $fragment_ref
| ([$rootrows[] | refof(.key)] | unique)               as $root_refs
| (reduce $members[] as $m ({}; .[$m] = refof($m)))    as $ref_by_key
| ([$rootrows[] | versionof(.key)] | unique)           as $versions
| (if ($versions | length) != 1
   then die("the two shipped binaries do not share a version: " + ($versions | join(", ")))
   else . end)

# --- the document ----------------------------------------------------------

| {
    bomFormat: "CycloneDX",
    specVersion: $spec,
    version: 1,
    metadata: {
      tools: [
        {vendor: "CycloneDX", name: "cargo-cyclonedx", version: $tool},
        {vendor: "FerriteCAD", name: "generate-rust-sbom.sh", version: $format}
      ],
      component: {
        type: "application",
        "bom-ref": $fragment_ref,
        name: "ferritecad-rust-fragment",
        version: $versions[0],
        description: ("Intermediate CycloneDX fragment. Describes only the Rust components of the FerriteCAD product for "
                      + $target
                      + ". This is not a complete product SBOM: the native libraries and the assets are not merged into it.")
      },
      properties: ([
        {name: "cdx:rustc:sbom:target:triple", value: $target},
        prop("complete"; "false"),
        prop("edge-kinds"; "normal"),
        prop("fragment-format"; $format),
        prop("kind"; "rust-fragment"),
        prop("pending-merge"; "native-libraries-and-assets"),
        prop("roots"; ([$rootrows[] | .bin] | sort | join(","))),
        prop("target"; $target)
      ] | sort_by(.name, .value))
    },
    components: ([$members[] | component(.)] | sort_by(."bom-ref")),
    dependencies: (([{ref: $fragment_ref, dependsOn: ($root_refs | sort)}]
      + [$members[] as $m
         | {ref: $ref_by_key[$m],
            dependsOn: ([$graph[] | select(.from == $m) | $ref_by_key[.to]] | unique)}])
      | sort_by(.ref))
  }
