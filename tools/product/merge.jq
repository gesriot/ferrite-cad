# SPDX-License-Identifier: MIT
#
# Merges one Rust CycloneDX fragment and the native/assets inventory into the
# complete product SBOM for one target.
#
# It decides the shape of the merged document and nothing about its content.
# Every Rust component object crosses unchanged, every Rust edge survives, and
# every native, build-input and asset component is the inventory's own record
# rewritten into CycloneDX without a second opinion about what it says.
#
# Two things this deliberately does not do.
#
#   * It does not carry a source archive digest as a component hash. That
#     digest is the digest of an archive; the built library's bytes are a
#     property of one build and are not in this document at all. So the digest
#     goes on an externalReference, which is where CycloneDX says a hash is a
#     hash of the resource the reference points at. An asset digest is
#     different: it is the digest of the asset itself, and that one is the
#     component's own hash.
#   * It does not keep the fragment's synthetic root. `ferritecad:rust-
#     fragment:<triple>` exists so that an incomplete document has something to
#     hang its two binaries off. It is not a component of the product, it is
#     not delivered, and the merged document's own root owns those two binaries
#     directly.

def die($msg): ($msg | halt_error(1));
def prop($name; $value): {name: $name, value: $value};

$fragment[0] as $frag
| $inventory[0] as $inv
| ($ns + ":") as $p
| ("ferritecad:rust-fragment:" + $target) as $fragroot
| ("ferritecad:product:" + $target) as $root

