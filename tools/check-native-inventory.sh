#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The gate on the native/assets inventory.
#
# The inventory answers six questions the Rust CycloneDX fragment cannot: which
# native components a target carries, which staged runtime file belongs to
# which of them, which inputs only ever take part in a build, which assets a
# product binary embeds, which product root loads which native component, and
# which libraries the loader shows that the delivery deliberately does not
# carry. Without an answer that a machine can check, a later merge can look
# perfectly correct while describing a different set of files.
#
# The gate runs in two halves, because half of what it asks needs a real staged
# layout and half of it does not.
#
#   Without --staging it asks what can be asked from the repository alone: the
#   document is schema-valid, it says in its own fields that it is not a
#   product SBOM, it carries nothing about the machine that produced it, two
#   runs of the generator reproduce the committed bytes, every version and
#   digest agrees with the one file that owns it, every embedded asset exists
#   and hashes to what is written down, the product graph embeds no asset the
#   inventory has missed, and the Rust fragments are byte for byte the ones the
#   inventory was written against.
#
#   With --staging it asks the questions only a measurement can answer: every
#   staged non-system file has exactly one owner, no staged file is unowned, no
#   name is promised that the staging does not produce, no build input appears
#   as a runtime file, each library's owner agrees with the class the loader
#   walk gave it independently, and - with --binary - each declared asset's
#   bytes really are inside the binary that claims them and are not inside the
#   one that does not.
#
# Runs no network.
#
# Run from the repository root:
#   tools/check-native-inventory.sh
#   tools/check-native-inventory.sh --platform macos --staging DIR \
#       --closure closure-viewer.txt --closure closure-cli.txt \
#       --binary viewer:path/to/ferritecad-viewer --binary cli:path/to/ferritecad

set -euo pipefail

NATIVE_TOOL='check-native-inventory'
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/native/lib.sh
. tools/native/lib.sh

platform=''
staging=''
closures=()
binaries=()

while [ $# -gt 0 ]; do
    case "$1" in
        --platform) platform="${2:?--platform needs a name}"; shift 2 ;;
        --staging)  staging="${2:?--staging needs a directory}"; shift 2 ;;
        --closure)  closures+=("${2:?--closure needs a file}"); shift 2 ;;
        --binary)   binaries+=("${2:?--binary needs label:path}"); shift 2 ;;
        *) native_die "unknown argument: $1" ;;
    esac
done

if [ -n "$staging" ]; then
    [ -n "$platform" ] || native_die '--staging needs --platform'
    [ -d "$staging" ] || native_die "no such staging directory: $staging"
    case " ${NATIVE_PLATFORMS[*]} " in
        *" $platform "*) ;;
        *) native_die "unknown platform $platform" ;;
    esac
fi

native_load_pins
native_require_jq

work="$(mktemp -d)"
# Explicitly, for the reason tools/generate-native-inventory.sh records: on
# bash 3.2 the last command of an EXIT trap can decide the exit status, and a
# gate that failed while reporting success is worse than no gate.
# shellcheck disable=SC2154  # assigned by the trap itself, one word earlier
trap 'status=$?; rm -rf "$work"; exit "$status"' EXIT

failures=0
checks=0
check() { checks=$((checks + 1)); }
fail() {
    failures=$((failures + 1))
    echo "$NATIVE_TOOL: $*" >&2
}

# ---------------------------------------------------------------------------
# What exists to be explained.
# ---------------------------------------------------------------------------
#
# Counted before the inventory is looked for, so that a run with no inventory
# says what it could not account for rather than only that a file is missing.

staged_count=0
if [ -n "$staging" ]; then
    find "$staging" -type f | LC_ALL=C sort > "$work/staged-actual.txt"
    staged_count="$(wc -l < "$work/staged-actual.txt" | tr -d ' ')"
    [ "$staged_count" -gt 0 ] || native_die \
        "the staging directory $staging holds no file at all, so there is nothing to explain"
fi

if [ ! -f "$NATIVE_INVENTORY" ]; then
    echo "$NATIVE_TOOL: no authoritative native/assets inventory exists." >&2
    if [ -n "$staging" ]; then
        echo >&2
        echo "  The staged $platform layout at $staging holds $staged_count files." >&2
        echo "  Not one of them has a component that owns it, no document says which" >&2
        echo "  of them are runtime files and which are not, and nothing names the" >&2
        echo "  assets the product binaries embed. A merge written against this" >&2
        echo "  state could describe any set of files and look correct." >&2
        # Parameter expansion rather than sed: RUNNER_TEMP on Windows is a
        # path with backslashes in it, and a backslash in a sed pattern is an
        # escape rather than a separator.
        while IFS= read -r found; do
            printf '    %s\n' "${found#"$staging"/}" >&2
        done < <(head -8 "$work/staged-actual.txt")
        [ "$staged_count" -le 8 ] || echo "    ... and $((staged_count - 8)) more" >&2
    fi
    echo >&2
    echo "  Expected $NATIVE_INVENTORY; write it with tools/generate-native-inventory.sh" >&2
    exit 1
