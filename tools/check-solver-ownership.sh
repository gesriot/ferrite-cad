#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Checks that one crate owns the sketch solver boundary.
#
# `ferritecad-sketch-solver` holds the contract, the FFI, the MIT bridge, the
# build detection and the native session's lifetime. `ferritecad-solver-lab` is
# a client of it and holds none of those; since 21A-2b1 so is
# `ferritecad-app`, and since 21B-1b so is `ferritecad-eval`, which is what
# puts a solver on the rebuild path. The direction matters every way: a bench with its own copy
# of the boundary would be measuring a second implementation and reporting it
# as the product's; a product that could reach into the bench could be handed
# the reference solver's answer; and an application with its own link detection
# would hold a second opinion about whether there is a solver at all, free to
# disagree with the one the product crate reached.
#
# Checked mechanically because the copy that comes back is the one nobody is
# looking at, and because every half keeps compiling either way.
#
# Run from the repository root:
#   tools/check-solver-ownership.sh

set -euo pipefail

readonly PRODUCT='crates/ferritecad-sketch-solver'
readonly LAB='crates/ferritecad-solver-lab'
readonly APP='crates/ferritecad-app'
readonly EVAL='crates/ferritecad-eval'
readonly DOCUMENT='crates/ferritecad-document'

problems=0
fail() {
    echo "error: $1" >&2
    problems=$((problems + 1))
}

# The lab must hold no second copy of the boundary.
[ -e "${LAB}/build.rs" ] \
    && fail "${LAB}/build.rs exists; build and link detection belongs to ${PRODUCT}"
[ -d "${LAB}/planegcs-bridge" ] \
    && fail "${LAB}/planegcs-bridge exists; the C bridge belongs to ${PRODUCT}"

if grep -rqn 'extern "C"' "${LAB}"; then
    grep -rn 'extern "C"' "${LAB}" >&2
    fail "${LAB} declares a C boundary of its own"
fi
if grep -rqn 'unsafe' "${LAB}"/src "${LAB}"/tests; then
    grep -rn 'unsafe' "${LAB}"/src "${LAB}"/tests >&2
    fail "${LAB} contains unsafe code; the FFI belongs to ${PRODUCT}"
fi

# And it must reach planegcs the way the application will.
grep -q '^ferritecad-sketch-solver' "${LAB}/Cargo.toml" \
    || fail "${LAB} does not depend on ${PRODUCT}, so it is not using the product path"

# The product crate must not depend on the bench. Dependency entries only:
# the manifest explains this arrangement in prose, and a check that could not
# tell a comment from a dependency would have to be silenced to say anything.
if grep -Eqn '^[[:space:]]*ferritecad-solver-lab[[:space:]]*[=.]' "${PRODUCT}/Cargo.toml"; then
    fail "${PRODUCT} depends on ${LAB}; the product path must not reach the bench"
fi
if grep -rqn 'ferritecad_solver_lab' "${PRODUCT}/src" "${PRODUCT}/tests"; then
    grep -rn 'ferritecad_solver_lab' "${PRODUCT}/src" "${PRODUCT}/tests" >&2
    fail "${PRODUCT} names the bench; the product path must not reach it"
fi

# The application reaches planegcs through the product crate, and only so.
#
# It has a real diagnostic command now, `ferritecad-viewer --solver-info`, and
# that is exactly why these hold: a binary that can answer a question about the
# solver is a binary somebody could be tempted to teach the answer to.
grep -Eq '^[[:space:]]*ferritecad-sketch-solver[[:space:]]*[=.]' "${APP}/Cargo.toml" \
    || fail "${APP} does not depend on ${PRODUCT}, so it is not on the product path"
grep -Fq 'ferritecad-sketch-solver/planegcs' "${APP}/Cargo.toml" \
    || fail "${APP} does not forward the planegcs feature to ${PRODUCT}, so an application \
built to link the library would not link one"

if grep -Eqn '^[[:space:]]*ferritecad-solver-lab[[:space:]]*[=.]' "${APP}/Cargo.toml"; then
    fail "${APP} depends on ${LAB}; the application must never be able to reach the bench"
fi
if grep -rqn 'ferritecad_solver_lab' "${APP}/src" "${APP}/tests"; then
    grep -rn 'ferritecad_solver_lab' "${APP}/src" "${APP}/tests" >&2
    fail "${APP} names the bench; the reference solver is not a fallback"
fi

# And it holds no copy of the boundary of its own.
[ -e "${APP}/build.rs" ] \
    && fail "${APP}/build.rs exists; build and link detection belongs to ${PRODUCT}"
[ -d "${APP}/planegcs-bridge" ] \
    && fail "${APP}/planegcs-bridge exists; the C bridge belongs to ${PRODUCT}"

