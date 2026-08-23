#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Checks that one crate owns the sketch solver boundary.
#
# `ferritecad-sketch-solver` holds the contract, the FFI, the MIT bridge, the
# build detection and the native session's lifetime. `ferritecad-solver-lab` is
# a client of it and holds none of those. The direction matters both ways: a
# bench with its own copy of the boundary would be measuring a second
# implementation and reporting it as the product's, and a product that could
# reach into the bench could be handed the reference solver's answer.
#
# Checked mechanically because the copy that comes back is the one nobody is
# looking at, and because both halves keep compiling either way.
#
# Run from the repository root:
#   tools/check-solver-ownership.sh

set -euo pipefail

readonly PRODUCT='crates/ferritecad-sketch-solver'
readonly LAB='crates/ferritecad-solver-lab'

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

echo "solver ownership: ${PRODUCT} owns the boundary, ${LAB} is a client of it"