fi

check
if ! jq -e . "$NATIVE_INVENTORY" >/dev/null 2>&1; then
    native_die "$NATIVE_INVENTORY is not readable JSON"
fi

inventory="$NATIVE_INVENTORY"

# ---------------------------------------------------------------------------
# The document, on its own terms.
# ---------------------------------------------------------------------------

if [ -z "$staging" ]; then
    # The schema is the definition of the shape, and it is a real validator
    # rather than a parser written here. Pinned like the CycloneDX one.
    check
    if ! command -v jsonschema-cli >/dev/null 2>&1; then
        fail "jsonschema-cli is not installed; the shape of $inventory cannot be checked"
    else
        . tools/sbom/pin.env
        found="$(jsonschema-cli --version 2>/dev/null | awk '{print $NF}')"
        if [ "$found" != "$JSONSCHEMA_CLI_VERSION" ]; then
            fail "jsonschema-cli $found is installed but $JSONSCHEMA_CLI_VERSION is pinned"
        elif ! jsonschema-cli validate "$NATIVE_SCHEMA" -i "$inventory" \
                > "$work/schema.log" 2>&1; then
            fail "$inventory does not match $NATIVE_SCHEMA:"
            sed 's/^/  /' "$work/schema.log" >&2
        fi
    fi

    # It has to keep saying what it is. The schema pins these too; asked again
    # here so the failure reads as a claim rather than as a const mismatch.
    check
    said="$(jq -r '[.kind, (.complete|tostring), (.isProductSbom|tostring), .pendingMerge]
                   | join(" ")' "$inventory" | native_strip_cr)"
    if [ "$said" != "ferritecad-native-assets-inventory false false rust-fragment-and-native-assets" ]; then
        fail "$inventory no longer declares itself an incomplete native/assets inventory: $said"
    fi

    # Nothing about the machine that produced it, and no path that leaves the
    # repository. Both would make the document a statement about a checkout.
    check
    if grep -nEi 'file://|/Users/|/home/|/root/|(^|[^A-Za-z0-9])[A-Za-z]:[\\/]|/private/var/|/tmp/|runner/work|AppData|RUNNER_TEMP|\.cargo/registry' \
            "$inventory" > "$work/host.txt"; then
        fail "$inventory carries host-specific data:"
        head -n 10 "$work/host.txt" | sed 's/^/  /' >&2 || true
    fi

    check
    if grep -nEi '"[0-9]{4}-[0-9]{2}-[0-9]{2}T|urn:uuid|serialNumber|timestamp' \
            "$inventory" > "$work/when.txt"; then
        fail "$inventory carries a timestamp or a generated identifier:"
        head -n 10 "$work/when.txt" | sed 's/^/  /' >&2 || true
    fi

    # Not swallowed. The first spelling of this had an unbalanced bracket in
    # the pattern, jq refused to compile it, the error went to /dev/null and
    # the check passed by doing nothing.
    check
    if ! jq -r '.. | strings | select(test("(^|/)[.][.](/|$)"))' "$inventory" \
            > "$work/traversal.txt" 2>"$work/traversal.err"; then
        fail 'the parent-traversal check could not run:'
        sed 's/^/  /' "$work/traversal.err" >&2
    elif [ -s "$work/traversal.txt" ]; then
        fail "$inventory holds a path with a parent traversal in it:"
        sed 's/^/  /' "$work/traversal.txt" >&2
    fi

    check
    if grep -q $'\r' "$inventory"; then
        fail "$inventory carries carriage returns; .gitattributes should keep it out of conversion"
    fi

    # Two runs, and the committed bytes.
    check
    if ! tools/generate-native-inventory.sh --output "$work/first.json" >/dev/null; then
        fail 'the generator failed'
    elif ! tools/generate-native-inventory.sh --output "$work/second.json" >/dev/null; then
        fail 'the generator failed on its second run'
    else
        check
        cmp -s "$work/first.json" "$work/second.json" \
            || fail 'two consecutive runs of the generator disagree'
        check
        if ! cmp -s "$inventory" "$work/first.json"; then
            fail "$inventory is stale; regenerate with tools/generate-native-inventory.sh"
            diff -u "$inventory" "$work/first.json" | head -n 40 >&2 || true
        fi
    fi

    # ---------------------------------------------------------------------
    # Every version and digest against the file that owns it. Read here from
    # the pin rather than from anything the generator wrote, so a generator
    # that invented a value has two answers to disagree with.
    # ---------------------------------------------------------------------

    pin_says() { # component-id-prefix jq-path expected
        check
        local got
        got="$(jq -r --arg p "$1" \
            '(.components // [])[] | select(.id | startswith($p)) | '"$2" "$inventory" \
            | native_strip_cr)"
        [ "$got" = "$3" ] || fail "$1: the inventory says $2 is '$got', the pin says '$3'"
    }
    pin_says 'native+occt@' '.version' "$OCCT_VERSION"
    pin_says 'native+occt@' '.source.commit' "$OCCT_COMMIT"
    pin_says 'native+occt@' '.source.sha256' "$OCCT_SHA256"
    pin_says 'native+occt@' '.source.url' "$OCCT_ARCHIVE_URL"
    pin_says 'native+planegcs@' '.version' "$FCAD_PLANEGCS_FREECAD_TAG"
    pin_says 'native+planegcs@' '.source.sha256' "$FCAD_PLANEGCS_ARCHIVE_SHA256"
    pin_says 'native+planegcs@' '.source.url' "$FCAD_PLANEGCS_FREECAD_URL"
    pin_says 'native+eigen@' '.version' "$FCAD_PLANEGCS_EIGEN_VERSION"
    pin_says 'native+eigen@' '.source.sha256' "$FCAD_PLANEGCS_EIGEN_SHA256"
    pin_says 'native+boost@' '.version' "$FCAD_PLANEGCS_BOOST_VERSION"
    pin_says 'native+boost@' '.source.sha256' "$FCAD_PLANEGCS_BOOST_SHA256"

    # A digest that belongs to another component is the failure this catches:
    # every digest in the document has to be one of the four the pins hold, and
    # each of the four has to be used once.
    check
    jq -r '[(.components // [])[] | .source.sha256 // empty] | .[]' "$inventory" \
        | native_strip_cr | LC_ALL=C sort -u > "$work/digests-used.txt"
    printf '%s\n' "$OCCT_SHA256" "$FCAD_PLANEGCS_ARCHIVE_SHA256" \
        "$FCAD_PLANEGCS_EIGEN_SHA256" "$FCAD_PLANEGCS_BOOST_SHA256" \
        | LC_ALL=C sort -u > "$work/digests-pinned.txt"
    if ! cmp -s "$work/digests-used.txt" "$work/digests-pinned.txt"; then
        fail 'the source digests in the inventory are not exactly the ones the pins hold:'
        diff -u "$work/digests-pinned.txt" "$work/digests-used.txt" | sed 's/^/  /' >&2 || true
    fi

    # ---------------------------------------------------------------------
    # And that the workflow still measures every platform.
    # ---------------------------------------------------------------------
    #
    # A platform dropped from the matrix takes its evidence with it, and a
    # comparison of the two that are left agrees perfectly. The workflow names
    # the platforms twice - once in the matrix that measures and once in the
    # loop that requires the evidence - and both lists have to be this one.

    check
    matrix="$(awk '/^[[:space:]]+matrix:/ { inmatrix = 1 }
                   inmatrix && /^[[:space:]]+name:[[:space:]]+[a-z]+$/ { print $2 }
                   inmatrix && /^[[:space:]]*defaults:/ { inmatrix = 0 }' \
              "$NATIVE_WORKFLOW" | LC_ALL=C sort -u | paste -sd, -)"
    expected_platforms="$(printf '%s\n' "${NATIVE_PLATFORMS[@]}" | LC_ALL=C sort | paste -sd, -)"
    [ "$matrix" = "$expected_platforms" ] \
        || fail "$NATIVE_WORKFLOW measures '$matrix' and the product platforms are '$expected_platforms'"

    check
    required="$(sed -n 's/^ *for platform in \(.*\); do$/\1/p' "$NATIVE_WORKFLOW" \
                | tr ' ' '\n' | grep -v '^$' | LC_ALL=C sort -u | paste -sd, -)"
    [ "$required" = "$expected_platforms" ] \
        || fail "$NATIVE_WORKFLOW requires evidence from '$required' and the product platforms are '$expected_platforms'"

    # ---------------------------------------------------------------------
    # And that the Open CASCADE pin still has one owner.
    # ---------------------------------------------------------------------
    #
    # tools/check-planegcs-pins.sh has enforced this for planegcs since
    # §21A-2b2b0a. Open CASCADE only acquired a file of its own when this
    # inventory needed to name it as a component, and a rule that is not
    # checked is a rule that comes back: the pin lived in a workflow's `env:`
    # block until now, and putting it back there would leave every gate green.

    check
    if grep -rnE '^[[:space:]]*OCCT_(COMMIT|SHA256|VERSION|TAG|ARCHIVE_URL):' \
            .github/workflows > "$work/occt-workflow-pin.txt"; then
        fail "a workflow declares an Open CASCADE pin of its own; $NATIVE_OCCT_PIN owns it:"
        sed 's/^/  /' "$work/occt-workflow-pin.txt" >&2
    fi

    check
    if grep -rn --exclude=pin.env -F "$OCCT_SHA256" tools .github/workflows \
            > "$work/occt-copy.txt"; then
        fail "the Open CASCADE archive digest is pinned in $NATIVE_OCCT_PIN and copied somewhere that runs:"
        sed 's/^/  /' "$work/occt-copy.txt" >&2
    fi

    # The document quotes the pin as a record. A digest in it that this
    # repository does not pin is a digest somebody typed.
    check
    for value in "$OCCT_COMMIT" "$OCCT_SHA256"; do
        grep -qF "$value" docs/build-occt.md \
            || fail "docs/build-occt.md does not quote ${value}, which $NATIVE_OCCT_PIN pins"
    done

    # ---------------------------------------------------------------------
    # The Rust fragments the inventory was written against.
    # ---------------------------------------------------------------------

    check
    jq -r '(.rustFragments // [])[] | .target + "\t" + .path + "\t" + .sha256' "$inventory" \
        | native_strip_cr | LC_ALL=C sort > "$work/fragments-claimed.tsv"
    : > "$work/fragments-actual.tsv"
    for target in "${NOTICE_TARGETS[@]}"; do
        fragment="sbom/rust/rust-fragment-${target}.cdx.json"
        if [ ! -f "$fragment" ]; then
            fail "the Rust fragment $fragment is missing"
            continue
        fi
        printf '%s\t%s\t%s\n' "$target" "$fragment" "$(native_sha256 "$fragment")" \
            >> "$work/fragments-actual.tsv"
    done
    LC_ALL=C sort -o "$work/fragments-actual.tsv" "$work/fragments-actual.tsv"
    if ! cmp -s "$work/fragments-claimed.tsv" "$work/fragments-actual.tsv"; then
        fail 'the Rust CycloneDX fragments are not the ones this inventory was written against:'
        diff -u "$work/fragments-claimed.tsv" "$work/fragments-actual.tsv" \
            | sed 's/^/  /' >&2 || true
        echo "  A fragment may only change through tools/generate-rust-sbom.sh." >&2
    fi

    # ---------------------------------------------------------------------
    # The embedded assets: they exist, they hash to what is written down, and
    # the product graph embeds none the inventory has missed.
    # ---------------------------------------------------------------------

    cargo metadata --locked --format-version 1 2>/dev/null > "$work/metadata.json" \
        || native_die 'cargo metadata --locked failed'
    jq -r '(.workspace_root | gsub("\\\\"; "/")) as $root
           | .packages[]
           | [(.name + "@" + .version),
              (.manifest_path | gsub("\\\\"; "/") | rtrimstr("/Cargo.toml")),
              (if (.manifest_path | gsub("\\\\"; "/") | startswith($root + "/"))
               then (.manifest_path | gsub("\\\\"; "/") | ltrimstr($root + "/")
                     | rtrimstr("/Cargo.toml"))
               else "" end)]
           | @tsv' "$work/metadata.json" | native_strip_cr \
        | LC_ALL=C sort -u > "$work/packages.tsv"

    # A dash rather than an empty column: tab is an IFS whitespace character,
    # so bash collapses a run of them and an empty field in the middle of a row
    # silently shifts every field after it.
    jq -r '(.components // [])[] | select(.role == "embedded-asset")
           | [.id, .asset.location, (.asset.crate // "-"), .asset.path,
              .asset.sha256, (.asset.bytes|tostring)] | @tsv' "$inventory" \
        | native_strip_cr | LC_ALL=C sort > "$work/assets-claimed.tsv"

    check
    [ -s "$work/assets-claimed.tsv" ] \
        || fail 'the inventory declares no embedded asset at all'

    while IFS=$'\t' read -r id location crate path sha bytes; do
        check
        case "$location" in
            repository) file="$path" ;;
            crate)
                dir="$(awk -F'\t' -v k="$crate" '$1 == k { print $2 }' "$work/packages.tsv")"
                if [ -z "$dir" ]; then
                    fail "$id names the crate $crate, which is not in this dependency graph"
                    continue
                fi
                file="$dir/$path" ;;
            *) fail "$id has an asset location this gate does not know: $location"; continue ;;
        esac
        if [ ! -f "$file" ]; then
            fail "$id names an asset that is not there"
            continue
        fi
        actual="$(native_sha256 "$file")"
        [ "$actual" = "$sha" ] \
            || fail "$id hashes to $actual and the inventory pins $sha"
        actual_bytes="$(wc -c < "$file" | tr -d ' ')"
        [ "$actual_bytes" = "$bytes" ] \
            || fail "$id is $actual_bytes bytes and the inventory says $bytes"
    done < "$work/assets-claimed.tsv"

    # The other direction, and it does not read the inventory to find its
    # answer. Every package the product graph reaches is walked for the two
    # include macros; this repository's own embedded files and every font,
    # wherever it came from, have to be declared.
    : > "$work/reach.txt"
    for target in "${NOTICE_TARGETS[@]}"; do
        for root in "${NOTICE_ROOTS[@]}"; do
            manifest="${root#*|}"; manifest="${manifest%%|*}"
            features="${root##*|}"
            tree=(cargo tree --locked --target "$target" -e normal
                  --prefix depth --format '{p}' --manifest-path "$manifest")
            [ -z "$features" ] || tree+=(--features "$features")
            "${tree[@]}" 2>/dev/null | native_strip_cr \
                | awk '{ if (match($0, /^[0-9]+/) == 0) next
                         rest = substr($0, RLENGTH + 1)
                         if (match(rest, /^[^ ]+ v[^ ]+/) == 0) next
                         head = substr(rest, 1, RLENGTH)
                         i = index(head, " v")
                         print substr(head, 1, i - 1) "@" substr(head, i + 2) }' \
                >> "$work/reach.txt"
        done
    done
    LC_ALL=C sort -u "$work/reach.txt" -o "$work/reach.txt"

    : > "$work/assets-found.txt"
    : > "$work/unreadable.txt"
    while IFS=$'\t' read -r key dir repo_rel; do
        grep -Fxq "$key" "$work/reach.txt" || continue
        if [ -n "$repo_rel" ]; then
            native_unreadable_includes "$dir" >> "$work/unreadable.txt"
        fi
        while IFS= read -r resolved; do
            [ -n "$resolved" ] || continue
            base="$(cd "$(dirname "$resolved")" && pwd -P)/$(basename "$resolved")"
            root_abs="$(cd "$dir" && pwd -P)"
            rel="${base#"$root_abs"/}"
            is_font=''
            ! native_is_font_file "$resolved" || is_font=1
            if [ -n "$repo_rel" ]; then
                printf 'asset+path+%s\n' "$repo_rel/$rel" >> "$work/assets-found.txt"
            elif [ -n "$is_font" ]; then
                printf 'asset+crate+%s#%s\n' "$key" "$rel" >> "$work/assets-found.txt"
            fi
        done < <(native_scan_includes "$dir")
    done < "$work/packages.tsv"
    LC_ALL=C sort -u "$work/assets-found.txt" -o "$work/assets-found.txt"

    check
    if [ -s "$work/unreadable.txt" ]; then
        fail 'a workspace package in the product graph embeds a file through a form this gate cannot read:'
        sed 's/^/  /' "$work/unreadable.txt" >&2
        echo "  An include nobody can read must not pass for an absence." >&2
    fi

    check
    cut -f1 "$work/assets-claimed.tsv" | LC_ALL=C sort -u > "$work/assets-ids.txt"
    if ! cmp -s "$work/assets-found.txt" "$work/assets-ids.txt"; then
        fail 'the assets the product graph embeds are not the assets the inventory declares:'
        diff -u "$work/assets-found.txt" "$work/assets-ids.txt" | sed 's/^/  /' >&2 || true
    fi

    # ---------------------------------------------------------------------
    # The ownership map, before any staging is looked at.
    # ---------------------------------------------------------------------

    check
    component_ids="$(jq -r '(.components // [])[].id' "$inventory" | native_strip_cr | LC_ALL=C sort -u)"
    root_names="$(jq -r '(.productRoots // [])[].binary' "$inventory" | native_strip_cr | LC_ALL=C sort -u)"
    jq -r '(.targets // [])[] | .triple as $t | (.stagedFiles // [])[]
           | [$t, .path, .owner, .ownerKind] | @tsv' "$inventory" \
        | native_strip_cr | LC_ALL=C sort > "$work/owned.tsv"
    while IFS=$'\t' read -r triple path owner kind; do
        case "$kind" in
            component) printf '%s\n' "$component_ids" | grep -Fxq "$owner" \
                || fail "$triple: $path is owned by $owner, which is not a component" ;;
            product-root) printf '%s\n' "$root_names" | grep -Fxq "$owner" \
                || fail "$triple: $path is owned by $owner, which is not a product root" ;;
        esac
    done < "$work/owned.tsv"

    check
    if awk -F'\t' '{ print $1 "\t" $2 }' "$work/owned.tsv" | LC_ALL=C sort \
            | uniq -d | grep -q .; then
        fail 'a staged path is claimed by more than one owner:'
        awk -F'\t' '{ print $1 "\t" $2 }' "$work/owned.tsv" | LC_ALL=C sort | uniq -d \
            | sed 's/^/  /' >&2
    fi

    # A build input that promised a staged runtime file would be asking the
    # packager to ship a header tree, and the name of one appearing in a staged
    # list is the same mistake wearing a filename.
    check
    jq -r '(.components // [])[] | select(.role != "runtime-native")
           | .artifactFilename // empty' "$inventory" \
        | native_strip_cr | LC_ALL=C sort -u > "$work/not-runtime.txt"
    while IFS= read -r name; do
        [ -n "$name" ] || continue
        if awk -F'\t' -v n="$name" '{ m = $2; sub(/.*\//, "", m); if (m == n) print }' \
                "$work/owned.tsv" | grep -q .; then
            fail "$name belongs to something that is not a runtime component and is staged"
        fi
    done < "$work/not-runtime.txt"

    check
    if jq -e '[(.components // [])[] | select(.role != "runtime-native")
              | select((.stagedFilenames // {}) | length > 0)] | length > 0' \
            "$inventory" >/dev/null; then
        fail 'a component that is not a runtime component declares staged filenames'
    fi

    # Each platform's staged paths have to look like that platform's layout,
    # so a file from another target cannot arrive as a longer list.
    for p in "${NATIVE_PLATFORMS[@]}"; do
        check
        triple="$(native_triple_for "$p")"
        bin_dir="$(native_bin_dir_for "$p")"
        lib_dir="$(native_lib_dir_for "$p")"
        pattern="$(native_library_pattern_for "$p")"
        while IFS=$'\t' read -r t path owner kind; do
            [ "$t" = "$triple" ] || continue
            case "$path" in
                "$bin_dir"/* | "$lib_dir"/*) ;;
                *) fail "$triple: $path is under neither $bin_dir nor $lib_dir" ;;
            esac
            [ "$kind" = component ] || continue
            # shellcheck disable=SC2254
            case "${path##*/}" in
                $pattern) ;;
                *) fail "$triple: ${path##*/} is not a $p shared library, so it is a file from another target" ;;
            esac
        done < "$work/owned.tsv"
    done

    # Three targets that staged the same set would mean the target sections
    # decide nothing, and every check above would still pass.
    check
    distinct="$(jq -r '(.targets // [])[] | [(.stagedFiles // [])[].path] | sort | tostring' "$inventory" \
        | native_strip_cr | LC_ALL=C sort -u | wc -l | tr -d ' ')"
    [ "$distinct" -eq "${#NATIVE_PLATFORMS[@]}" ] \
        || fail "the ${#NATIVE_PLATFORMS[@]} target sections describe only $distinct distinct staged sets"

    check
    triples="$(jq -r '[(.targets // [])[].triple] | sort | join(",")' "$inventory" | native_strip_cr)"
    expected="$(printf '%s\n' "${NOTICE_TARGETS[@]}" | LC_ALL=C sort | paste -sd, -)"
    [ "$triples" = "$expected" ] \
        || fail "the inventory covers $triples and the product targets are $expected"
