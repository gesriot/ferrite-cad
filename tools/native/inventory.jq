# SPDX-License-Identifier: MIT
#
# Builds the native/assets inventory document.
#
# Every value here arrives from somewhere that already owned it: the two pin
# files, the measured ownership maps, the measured loaded-by map, and the
# product graph cargo's own resolver reports. This program decides the shape of
# the document and nothing about its content.

def lines: split("\n") | map(select(length > 0));
def rows: lines | map(select(startswith("#") | not)) | map(split("\t"));
def die($msg): ($msg | halt_error(1));

($targets | rows | map({platform: .[0], triple: .[1], bin: .[2], lib: .[3]}))
                                                          as $targetrows
| ([$targetrows[] | .triple] | sort)                       as $triples
| (reduce $targetrows[] as $t ({}; .[$t.platform] = $t.triple))
                                                          as $triple_by_platform
| ($staged | rows | map({platform: .[0], owner: .[1], path: .[2]}))
                                                          as $stagedrows
| ($loadedby | rows | map({owner: .[0], binary: .[1]}))    as $loadedrows
| ($roots  | rows
   | map({bin: .[0], package: .[1], version: .[2], ref: .[3]}))
                                                          as $rootrows
| ($fragments | rows | map({target: .[0], path: .[1], sha256: .[2]}))
                                                          as $fragmentrows
| ($assets | rows
   | map({kind: .[0], package: .[1], packageDir: .[2], path: .[3], name: .[4],
          sha256: .[5], bytes: (.[6] | tonumber),
          embeddedIn: (.[7] | split(",") | sort)}))        as $assetrows

# A staged path claimed twice is the failure this whole slice exists to make
# impossible, so it cannot be allowed to become a document first.
| ([$stagedrows | group_by(.platform + " " + .path)[]
    | select(length > 1)
    | {platform: .[0].platform, path: .[0].path,
       owners: (map(.owner) | unique)}])                   as $doubled
| (if ($doubled | length) > 0
   then die("a staged file is claimed by more than one owner: "
            + ($doubled | map(.platform + " " + .path + " -> "
                              + (.owners | join(", "))) | join("; ")))
   else . end)

| ([$rootrows[] | .bin] | sort)                            as $root_names
| ([$stagedrows[] | .owner] | unique)                      as $owners
| ([$owners[] | select(. as $o | ($root_names | index($o)) == null)]) as $component_owners
| ([$component_owners[] | select(. != "occt" and . != "planegcs")]) as $unknown_owners
| (if ($unknown_owners | length) > 0
   then die("a staged file is owned by something this inventory has no component for: "
            + ($unknown_owners | join(", ")))
   else . end)

| ([$rootrows[] | .version] | unique)                      as $product_versions
| (if ($product_versions | length) != 1
   then die("the two product roots do not share a version: "
            + ($product_versions | join(", ")))
   else . end)

# --- what each owner stages, per target ------------------------------------

| def staged_for($owner):
    (reduce $targetrows[] as $t ({};
       .[$t.triple] = ([$stagedrows[]
                        | select(.platform == $t.platform and .owner == $owner)
                        | .path | split("/") | last] | sort)));

  def targets_staging($owner):
    ([$stagedrows[] | select(.owner == $owner) | $triple_by_platform[.platform]]
     | unique | sort);

  def loaded_by($owner):
    ([$loadedrows[] | select(.owner == $owner) | .binary] | unique | sort);

# --- components ------------------------------------------------------------

  {
    id: ("native+occt@" + $occt_version),
    name: "Open CASCADE Technology",
    version: $occt_version,
    role: "runtime-native",
    stagedRuntimeFile: true,
    targets: targets_staging("occt"),
    source: {
      kind: "source-archive",
      tag: $occt_tag,
      commit: $occt_commit,
      url: $occt_url,
      sha256: $occt_sha256,
      pin: "tools/occt/pin.env"
    },
    stagedFilenames: staged_for("occt"),
    loadedBy: loaded_by("occt"),
    # Decided by measurement, not by what was easier to generate. Every staged
    # toolkit comes from one commit, reports one OCC_VERSION_COMPLETE and is
    # built by one CMake configure; not one of them carries a version, a digest
    # or an upstream identity of its own, and which of them a platform stages
    # is a property of that platform's linker rather than of the toolkit. So
    # Open CASCADE is one component with a per-target file inventory, and the
    # gate is on that inventory: every staged toolkit is named here, and every
    # name here is a file the staging really produced.
    fileModel: "one-component-with-file-inventory"
  } as $occt

| {
    id: ("native+planegcs@" + $planegcs_tag),
    name: "planegcs",
    version: $planegcs_tag,
    role: "runtime-native",
    stagedRuntimeFile: true,
    targets: targets_staging("planegcs"),
    source: {
      kind: "source-archive",
      tag: $planegcs_tag,
      url: $planegcs_url,
      sha256: $planegcs_sha256,
      pin: "tools/planegcs/pin.env",
      note: "the planegcs sources are taken from the FreeCAD release archive named here"
    },
    stagedFilenames: staged_for("planegcs"),
    loadedBy: loaded_by("planegcs"),
    fileModel: "one-component-with-file-inventory"
  } as $planegcs

