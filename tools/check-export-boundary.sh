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
readonly JOBS='ferritecad-jobs'
readonly SCENE='crates/ferritecad-scene'

# Not in the shared job's normal build graph, at any depth. The job knows a
# document, a rebuild and a filesystem — that is what it is for — and must
# still reach no kernel implementation, no window and no renderer: the caller
# opens the session and hands it in, and what an export writes cannot depend on
# whether anything is on screen.
readonly FORBIDDEN_FOR_JOBS=(
    ferritecad-occt
    ferritecad-viewport-gpu
    ferritecad-ui
    ferritecad-app
    ferritecad-cli
    wgpu
    egui
    winit
    rfd
)

# And a window must never reach the command line. The two interfaces share the
# work through the job; a viewer that could name the CLI crate could start it,
# and starting it is the second export this whole arrangement exists to
# prevent.
readonly FORBIDDEN_FOR_APP=(
    ferritecad-cli
)

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

# The resolved normal-dependency closure of one crate, by name.
closure_of() {
    jq -r --arg root "$1" '
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
    ' "$metadata"
}

# Whether any of the named crates is in one crate's closure.
refuses_in_closure() {
    local root="$1"
    local why="$2"
    shift 2
    local resolved
    resolved="$(closure_of "$root")"
    if [ -z "$resolved" ]; then
        echo "error: could not resolve the dependency closure of ${root}" >&2
        exit 1
    fi
    local present crate
    while IFS= read -r present; do
        for crate in "$@"; do
            if [ "$present" = "$crate" ]; then
                fail "${root} depends on ${crate}; ${why}"
            fi
        done
    done <<<"$resolved"
}

closure="$(closure_of "$EXPORT")"

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

# The shared job may know a document and a filesystem, and may not know a
# kernel implementation, a window or a renderer.
refuses_in_closure "$JOBS" \
    "the shared export job owns no session, no window and no picture" \
    "${FORBIDDEN_FOR_JOBS[@]}"

# And the window may not reach the command line at all: a viewer that could
# name that crate could start it as a child process, which is the second export
# this arrangement exists to prevent.
refuses_in_closure "ferritecad-app" \
    "a window must not be able to reach the command line" \
    "${FORBIDDEN_FOR_APP[@]}"


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
    # The durable identity of a placement. §22B-1e3a gives every placement one
    # and delivers it to the neutral boundary, and stops there: what a writer
    # should do with it depends on what a name is in the target program, which
    # §22B-1e1 and §22B-1e2a measured and no slice has yet acted on. A writer
    # that started reading it would change the bytes of every existing export
    # under a slice whose whole claim is that it changes none.
    ExportOccurrence
    OccurrenceId
    occurrence
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

# And the model does carry it, so the gate above is about a writer that
# refuses to read something that is there rather than about a name nothing
# defines yet.
if ! grep -q 'pub occurrence: ExportOccurrence,' "$MODEL"; then
    fail "the export model has no placement identity on its nodes"
fi
if ! grep -q 'pub enum ExportOccurrence {' "$MODEL"; then
    fail "the export model does not define ExportOccurrence"
fi
# And it is delivered by the one load spine, from the stored payload and from
# nowhere else. A `NodeIdentity::Occurrence` minted in the scene crate would be
# a durable identity that is new on every export, and would compile.
readonly SPINE="${SCENE}/src/prepare.rs"
if [ "$(shipped_count 'OccurrenceId::new(' "$SPINE" "${SCENE}/src/export.rs")" != "0" ]; then
    fail "${SCENE} mints a placement identity; identities come from the stored payload"
fi
if [ "$(shipped_count 'reopened.occurrences()' "$SPINE")" != "1" ]; then
    fail "${SPINE} does not read the stored placement identities exactly once"
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

