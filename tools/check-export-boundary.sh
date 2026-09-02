#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Checks that the export model stays a model and does not become a viewer.
#
# `ferritecad-export` owns what a scene handed to an interchange writer is
# allowed to be. That is only useful while the type cannot reach the things a
# writer must not depend on: a document to reopen, a scene loader to call, a
# kernel implementation to ask for more geometry, and a GPU snapshot, camera or
# picking identity to read a picture out of. Every one of those would let two
# runs of the same export produce different bytes, and none of them would show
# up as a compile error, because adding a dependency always compiles.
#
# `ferritecad-scene` is the other half and deliberately the opposite: it is the
# one crate that knows a document, a rebuild, a stored import and a kernel at
# once, which is exactly why the neutral model does not live there.
#
# Checked mechanically over the resolved build graph rather than over the
# manifest, so a dependency that arrives through a third crate is caught too.
#
# Run from the repository root:
#   tools/check-export-boundary.sh

set -euo pipefail

readonly EXPORT='ferritecad-export'
readonly SCENE='crates/ferritecad-scene'

# Not in the export crate's normal build graph, at any depth.
readonly FORBIDDEN=(
    ferritecad-document
    ferritecad-scene
    ferritecad-occt
    ferritecad-viewport
    ferritecad-viewport-gpu
    ferritecad-ui
    ferritecad-app
    ferritecad-eval
    wgpu
    egui
    winit
)

# Types a writer must never be handed, whatever crate they come from.
readonly FORBIDDEN_NAMES=(
    RenderSnapshot
    SnapshotBuilder
    Camera
    PickId
    FacePickId
    EdgePickId
    VertexPickId
    ShapeHandle
    SubShapeHandle
    SessionId
    Document
)

problems=0
fail() {
    echo "error: $1" >&2
    problems=$((problems + 1))
}

# The part of a file that ships, which is everything above its test module. A
# gate may name whatever it is gating; what matters is the code that runs.
shipped() {
    awk '/^#\[cfg\(test\)\]/ { exit } { print }' "$1"
}

# Every line of the shipped half of a file that names something, with its line
# number, or nothing.
#
# Deliberately not `grep -q` at the end of a pipeline: this script runs under
# `pipefail`, and a `grep -q` that exits on its first match kills the `grep`
# feeding it with SIGPIPE. The pipeline then reports 141, an `if` around it is
# false, and the check silently passes whatever it was supposed to catch.
# String literals are removed before matching. A writer must not *use* a
# document, a kernel or a clock; that FBX happens to call one of its own nodes
# `Document` is the format's vocabulary and not a dependency.
names() {
    local file="$1"
    local name="$2"
    shipped "$file" \
        | grep -vE '^[[:space:]]*(//|\*)' \
        | sed 's/"[^"]*"//g' \
        | grep -nw "$name" || true
}

# How many times the shipped half of a crate's sources does something.
shipped_count() {
    local pattern="$1"
    shift
    local total=0
    local file
    for file in "$@"; do
        local found
        found="$(shipped "$file" | grep -vE '^[[:space:]]*(//|\*)' \
            | grep -cF "$pattern" || true)"
        total=$((total + found))
    done
    echo "$total"
}

command -v cargo >/dev/null || { echo "error: cargo is not on PATH" >&2; exit 1; }
command -v jq >/dev/null || { echo "error: jq is not on PATH" >&2; exit 1; }

# The resolved normal-dependency closure of the export crate. Dev-dependencies
# are excluded on purpose: a gate may use anything, and what ships is what this
# is about.
metadata="$(mktemp "${TMPDIR:-/tmp}/ferritecad-export-metadata.XXXXXX")"
trap 'rm -f "$metadata"' EXIT
if ! cargo metadata --locked --format-version 1 >"$metadata" 2>"$metadata.err"; then
    cat "$metadata.err" >&2
    rm -f "$metadata.err"
    echo "error: cargo could not resolve the workspace; a new dependency needs Cargo.lock" >&2
    exit 1
fi
rm -f "$metadata.err"

closure="$(jq -r --arg root "$EXPORT" '
    . as $meta
    | ($meta.packages | map({key: .id, value: .}) | from_entries) as $byid
    | ($meta.resolve.nodes | map({key: .id, value: .}) | from_entries) as $nodes
    | ($meta.packages[] | select(.name == $root) | .id) as $rootid
    | def walk($seen; $queue):
        if ($queue | length) == 0 then $seen
        else
          ($queue[0]) as $id
          | ($queue[1:]) as $rest
          | if ($seen | index($id)) then walk($seen; $rest)
            else
              ($nodes[$id].deps // []
                | map(select(any(.dep_kinds[]?; .kind == null)) | .pkg)) as $next
              | walk($seen + [$id]; $rest + $next)
            end
        end;
      walk([]; [$rootid])
    | map($byid[.].name)
    | unique
    | .[]
' "$metadata")"

if [ -z "$closure" ]; then
    echo "error: could not resolve the dependency closure of ${EXPORT}" >&2
    exit 1
fi

echo "${EXPORT} builds on:"
while IFS= read -r present; do
    echo "  ${present}"
    for crate in "${FORBIDDEN[@]}"; do
        if [ "$present" = "$crate" ]; then
            fail "${EXPORT} depends on ${crate}; the export model must not reach it"
        fi
    done
done <<<"$closure"

# `fail` ran in this shell, not a subshell, so the count above is the real one.