fi

# ---------------------------------------------------------------------------
# The staged half.
# ---------------------------------------------------------------------------

if [ -n "$staging" ]; then
    triple="$(native_triple_for "$platform")"
    echo "$NATIVE_TOOL: $platform ($triple), staged layout at $staging"

    jq -r --arg t "$triple" '(.targets // [])[] | select(.triple == $t) | (.stagedFiles // [])[]
           | [.path, .owner, .ownerKind] | @tsv' "$inventory" \
        | native_strip_cr | LC_ALL=C sort > "$work/expected.tsv"
    check
    [ -s "$work/expected.tsv" ] || fail "the inventory has no staged files for $triple"

    : > "$work/actual.txt"
    while IFS= read -r found; do
        printf '%s\n' "${found#"$staging"/}" >> "$work/actual.txt"
    done < "$work/staged-actual.txt"
    LC_ALL=C sort -o "$work/actual.txt" "$work/actual.txt"
    cut -f1 "$work/expected.tsv" | LC_ALL=C sort > "$work/promised.txt"

    check
    if comm -23 "$work/actual.txt" "$work/promised.txt" | grep -q .; then
        fail "$triple: staged files nobody owns:"
        comm -23 "$work/actual.txt" "$work/promised.txt" | sed 's/^/  /' >&2
    fi

    check
    if comm -13 "$work/actual.txt" "$work/promised.txt" | grep -q .; then
        fail "$triple: the inventory promises runtime files the staging did not produce:"
        comm -13 "$work/actual.txt" "$work/promised.txt" | sed 's/^/  /' >&2
    fi

    # The independent answer about who owns a library: the class
    # tools/runtime-closure.sh gave it while walking the loader graph, which
    # knows nothing about this inventory and predates it.
    if [ "${#closures[@]}" -gt 0 ]; then
        for closure in "${closures[@]}"; do
            [ -f "$closure" ] || native_die "no such closure report: $closure"
        done
        cat "${closures[@]}" | awk '$1 == "dep" { print $4 "\t" $5 }' \
            | LC_ALL=C sort -u > "$work/classified.tsv"
        check
        [ -s "$work/classified.tsv" ] \
            || fail 'the closure reports classify nothing, so they cannot be an independent answer'

        occt_id="$(jq -r '(.components // [])[] | select(.id | startswith("native+occt@")) | .id' \
            "$inventory" | native_strip_cr)"
        planegcs_id="$(jq -r '(.components // [])[] | select(.id | startswith("native+planegcs@")) | .id' \
            "$inventory" | native_strip_cr)"

        check
        while IFS=$'\t' read -r path owner kind; do
            [ "$kind" = component ] || continue
            base="${path##*/}"
            class="$(awk -F'\t' -v b="$base" '$2 == b { print $1 }' "$work/classified.tsv" \
                | LC_ALL=C sort -u)"
            if [ -z "$class" ]; then
                fail "$triple: $base is staged and the loader walk never named it"
                continue
            fi
            case "$class" in
                occt)     expect="$occt_id" ;;
                planegcs) expect="$planegcs_id" ;;
                system)
                    fail "$triple: $base is a system library and the delivery must not carry it"
                    continue ;;
                *) fail "$triple: the loader walk calls $base '$class', which nothing owns"
                   continue ;;
            esac
            [ "$owner" = "$expect" ] \
                || fail "$triple: $base is owned by $owner and the loader walk calls it $class, which is $expect"
        done < "$work/expected.tsv"

        # And nothing the loader walk called a system library is claimed by a
        # component or staged.
        check
        while IFS=$'\t' read -r class name; do
            [ "$class" = system ] || continue
            if awk -F'\t' -v n="$name" '{ m = $1; sub(/.*\//, "", m); if (m == n) print }' \
                    "$work/expected.tsv" | grep -q .; then
                fail "$triple: $name is a system library and the inventory carries it as delivered"
            fi
        done < "$work/classified.tsv"

        # loaded-by, in both directions, against the same reports.
        check
        for pair in "viewer:ferritecad-viewer" "cli:ferritecad"; do
            label="${pair%%:*}"; binary="${pair#*:}"
            for owner in occt planegcs; do
                id="$occt_id"; [ "$owner" = occt ] || id="$planegcs_id"
                declared="$(jq -r --arg i "$id" --arg b "$binary" \
                    '(.components // [])[] | select(.id == $i) | .loadedBy | index($b) | tostring' \
                    "$inventory" | native_strip_cr)"
                measured=no
                awk -v l="$label" -v c="$owner" '$1 == "dep" && $2 == l && $4 == c { found = 1 }
                     END { exit found ? 0 : 1 }' "${closures[@]}" && measured=yes
                if [ "$measured" = yes ] && [ "$declared" = null ]; then
                    fail "$triple: the closure shows $binary loading $owner and the inventory does not say so"
                fi
                if [ "$measured" = no ] && [ "$declared" != null ]; then
                    fail "$triple: the inventory says $binary loads $owner and the closure of $label does not"
                fi
            done
        done
    fi

    # A build input must not be in the staged layout under any name.
    check
    jq -r '(.components // [])[] | select(.role == "build-input")
           | (.artifactFilename // empty), (.name | ascii_downcase)' "$inventory" \
        | native_strip_cr | LC_ALL=C sort -u > "$work/build-inputs.txt"
    while IFS= read -r needle; do
        [ -n "$needle" ] || continue
        if awk -v n="$needle" 'BEGIN { IGNORECASE = 1 }
             { m = tolower($0); sub(/.*\//, "", m)
               if (m == tolower(n) || index(m, tolower(n) "-") == 1) print }' \
                "$work/actual.txt" | grep -q .; then
            fail "$triple: a build input is in the staged layout under the name $needle"
        fi
    done < "$work/build-inputs.txt"

    # And the assets, in the binaries that claim them.
    if [ "${#binaries[@]}" -gt 0 ]; then
        command -v python3 >/dev/null 2>&1 \
            || native_die 'python3 is needed to look for an asset inside a binary and is not installed'
        cargo metadata --locked --format-version 1 2>/dev/null > "$work/metadata.json" \
            || native_die 'cargo metadata --locked failed'
        jq -r '.packages[]
               | [(.name + "@" + .version),
                  (.manifest_path | gsub("\\\\"; "/") | rtrimstr("/Cargo.toml"))]
               | @tsv' "$work/metadata.json" | native_strip_cr \
            | LC_ALL=C sort -u > "$work/packages.tsv"

        : > "$work/probe.tsv"
        while IFS=$'\t' read -r id location crate path sha embedded; do
            case "$location" in
                repository) file="$path" ;;
                crate)
                    dir="$(awk -F'\t' -v k="$crate" '$1 == k { print $2 }' "$work/packages.tsv")"
                    file="$dir/$path" ;;
            esac
            printf '%s\t%s\t%s\n' "$id" "$(native_path "$file")" "$embedded" \
                >> "$work/probe.tsv"
        done < <(jq -r '(.components // [])[] | select(.role == "embedded-asset")
                        | [.id, .asset.location, (.asset.crate // "-"), .asset.path,
                           .asset.sha256, (.embeddedIn | join(","))] | @tsv' "$inventory" \
                 | native_strip_cr | LC_ALL=C sort)

        : > "$work/binaries.tsv"
        for entry in "${binaries[@]}"; do
            name="${entry%%:*}"; file="${entry#*:}"
            [ -f "$file" ] || native_die "no such binary: $file"
            printf '%s\t%s\n' "$name" "$(native_path "$file")" >> "$work/binaries.tsv"
        done

        # `|| probe_status=$?` rather than `|| fail`, and the status read
        # inside the `||`. An earlier spelling asked `$?` on the next line,
        # where it was the status of `fail` and therefore always zero: the run
        # that found this reported the probe as broken and then printed the
        # line that says every asset was where it should be.
        check
        probe_status=0
        python3 - "$work/probe.tsv" "$work/binaries.tsv" > "$work/probe.out" 2>&1 <<'PY' \
            || probe_status=$?
import sys

probe, binaries = sys.argv[1], sys.argv[2]
bins = {}
for line in open(binaries):
    name, path = line.rstrip("\n").split("\t")
    with open(path, "rb") as handle:
        bins[name] = handle.read()

bad = 0
for line in open(probe):
    ident, path, embedded = line.rstrip("\n").split("\t")
    claimed = set(x for x in embedded.split(",") if x)
    with open(path, "rb") as handle:
        data = handle.read()
    # A window from the middle of the file. The head of a font is a table
    # directory that several fonts share, and the tail is padding.
    needle = data[len(data) // 2:len(data) // 2 + 64]
    if len(needle) < 32:
        print(f"{ident}: too small to look for")
        bad += 1
        continue
    for name, blob in bins.items():
        present = blob.find(needle) >= 0
        want = name in claimed
        if present != want:
            state = "is in" if present else "is not in"
            says = "claims" if want else "does not claim"
            print(f"{ident}: {state} {name}, and the inventory {says} it")
            bad += 1
        else:
            print(f"{ident}: {name} {'yes' if present else 'no'} (as declared)")
sys.exit(1 if bad else 0)
PY
        if [ "$probe_status" -ne 0 ]; then
            fail 'the asset probe did not agree with the inventory:'
            sed 's/^/  /' "$work/probe.out" >&2
        else
            echo "$NATIVE_TOOL: every declared asset is inside the binary that claims it and outside the one that does not"
        fi
    fi
fi

if [ "$failures" -gt 0 ]; then
    echo >&2
    echo "$NATIVE_TOOL: $failures of $checks checks failed" >&2
    exit 1
fi

echo "$NATIVE_TOOL: $checks checks passed"