# --------------------------------------------------------- the one job
#
# There is one route from a stored document to a published FBX, and both
# interfaces take it. Everything that makes it that route rather than a second
# one is invisible to the compiler: a job that reached for a flattened picture,
# opened the document a second time, read the external STEP again, called the
# writer twice, wrote straight into the destination, published before the
# writer finished, published after the request was withdrawn, or worked out its
# own list of what is missing would all build.
readonly JOB='crates/ferritecad-jobs/src/fbx.rs'
readonly PUBLISH='crates/ferritecad-jobs/src/publish.rs'
readonly ROUTE='crates/ferritecad-cli/src/export_fbx.rs'
readonly MAIN='crates/ferritecad-cli/src/main.rs'
readonly WINDOW='crates/ferritecad-app/src/exports.rs'

for source in "$JOB" "$PUBLISH" "$ROUTE" "$MAIN" "$WINDOW"; do
    [ -f "$source" ] || fail "${source} is missing"
done

if [ -f "$JOB" ] && [ -f "$ROUTE" ] && [ -f "$MAIN" ] && [ -f "$WINDOW" ]; then
    # The neutral model is built by the crate that knows a document and a
    # kernel, so the job must actually depend on it rather than on a picture it
    # assembled itself.
    if ! awk '/^\[dev-dependencies\]/ { exit } { print }' \
        crates/ferritecad-jobs/Cargo.toml | grep -q 'ferritecad-scene'; then
        fail "ferritecad-jobs does not depend on ferritecad-scene, so the shared job is not on \
the production route"
    fi
    # And both interfaces must reach the export through that job.
    for crate in ferritecad-cli ferritecad-app; do
        if ! awk '/^\[dev-dependencies\]/ { exit } { print }' \
            "crates/${crate}/Cargo.toml" | grep -q 'ferritecad-jobs'; then
            fail "${crate} does not depend on ferritecad-jobs, so it is not on the shared route"
        fi
    done

    # A flattened picture multiplies every placement out and throws the
    # assembly away. Handing one to the FBX writer would export the same model
    # as a hundred and twelve unrelated draws. The job must not name a clock or
    # a random number either: an export is a function of the document.
    for name in \
        RenderSnapshot SnapshotBuilder PreparedSnapshot LoadedScene load_scene CatalogueEntry \
        sketch_drawing Camera PickId FacePickId EdgePickId VertexPickId \
        ShapeHandle SubShapeHandle SessionId binary_stl export_stl \
        OcctKernel Renderer Window \
        SystemTime Instant now rand random
    do
        found="$(names "$JOB" "$name")"
        if [ -n "$found" ]; then
            echo "$found" >&2
            fail "the shared export job names ${name}"
        fi
    done

    # Exactly one of each. A second document read, a second rebuild, a second
    # reading of the stored STEP or a second call to the writer would each be a
    # second answer to a question this job answers once.
    for what in \
        'export_scene(|builds the export scene|1' \
        'write_fbx_ascii_7400(|calls the writer|1' \
        'OcctKernel::new(|opens a kernel session of its own|0' \
        'Document::open|opens the document itself|0' \
        'rebuild_cold(|rebuilds the document itself|0' \
        'reopen_step_import(|reads a stored import itself|0' \
        'std::fs::read(|reads a file of its own|0' \
        'tessellate(|asks the kernel for geometry|0' \
        'Command::new(|starts another program|0'
    do
        pattern="${what%%|*}"
        rest="${what#*|}"
        said="${rest%%|*}"
        want="${rest##*|}"
        count="$(shipped_count "$pattern" "$JOB")"
        if [ "$count" != "$want" ]; then
            fail "the shared export job ${said} ${count} time(s), expected ${want}"
        fi
    done

    # The scratch file is the only thing this opens, and publication is the
    # shared one rather than a second implementation of it.
    opened="$(shipped_count '.open(' "$JOB")"
    scratch="$(shipped_count '.open(temporary.path())' "$JOB")"
    if [ "$opened" != "1" ] || [ "$scratch" != "1" ]; then
        fail "the shared export job opens ${opened} path(s), of which ${scratch} is the scratch \
file; it must open exactly the scratch file and nothing else"
    fi
    for pattern in 'std::fs::rename' 'hard_link' 'std::fs::write' 'std::fs::copy' 'File::create'; do
        if [ "$(shipped_count "$pattern" "$JOB")" != "0" ]; then
            fail "the shared export job names ${pattern}; atomic publication has one \
implementation"
        fi
    done
    for pattern in 'Temporary::beside(' 'create_new(true)'; do
        if [ "$(shipped_count "$pattern" "$JOB")" != "1" ]; then
            fail "the shared export job does not use ${pattern} exactly once"
        fi
    done
    # Defined once and called once: the publication and the check that guards
    # it are one function, so there is no second way to reach the destination.
    if [ "$(shipped_count 'publish_if_still_wanted(' "$JOB")" != "2" ]; then
        fail "the shared export job does not define and call publish_if_still_wanted exactly once"
    fi
    # One publication, and it is inside the one function that checks first.
    if [ "$(shipped_count 'temporary.publish(' "$JOB")" != "1" ]; then
        fail "the shared export job publishes from somewhere other than one place"
    fi

    # And nothing is published until the writer has finished. Ordering, because
    # a publication moved above the write would compile and would put a
    # half-written file where the user asked for a finished one.
    wrote="$(shipped "$JOB" | grep -n 'write_fbx_ascii_7400(' | head -1 | cut -d: -f1)"
    published="$(shipped "$JOB" | grep -n 'temporary.publish(' | head -1 | cut -d: -f1)"
    if [ -z "$wrote" ] || [ -z "$published" ] || [ "$wrote" -ge "$published" ]; then
        fail "the shared export job publishes at line ${published:-none} and writes at line \
${wrote:-none}; the writer must finish first"
    fi

    # And a request that has been withdrawn publishes nothing. The check stands
    # immediately before the publication, in the same function, so there is no
    # arrangement in which a cancellation arrives in time and the file appears
    # anyway.
    checked="$(shipped "$JOB" | grep -n 'cancel.check()?;' | tail -1 | cut -d: -f1)"
    if [ -z "$checked" ] || [ "$checked" -ge "$published" ]; then
        fail "the shared export job checks for cancellation at line ${checked:-none} and \
publishes at line ${published}; the last check must come first"
    fi

    # Serialisation is cancellable too, through the sink rather than through
    # the writer: the writer is handed a scene and a byte sink and nothing
    # else, and that is what makes its output a function of the scene.
    if [ "$(shipped_count 'Cancellable' "$JOB")" -lt "3" ]; then
        fail "the shared export job does not write through a sink that can be told to stop"
    fi

    # ------------------------------------------------ the command line
    #
    # A thin adapter: it opens the session, calls the job once, and turns the
    # outcome into a number and some text. It must not have an export of its
    # own.
    for what in \
        'export_document_as_fbx(|calls the shared job|1' \
        'OcctKernel::new(|opens the kernel session|1' \
        'export_scene(|builds a scene of its own|0' \
        'write_fbx_ascii_7400(|calls the writer itself|0' \
        'Temporary|publishes a file itself|0' \
        'Command::new(|starts another program|0' \
        'Document::open|opens the document itself|0' \
        'rebuild_cold(|rebuilds the document itself|0' \
        'reopen_step_import(|reads a stored import itself|0' \
        'std::fs::read(|reads a file of its own|0' \
        'tessellate(|asks the kernel for geometry|0'
    do
        pattern="${what%%|*}"
        rest="${what#*|}"
        said="${rest%%|*}"
        want="${rest##*|}"
        count="$(shipped_count "$pattern" "$ROUTE")"
        if [ "$count" != "$want" ]; then
            fail "the shipped FBX command ${said} ${count} time(s), expected ${want}"
        fi
    done
    for name in \
        RenderSnapshot SnapshotBuilder LoadedScene load_scene CatalogueEntry \
        sketch_drawing Camera PickId FacePickId EdgePickId VertexPickId \
        ShapeHandle SubShapeHandle SessionId binary_stl export_stl \
        SystemTime Instant now rand random
    do
        found="$(names "$ROUTE" "$name")"
        if [ -n "$found" ]; then
            echo "$found" >&2
            fail "the shipped FBX command names ${name}"
        fi
    done

    # The report is the writer's own record and not a second opinion. Reading
    # the scene's completeness here would be a list that could disagree with
    # the file that was published.
    if [ "$(shipped_count 'report.omissions()' "$ROUTE")" -lt "1" ]; then
        fail "the shipped FBX command does not report from FbxWriteReport::omissions()"
    fi
    if [ "$(shipped_count 'completeness()' "$ROUTE")" != "0" ]; then
        fail "the shipped FBX command reads the scene's completeness; the writer already answered"
    fi
    # Neither a `Debug` rendering nor a refusal's message is a fact. The typed
    # refusal has a stable name and that is what a report records.
    if [ "$(shipped_count 'refusal.stable_name()' "$ROUTE")" != "1" ]; then
        fail "the shipped FBX command does not record the typed refusal by its stable name"
    fi
    for pattern in '{:?}' 'refusal.to_string()' '{refusal}'; do
        if [ "$(shipped_count "$pattern" "$ROUTE")" != "0" ]; then
            fail "the shipped FBX command renders ${pattern} into its report"
        fi
    done
    # An imported key is local to its file, so it never travels without the
    # identity of the bytes it came from.
    qualified="$(shipped "$ROUTE" | grep -F 'imported source {source}' || true)"
    if [ "$(shipped_count 'definition_key' "$ROUTE")" -lt "1" ] || [ -z "$qualified" ]; then
        fail "the shipped FBX command does not qualify an imported key with its source"
    fi

    # The command is wired to this route and not to the STL one beside it.
    if ! grep -q 'Command::ExportFbx(args) => export_fbx::export_fbx(args),' "$MAIN"; then
        fail "${MAIN} does not route export-fbx to the FBX export"
    fi

    # And a partial export has a code of its own. Two constants sharing a value
    # would make a script unable to tell "the file is not the whole model" from
    # anything else.
    codes="$(grep -oE '^const EXIT_[A-Z]+: u8 = [0-9]+;' "$MAIN" || true)"
    values="$(printf '%s\n' "$codes" | grep -oE '= [0-9]+;' | tr -d '= ;')"
    if [ "$(printf '%s\n' "$values" | sort -u | wc -l)" \
        != "$(printf '%s\n' "$values" | wc -l)" ]; then
        printf '%s\n' "$codes" >&2
        fail "two exit codes share a value"
    fi
    if ! grep -q '^const EXIT_PARTIAL: u8 = 6;$' "$MAIN"; then
        fail "${MAIN} does not define the partial export as exit code 6"
    fi

    # ----------------------------------------------------- the window
    #
    # The other adapter, on the same terms: one session, one call to the job,
    # and no export of its own. It must also not reach the picture — what is
    # written is a cold read of the stored document, never what is on screen —
    # and must not start a program.
    for what in \
        'export_document_as_fbx(|calls the shared job|1' \
        'OcctKernel::new(|opens the kernel session|1' \
        'export_scene(|builds a scene of its own|0' \
        'write_fbx_ascii_7400(|calls the writer itself|0' \
        'Temporary|publishes a file itself|0' \
        'std::fs::rename|replaces a file itself|0' \
        'hard_link|publishes a file itself|0' \
        'Command::new(|starts another program|0' \
        'completeness()|works out what is missing itself|0' \
        'Document::open|opens the document itself|0' \
        'rebuild_cold(|rebuilds the document itself|0' \
        'reopen_step_import(|reads a stored import itself|0' \
        'std::fs::read(|reads a file of its own|0' \
        'tessellate(|asks the kernel for geometry|0' \
        '{:?}|renders a debugging aid as data|0'
    do
        pattern="${what%%|*}"
        rest="${what#*|}"
        said="${rest%%|*}"
        want="${rest##*|}"
        count="$(shipped_count "$pattern" "$WINDOW")"
        if [ "$count" != "$want" ]; then
            fail "the window's export ${said} ${count} time(s), expected ${want}"
        fi
    done
    for name in \
        RenderSnapshot PreparedSnapshot SnapshotBuilder LoadedScene LiveScene Renderer \
        Selection Visibility Hovered Camera PickId snapshot_of
    do
        found="$(names "$WINDOW" "$name")"
        if [ -n "$found" ]; then
            echo "$found" >&2
            fail "the window's export names ${name}; an export is a cold read of the document"
        fi
    done
    # The same rules about what a report may say, in the other interface.
    if [ "$(shipped_count 'report.omissions()' "$WINDOW")" -lt "1" ]; then
        fail "the window's export does not report from FbxWriteReport::omissions()"
    fi
    if [ "$(shipped_count 'refusal.stable_name()' "$WINDOW")" != "1" ]; then
        fail "the window's export does not record the typed refusal by its stable name"
    fi
    qualified="$(shipped "$WINDOW" | grep -F 'imported source {source}' || true)"
    if [ "$(shipped_count 'definition_key' "$WINDOW")" -lt "1" ] || [ -z "$qualified" ]; then
        fail "the window's export does not qualify an imported key with its source"
    fi

    # And no part of the window may start a program. This is the whole crate,
    # not only the export: a subprocess anywhere in a viewer that ships beside
    # a command line of the same name is the thing to refuse outright.
    app_sources=(crates/ferritecad-app/src/*.rs)
    for pattern in 'std::process::Command' 'Command::new('; do
        if [ "$(shipped_count "$pattern" "${app_sources[@]}")" != "0" ]; then
            fail "the window names ${pattern}; it must never start another program"
        fi
    done

    # The document an export reads is the one that was accepted, and it is
    # replaced by the statement that replaces the picture and by no other.
    if [ "$(shipped_count 'Some(next.document)' \
        crates/ferritecad-app/src/main.rs)" != "1" ]; then
        fail "the window does not name the accepted document where it commits a picture"
    fi
    if [ "$(shipped_count 'scene.document' crates/ferritecad-app/src/main.rs)" -lt "1" ]; then
        fail "the window does not read the accepted document from the picture on screen"
    fi
    # Beginning an Open stops the export of the document being left behind,
    # so an answer about the old document cannot arrive describing the new one.
    # The check the job makes before it publishes is what makes that safe; this
    # is what makes it happen.
    stopped="$(shipped crates/ferritecad-app/src/main.rs \
        | grep -A 8 -F 'fn open(&mut self, path: PathBuf) {' \
        | grep -F 'exports::cancel_export(' || true)"
    if [ -z "$stopped" ]; then
        fail "beginning an Open does not stop the export of the document being left behind"
    fi

    # And the window offers the action from the picture rather than from the
    # path a file dialog remembers.
    if [ "$(shipped_count 'fn can_export<P>(scene: &LiveScene<P>) -> bool {' \
        crates/ferritecad-app/src/main.rs)" != "1" ]; then
        fail "the window does not decide in one place whether there is anything to export"
    fi

    # And the line that starts one reads the picture rather than the path the
    # file dialog remembers, which is already the next document by then.
    started="$(shipped crates/ferritecad-app/src/main.rs \
        | grep -F 'exports::begin_export(' -B 5 -A 5 | grep -F 'self.document' || true)"
    if [ -n "$started" ]; then
        echo "$started" >&2
        fail "the window's export reads the last path the user named rather than the document \
on screen"
    fi
fi

if [ "$problems" -ne 0 ]; then
    echo "export boundary: ${problems} problem(s)" >&2
    exit 1
fi
echo "export boundary: the model is neutral, there is one load path, one shared export job \
and two thin adapters over it"