for forbidden in 'extern "C"' 'unsafe' 'fc_gcs_' 'link_name' '#[link'; do
    if grep -rqnF "${forbidden}" "${APP}/src" "${APP}/tests"; then
        grep -rnF "${forbidden}" "${APP}/src" "${APP}/tests" >&2
        fail "${APP} contains ${forbidden}; the FFI and its lifetime belong to ${PRODUCT}"
    fi
done

# The document must not reach the solver at all.
#
# A stored constraint is written in the document's own vocabulary on purpose:
# a `SketchPointRef` outlives the solver that reads it, and a solver `PointId`
# is issued per solve and does not outlive the call it was minted for. The
# moment a document could hold one, saving a sketch would mean saving a number
# that means nothing tomorrow.
if grep -Eqn '^[[:space:]]*ferritecad-sketch-solver[[:space:]]*[=.]' "${DOCUMENT}/Cargo.toml"; then
    fail "${DOCUMENT} depends on ${PRODUCT}; a stored constraint must not be written in solver terms"
fi
if grep -rqn 'ferritecad_sketch_solver' "${DOCUMENT}/src" "${DOCUMENT}/tests"; then
    grep -rn 'ferritecad_sketch_solver' "${DOCUMENT}/src" "${DOCUMENT}/tests" >&2
    fail "${DOCUMENT} names ${PRODUCT}; a stored constraint must not be written in solver terms"
fi

# The evaluator is the product path's client, and since 21B-1b it is the only
# one that solves anything.
#
# It is where a stored constraint becomes a solved coordinate, so it must reach
# the solver — and it must reach nothing else. The translation between the
# document's durable words and the solver's transient ones is a mapping it
# builds per solve and throws away; a build script, a C boundary or a link
# decision of its own would be a second opinion about whether there is a solver
# at all, and the bench is not a fallback for when there is not.
grep -Eq '^[[:space:]]*ferritecad-sketch-solver[[:space:]]*[=.]' "${EVAL}/Cargo.toml" \
    || fail "${EVAL} does not depend on ${PRODUCT}, so a constrained sketch has nothing to solve it"
grep -Fq 'ferritecad-sketch-solver/planegcs' "${EVAL}/Cargo.toml" \
    || fail "${EVAL} does not forward the planegcs feature to ${PRODUCT}, so a build meant to \
link the library would not link one on the rebuild path"
grep -Fqx "      - 'crates/ferritecad-eval/**'" .github/workflows/planegcs-pin.yml \
    || fail "planegcs pin does not watch ${EVAL}; a translation change would miss linked \
three-platform coverage"

if grep -Eqn '^[[:space:]]*ferritecad-solver-lab[[:space:]]*[=.]' "${EVAL}/Cargo.toml"; then
    fail "${EVAL} depends on ${LAB}; the rebuild path must never be able to reach the bench"
fi
if grep -rqn 'ferritecad_solver_lab' "${EVAL}/src" "${EVAL}/tests"; then
    grep -rn 'ferritecad_solver_lab' "${EVAL}/src" "${EVAL}/tests" >&2
    fail "${EVAL} names ${LAB}; the reference solver is not a fallback"
fi

[ -e "${EVAL}/build.rs" ] \
    && fail "${EVAL}/build.rs exists; build and link detection belongs to ${PRODUCT}"
[ -d "${EVAL}/planegcs-bridge" ] \
    && fail "${EVAL}/planegcs-bridge exists; the C bridge belongs to ${PRODUCT}"

for forbidden in 'extern "C"' 'unsafe' 'fc_gcs_' 'link_name' '#[link'; do
    if grep -rqnF "${forbidden}" "${EVAL}/src" "${EVAL}/tests"; then
        grep -rnF "${forbidden}" "${EVAL}/src" "${EVAL}/tests" >&2
        fail "${EVAL} contains ${forbidden}; the FFI and its lifetime belong to ${PRODUCT}"
    fi
done

# The bridge and the build detection are where they are said to be.
for required in \
    "${PRODUCT}/build.rs" \
    "${PRODUCT}/planegcs-bridge/planegcs_shim.c"* \
    "${PRODUCT}/planegcs-bridge/planegcs_shim.h" \
    "${PRODUCT}/planegcs-bridge/CMakeLists.txt"
do
    [ -e "${required}" ] || fail "${required} is missing; ${PRODUCT} must own it"
done

if [ "${problems}" -gt 0 ]; then
    echo >&2
    echo "one crate owns the sketch solver boundary; see docs/build-planegcs.md" >&2
    exit 1
fi

echo "solver ownership: ${PRODUCT} owns the boundary; ${LAB}, ${APP} and ${EVAL} are clients of \
it, and ${DOCUMENT} does not know it exists"