# And the model itself must not name a transient identity, whatever it depends
# on. The scene model only: `binary_stl` beside it takes a kernel mesh, whose
# face ranges are handles, and that is what an STL writer is handed.
readonly MODEL='crates/ferritecad-export/src/scene.rs'
for name in "${FORBIDDEN_NAMES[@]}"; do
    found="$(names "$MODEL" "$name")"
    if [ -n "$found" ]; then
        echo "$found" >&2
        fail "${EXPORT} names ${name} in the export model"
    fi
done

# The FBX writer is the first thing handed the model, and the whole point of
# the model is what the writer therefore cannot reach. It is given a scene and
# a byte sink, so its output is a function of the scene; a writer that could
# reopen the document, ask the kernel for more geometry, read the STEP the
# document was imported from, look at a picture, open a file, read a clock or
# draw a random number would be a writer whose output is a function of the
# machine it ran on. None of those would be a compile error, so they are
# checked here.
readonly WRITER_SOURCES=(
    crates/ferritecad-export/src/fbx/mod.rs
    crates/ferritecad-export/src/fbx/contract.rs
    crates/ferritecad-export/src/fbx/syntax.rs
)

# What a writer must not name, over and above the transient identities the
# model is already checked for.
readonly FORBIDDEN_IN_WRITER=(
    # A document, a kernel, an importer or a picture.
    Document
    DocumentId
    GeometryKernel
    OcctKernel
    Occt
    OCCT
    StepImporter
    Import
    RenderSnapshot
    SnapshotBuilder
    LiveScene
    Camera
    PickId
    ShapeHandle
    SubShapeHandle
    SessionId
    tessellate
    rebuild_cold
    # A filesystem. The sink is a `Write`; naming a path would be naming where
    # the bytes go, which is the caller's decision and not the writer's.
    Path
    PathBuf
    File
    OpenOptions
    read_to_string
    create_dir
    # A clock, an environment or a random number: three ways for one scene to
    # produce two files.
    SystemTime
    Instant
    UNIX_EPOCH
    now
    chrono
    rand
    random
    thread_rng
    var_os
    env
    # An unordered map iterated into the output would put a hash seed in the
    # file. Ordered collections and vectors are what a deterministic file is
    # built from.
    HashMap
    HashSet
    BTreeMap
    BTreeSet
)

for source in "${WRITER_SOURCES[@]}"; do
    [ -f "$source" ] || fail "the FBX writer source ${source} is missing"
done
for name in "${FORBIDDEN_IN_WRITER[@]}"; do
    for source in "${WRITER_SOURCES[@]}"; do
        [ -f "$source" ] || continue
        found="$(names "$source" "$name")"
        if [ -n "$found" ]; then
            echo "$found" >&2
            fail "the FBX writer names ${name} in ${source}"
        fi
    done
done

# And it is handed a scene and a sink, and nothing else. A third parameter
# would be a way to tell the writer what to say about a scene, and the first
# thing anybody would tell it is that a partial export is complete.
writer_signature="$(shipped crates/ferritecad-export/src/fbx/mod.rs \
    | tr '\n' ' ' \
    | grep -o 'pub fn write_fbx_ascii_7400([^)]*)' || true)"
if [ -z "$writer_signature" ]; then
    fail "crates/ferritecad-export/src/fbx/mod.rs does not define write_fbx_ascii_7400"
elif ! printf '%s' "$writer_signature" \
    | grep -qE '^pub fn write_fbx_ascii_7400\( *scene: &ExportScene, *output: &mut impl Write, *\)$'
then
    echo "$writer_signature" >&2
    fail "the FBX writer takes something other than a scene and a byte sink"
fi

# Nothing in the export model may be serialised.
if grep -rn --include='*.rs' -E 'derive\([^)]*\b(Serialize|Deserialize)\b' \
    crates/ferritecad-export/src | grep -q .; then
    grep -rn --include='*.rs' -E 'derive\([^)]*\b(Serialize|Deserialize)\b' \
        crates/ferritecad-export/src >&2
    fail "${EXPORT} derives serialisation; an export scene is one build's tessellation"
fi

# The production builder lives in the crate that knows a document and a kernel.
grep -q 'ferritecad-export' "${SCENE}/Cargo.toml" \
    || fail "${SCENE} does not depend on ${EXPORT}, so it is not building the model"
[ -f "${SCENE}/src/export.rs" ] \
    || fail "${SCENE}/src/export.rs is missing; the production builder belongs there"

# And there is exactly one preparation spine behind both the picture and the
# export. A second `Document::open_read_only` in this crate would be a second
# read of the file an export is not allowed to read twice.
sources=("${SCENE}"/src/*.rs)
for what in \
    'Document::open_read_only|opens a document' \
    'rebuild_cold(|rebuilds' \
    'reopen_step_import(|reads a stored STEP'
do
    pattern="${what%%|*}"
    said="${what##*|}"
    count="$(shipped_count "$pattern" "${sources[@]}")"
    if [ "$count" != "1" ]; then
        grep -rn --include='*.rs' -F "$pattern" "${SCENE}/src" >&2
        fail "${SCENE} ${said} in ${count} places; one load path means one of each"
    fi
done

if [ "$problems" -ne 0 ]; then
    echo "export boundary: ${problems} problem(s)" >&2
    exit 1
fi
echo "export boundary: the model is neutral and there is one load path"