# The Windows import library. It is not a runtime file: it carries no planegcs
# implementation and the staged layout does not hold it. Written down because a
# build artifact nobody names is a build artifact somebody ships.
#
# Which way it points was measured rather than assumed, and the first spelling
# of this had it backwards. `planegcs.lib` is produced by the planegcs build -
# tools/build-planegcs.sh emits it beside planegcs.dll on Windows and nowhere
# else - and it is consumed by the Windows linker on behalf of the one crate
# that links planegcs, whose build.rs refuses to link without it. So planegcs
# produces it and the crate consumes it; calling it a build input of planegcs
# would say planegcs is built from it, which is the opposite of what happens.
| {
    id: ("native+planegcs-import-library@" + $planegcs_tag),
    name: "planegcs import library",
    version: $planegcs_tag,
    role: "build-input",
    stagedRuntimeFile: false,
    notAStagedRuntimeFile:
      "linker metadata only. It lets somebody relink against a planegcs they replaced; it holds no planegcs code and the staged Windows layout does not carry it.",
    targets: [$triple_by_platform["windows"]],
    artifactFilename: "planegcs.lib",
    producedBy: ("native+planegcs@" + $planegcs_tag),
    buildInputOf: [$importlib_consumer],
    source: {
      kind: "built-from",
      tag: $planegcs_tag,
      url: $planegcs_url,
      sha256: $planegcs_sha256,
      pin: "tools/planegcs/pin.env"
    }
  } as $import_library

| {
    id: ("native+eigen@" + $eigen_version),
    name: "Eigen",
    version: $eigen_version,
    role: "build-input",
    stagedRuntimeFile: false,
    notAStagedRuntimeFile:
      "a header tree compiled into planegcs and into the sketch solver shim. No file of it is staged, on any target.",
    targets: $triples,
    buildInputOf: [("native+planegcs@" + $planegcs_tag)],
    source: {
      kind: "source-archive",
      version: $eigen_version,
      url: $eigen_url,
      sha256: $eigen_sha256,
      pin: "tools/planegcs/pin.env"
    }
  } as $eigen

| {
    id: ("native+boost@" + $boost_version),
    name: "Boost",
    version: $boost_version,
    role: "build-input",
    stagedRuntimeFile: false,
    notAStagedRuntimeFile:
      "a header tree compiled into planegcs and into the sketch solver shim. No file of it is staged, on any target.",
    targets: $triples,
    buildInputOf: [("native+planegcs@" + $planegcs_tag)],
    source: {
      kind: "source-archive",
      version: $boost_version,
      url: $boost_url,
      sha256: $boost_sha256,
      pin: "tools/planegcs/pin.env"
    }
  } as $boost

| ([$assetrows[]
    | (.package | split("@")) as $pkg
    | {
        id: (if .kind == "repository"
             then "asset+path+" + .path
             else "asset+crate+" + .package + "#" + .path end),
        name: .name,
        version: $pkg[1],
        role: "embedded-asset",
        stagedRuntimeFile: false,
        notAStagedRuntimeFile:
          "compiled into a product executable. There is no separate file to stage and none is staged.",
        targets: $triples,
        embeddedIn: .embeddedIn,
        asset: ({
          location: .kind,
          path: .path,
          sha256: .sha256,
          bytes: .bytes
        } + (if .kind == "crate" then {crate: .package} else {} end)),
        containedBy: (if .kind == "repository"
                      then "path+" + .packageDir + "#" + .package
                      else "registry+https://github.com/rust-lang/crates.io-index#" + .package
                      end)
      }]) as $assets_out

| {
    kind: "ferritecad-native-assets-inventory",
    formatVersion: ($format | tonumber),
    complete: false,
    isProductSbom: false,
    statement: "This is a native and assets inventory, not a complete product SBOM. It describes the native runtime components a FerriteCAD release carries, the native inputs that only take part in a build, and the assets a product binary embeds. The Rust components are described by the CycloneDX fragments named in rustFragments, and nothing here merges the two.",
    pendingMerge: "rust-fragment-and-native-assets",
    productVersion: $product_versions[0],

    rustFragments: ([$fragmentrows[]
      | {target: .target, path: .path, sha256: .sha256,
         ref: ("ferritecad:rust-fragment:" + .target)}]
      | sort_by(.target)),

    productRoots: ([$rootrows[]
      | .bin as $bin
      | {binary: $bin,
         package: .package,
         version: .version,
         rustFragmentRef: .ref,
         stagedFilenames: staged_for($bin),
         loads: ([$loadedrows[] | select(.binary == $bin) | .owner] | unique | sort)}]
      | sort_by(.binary)),

    components: (([$occt, $planegcs, $import_library, $eigen, $boost] + $assets_out)
      | sort_by(.id)),

    targets: ([$targetrows[]
      | .platform as $platform
      | .triple as $triple
      | {triple: $triple,
         platform: $platform,
         layout: {executables: .bin, libraries: .lib},
         stagedFiles: ([$stagedrows[]
           | select(.platform == $platform)
           | .owner as $owner
           | ($root_names | index($owner)) as $is_root
           | {path: .path,
              owner: (if $is_root != null then $owner
                      elif $owner == "occt" then $occt.id
                      else $planegcs.id end),
              ownerKind: (if $is_root != null then "product-root" else "component" end)}]
           | sort_by(.path)),
         systemLibraries: {
           shipped: false,
           rule: "A library the operating system ships is not part of the delivery and is never copied. tools/runtime-closure.sh decides this by where a name resolves - /lib, /lib64, /usr/lib, /usr/lib64, the program interpreter and the virtual DSO on Linux; /usr/lib and /System/Library on macOS, where the dyld shared cache means there is no file at all; System32, SysWOW64, WinSxS and the API set names the loader redirects on Windows. Open CASCADE and planegcs are matched by name before location, so a toolkit installed into a system directory is still this product's to carry.",
           enumerated: false,
           enumerationNote: "The set differs between runner images and is therefore evidence of one run rather than a property of the product. Each run of the combined runtime layout workflow uploads what it observed; this document commits the rule instead."
         }}]
      | sort_by(.triple))
  }