| (if ($frag.metadata.component["bom-ref"] // "") != $fragroot
   then die("the fragment for " + $target + " is not rooted at " + $fragroot)
   else . end)
| (if ([$inv.targets[].triple] | index($target)) == null
   then die("the inventory says nothing about " + $target)
   else . end)

# --- the product roots -----------------------------------------------------
#
# Taken from the inventory, which took them from the fragment. A binary name is
# what the inventory's relationships are written in; a bom-ref is what a
# CycloneDX edge is written in, and this is the one place the two meet.

| (reduce $inv.productRoots[] as $r ({}; .[$r.binary] = $r.rustFragmentRef))
                                                            as $ref_of_binary
| ([$inv.productRoots[].rustFragmentRef] | sort)            as $rootrefs
| ([$frag.components[]["bom-ref"]] | sort)                  as $rustrefs
| (if ([$rootrefs[] | select(. as $r | ($rustrefs | index($r)) == null)] | length) > 0
   then die("the inventory names a product root the fragment does not carry")
   else . end)
| ([$frag.metadata.properties[] | select(.name == $p + "edge-kinds") | .value]
   | first // die("the fragment does not say which edge kinds it covers"))
                                                            as $edgekinds

# --- the components this target carries ------------------------------------

| ([$inv.components[] | select(.targets | index($target))])  as $mine

| def source_reference:
    {type: "distribution",
     url: .source.url,
     hashes: [{alg: "SHA-256", content: .source.sha256}],
     comment: (if .source.kind == "built-from"
               then "the pinned source archive consumed by the build that emits this file. The digest is of that archive and is not the digest of any built artefact."
               else "the pinned source archive this component is built from. The digest is of that archive and is not the digest of any built library."
               end)};

  def source_properties:
    [prop($p + "source-pin"; .source.pin),
     prop($p + "source-kind"; .source.kind)]
    + (if .source.tag then [prop($p + "source-tag"; .source.tag)] else [] end)
    + (if .source.commit then [prop($p + "source-commit"; .source.commit)] else [] end)
    + (if .source.note then [prop($p + "source-note"; .source.note)] else [] end);

  def common_properties:
    [prop($p + "role"; .role),
     prop($p + "staged-runtime-file"; (.stagedRuntimeFile | tostring))]
    + (if .notAStagedRuntimeFile
       then [prop($p + "not-a-staged-runtime-file"; .notAStagedRuntimeFile)]
       else [] end);

  def finish:
    (.properties |= sort_by(.name, .value))
    | (if has("externalReferences") then .externalReferences |= sort_by(.type, .url)
       else . end);

  def binary_ref($name):
    ($ref_of_binary[$name]
     // die("the inventory names the binary " + $name
            + " and no product root is called that"));

# A native library the delivery carries. Its files for this target are named as
# properties: the component is one identity with a file inventory, and that is
# the model the native inventory measured rather than a convenience here.
  ([$mine[] | select(.role == "runtime-native")
   | {"bom-ref": .id,
      type: "library",
      name: .name,
      version: .version,
      scope: "required",
      externalReferences: [source_reference],
      properties: (common_properties + source_properties
        + [prop($p + "file-model"; .fileModel)]
        + [.loadedBy[] | prop($p + "loaded-by"; .)]
        + [(.stagedFilenames[$target] // [])[] | prop($p + "staged-file"; .)])}
   | finish])                                               as $runtime

# An input that only ever takes part in a build. `build-input-of` says what
# consumes it and `produced-by` says what emits it, and those are two different
# questions: the Windows import library is produced by the planegcs build and
# consumed by the one crate whose link reads it.
| ([$mine[] | select(.role == "build-input")
   | {"bom-ref": .id,
      type: (if .artifactFilename then "file" else "library" end),
      name: .name,
      version: .version,
      scope: "required",
      externalReferences: [source_reference],
      properties: (common_properties + source_properties
        + [.buildInputOf[] | prop($p + "build-input-of"; .)]
        + (if .producedBy then [prop($p + "produced-by"; .producedBy)] else [] end)
        + (if .artifactFilename
           then [prop($p + "artifact-filename"; .artifactFilename)] else [] end))}
   | finish])                                               as $inputs

# A file a product binary compiles into itself. Two relationships, and both are
# kept: the crate the file comes out of contains it, and the binary embeds it.
  | ([$mine[] | select(.role == "embedded-asset")
   | {"bom-ref": .id,
      type: "file",
      name: .name,
      version: .version,
      scope: "required",
      hashes: [{alg: "SHA-256", content: .asset.sha256}],
      properties: (common_properties
        + [prop($p + "asset-location"; .asset.location),
           prop($p + "asset-path"; .asset.path),
           prop($p + "asset-bytes"; (.asset.bytes | tostring)),
           prop($p + "contained-by"; .containedBy)]
        + (if .asset.crate then [prop($p + "asset-crate"; .asset.crate)] else [] end)
        + [.embeddedIn[] | prop($p + "embedded-in"; .)])}
   | finish])                                               as $assets

| ($runtime + $inputs + $assets)                            as $added

# --- the edges the inventory's relationships mean ---------------------------
#
# CycloneDX dependencies are untyped, so the kind of every edge added here is
# written down a second time as a property of the component the edge arrives
# at. The graph and the properties are two spellings of one fact; the gate
# checks that they agree, and neither of them is allowed to be the only record.

| ([$mine[] | select(.role == "runtime-native")
    | .id as $id | .loadedBy[] | {from: binary_ref(.), to: $id}]
   + [$mine[] | select(.role == "embedded-asset")
      | .id as $id
      | (.embeddedIn[] | {from: binary_ref(.), to: $id}),
        {from: .containedBy, to: $id}]
   + [$mine[] | select(.role == "build-input")
      | .id as $id
      | (.buildInputOf[] | {from: ., to: $id}),
        (if .producedBy then {from: $id, to: .producedBy} else empty end)]
   + [$rootrefs[] | {from: $root, to: .}])                  as $edges

| (reduce $frag.dependencies[] as $d ({};
     if $d.ref == $fragroot then . else .[$d.ref] = ($d.dependsOn // []) end))
                                                            as $base
| (reduce $added[] as $c ($base; .[$c["bom-ref"]] = (.[$c["bom-ref"]] // [])))
                                                            as $withnew
| (reduce $edges[] as $e (($withnew | .[$root] = (.[$root] // []));
     .[$e.from] = ((.[$e.from] // []) + [$e.to])))          as $graph

# --- the document ----------------------------------------------------------

| {bomFormat: "CycloneDX",
   specVersion: $spec,
   version: 1,
   metadata: {
     component: {
       "bom-ref": $root,
       type: "application",
       name: "FerriteCAD",
       version: $inv.productVersion,
       description: ("The complete FerriteCAD product for " + $target
         + ": the Rust components of both shipped binaries, the native libraries the delivery carries, the native inputs that only take part in a build, and the files those binaries embed. Nothing about it is pending.")
     },
     properties: ([
       prop("cdx:rustc:sbom:target:triple"; $target),
       prop($p + "complete"; "true"),
       prop($p + "edge-kinds"; $edgekinds),
       prop($p + "kind"; "product"),
       prop($p + "merged-from"; ($fragment_path + "@sha256:" + $fragment_sha256)),
       prop($p + "merged-from"; ($inventory_path + "@sha256:" + $inventory_sha256)),
       prop($p + "product-format"; $format),
       prop($p + "roots"; ([$inv.productRoots[].binary] | sort | join(","))),
       prop($p + "target"; $target)
     ] | sort_by(.name, .value)),
     tools: ($frag.metadata.tools
       + [{vendor: "FerriteCAD", name: "generate-native-inventory.sh",
           version: ($inv.formatVersion | tostring)},
          {vendor: "FerriteCAD", name: "generate-product-sbom.sh",
           version: $format}]
       | sort_by(.vendor, .name, .version))
   },
   components: (($frag.components + $added) | sort_by(.["bom-ref"])),
   dependencies: ($graph | to_entries
     | map({ref: .key, dependsOn: (.value | unique)})
     | sort_by(.ref))}
