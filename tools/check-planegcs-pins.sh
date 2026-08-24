#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Checks that planegcs's native build inputs have one owner.
#
# FreeCAD, Eigen and Boost each have a version, an archive URL and a SHA-256,
# and all nine values live in tools/planegcs/pin.env. A second copy in a
# workflow is not a redundancy: it is a second answer, and the one that drifts
# is the one nobody is looking at. A Boost taken from apt, Homebrew or vcpkg is
# not a pin at all - it is whatever the runner image happened to bring, which
# makes the release input a property of the machine rather than of this
# repository. ADR 0002 is why that is not allowed on the release path.
#
# Scope is the executable inputs: tools/ and the workflows. Documents quote
# these values as a record and are checked for agreeing with the pin, not for
# staying silent.
#
# Run from the repository root:
#   tools/check-planegcs-pins.sh

set -euo pipefail

readonly PIN='tools/planegcs/pin.env'
readonly PLANEGCS_DOC='docs/build-planegcs.md'

problems=0
fail() {
    echo "error: $1" >&2
    problems=$((problems + 1))
}

[ -f "${PIN}" ] || { echo "error: ${PIN} is missing" >&2; exit 1; }

# Assignments and comments only. It is sourced by a shell and parsed by
# ferritecad-sketch-solver's build script, and the second of those understands
# nothing else.
if grep -vqE '^[[:space:]]*(#|$)|^[A-Z0-9_]+=[^[:space:]]+$' "${PIN}"; then
    grep -vnE '^[[:space:]]*(#|$)|^[A-Z0-9_]+=[^[:space:]]+$' "${PIN}" >&2
    fail "${PIN} holds something that is neither a comment nor a bare assignment"
fi

value_of() {
    grep -E "^$1=" "${PIN}" | cut -d= -f2-
}

pinned_digests=()
require_pin() {
    local name="$1" value count
    count="$(grep -cE "^${name}=" "${PIN}" || true)"
    if [ "${count}" -ne 1 ]; then
        fail "${PIN} declares ${name} ${count} times; exactly one owner is the point"
        return 1
    fi
    value="$(value_of "${name}")"
    [ -n "${value}" ] || { fail "${PIN} leaves ${name} empty"; return 1; }
    printf '%s' "${value}"
}

require_digest() {
    local name="$1" value
    value="$(require_pin "${name}")" || return 1
    if ! printf '%s' "${value}" | grep -qE '^[0-9a-f]{64}$'; then
        fail "${name} is ${value}, which is not a SHA-256"
        return 1
    fi
    pinned_digests+=("${value}")
    printf '%s' "${value}"
}

freecad_tag="$(require_pin FCAD_PLANEGCS_FREECAD_TAG || true)"
freecad_url="$(require_pin FCAD_PLANEGCS_FREECAD_URL || true)"
require_digest FCAD_PLANEGCS_ARCHIVE_SHA256 > /dev/null || true
eigen_version="$(require_pin FCAD_PLANEGCS_EIGEN_VERSION || true)"
eigen_url="$(require_pin FCAD_PLANEGCS_EIGEN_URL || true)"
require_digest FCAD_PLANEGCS_EIGEN_SHA256 > /dev/null || true
boost_version="$(require_pin FCAD_PLANEGCS_BOOST_VERSION || true)"
boost_url="$(require_pin FCAD_PLANEGCS_BOOST_URL || true)"
require_digest FCAD_PLANEGCS_BOOST_SHA256 > /dev/null || true

# A URL that does not carry its own version is a URL that keeps working after
# the version beside it was changed, and then the digest is the only thing that
# notices - one release too late.
[ -z "${freecad_tag}${freecad_url}" ] || case "${freecad_url}" in
    *"${freecad_tag}"*) ;;
    *) fail "the FreeCAD URL does not name ${freecad_tag}" ;;
esac
[ -z "${eigen_version}${eigen_url}" ] || case "${eigen_url}" in
    *"${eigen_version}"*) ;;
    *) fail "the Eigen URL does not name ${eigen_version}" ;;
esac
if [ -n "${boost_version}${boost_url}" ]; then
    boost_underscored="$(printf '%s' "${boost_version}" | tr . _)"
    case "${boost_url}" in
        *"${boost_underscored}"*) ;;
        *) fail "the Boost URL does not name ${boost_underscored}" ;;
    esac
fi

# No second copy of a pinned digest anywhere that runs.
for digest in ${pinned_digests[@]+"${pinned_digests[@]}"}; do
    if grep -rn --exclude=pin.env -F "${digest}" tools .github/workflows > /dev/null; then
        grep -rn --exclude=pin.env -F "${digest}" tools .github/workflows >&2
        fail "${digest} is pinned in ${PIN} and copied somewhere that runs"
    fi
done

# And no workflow-local pin of its own, digest or not.
if grep -rnE '^[[:space:]]*(EIGEN|BOOST)_(VERSION|SHA256|URL):' .github/workflows > /dev/null; then
    grep -rnE '^[[:space:]]*(EIGEN|BOOST)_(VERSION|SHA256|URL):' .github/workflows >&2
    fail "a workflow declares an Eigen or Boost pin of its own; ${PIN} owns them"
fi

# The release path may not be handed a dependency by the runner image.
if grep -rnE 'libboost|boost-graph|boost-math|boost-devel|libeigen|eigen3' \
    .github/workflows > /dev/null; then
    grep -rnE 'libboost|boost-graph|boost-math|boost-devel|libeigen|eigen3' \
        .github/workflows >&2
    fail "a workflow installs Eigen or Boost from a package manager; the pinned source is \
what planegcs is built against"
fi
if grep -rnE '(brew|choco|vcpkg|apt-get|apk|dnf|yum|pacman)[^|]*install.*[[:space:]](boost|eigen)([[:space:]]|$)' \
    .github/workflows > /dev/null; then
    grep -rnE '(brew|choco|vcpkg|apt-get|apk|dnf|yum|pacman)[^|]*install.*[[:space:]](boost|eigen)([[:space:]]|$)' \
        .github/workflows >&2
    fail "a workflow installs Eigen or Boost from a package manager"
fi

# Nor by an environment somebody set two steps earlier. The delivery helper
# works out both include directories from the pin; a workflow that exported
# them would be deciding what the release was built against.
if grep -rnE 'FCAD_(EIGEN|BOOST)_INCLUDE=.*GITHUB_ENV' .github/workflows > /dev/null; then
    grep -rnE 'FCAD_(EIGEN|BOOST)_INCLUDE=.*GITHUB_ENV' .github/workflows >&2
    fail "a workflow exports FCAD_EIGEN_INCLUDE or FCAD_BOOST_INCLUDE; the delivery helper \
derives both from ${PIN}"
fi
#
# Both places, not one. The MIT shim in ferritecad-sketch-solver compiles the
# same planegcs headers as the library, so it needs the same Eigen and the same
# Boost, and its build script held its own copy of the fallback this rule is
# about. Two copies of a rule is one place for it to come back.
readonly REDIRECTABLE=(
    'tools/build-planegcs.sh'
    'crates/ferritecad-sketch-solver/build.rs'
)
for source in "${REDIRECTABLE[@]}"; do
    if grep -nE '\$\{?FCAD_(EIGEN|BOOST)_INCLUDE|var(_os)?\("FCAD_(EIGEN|BOOST)_INCLUDE' \
        "${source}" > /dev/null; then
        grep -nE '\$\{?FCAD_(EIGEN|BOOST)_INCLUDE|var(_os)?\("FCAD_(EIGEN|BOOST)_INCLUDE' \
            "${source}" >&2
        fail "${source} reads FCAD_EIGEN_INCLUDE or FCAD_BOOST_INCLUDE; neither the delivery \
nor the shim may be redirected by an environment variable"
    fi
    # And no system include directory, which is the same rule wearing a
    # different hat: a header tree discovered on a machine is a build input
    # nobody recorded.
    if grep -nE '/(opt/homebrew|usr/local|usr)/include' "${source}" > /dev/null; then
        grep -nE '/(opt/homebrew|usr/local|usr)/include' "${source}" >&2
        fail "${source} names a system include directory; planegcs and its shim are compiled \
against the source ${PIN} pins"
    fi
done

# Which environment the shim's build script may read at all, by name.
#
# Naming the two forbidden variables was not enough, and a mutation showed it:
# the loop that configures the shim has the variable's name in a local, so
# `env::var_os(name)` reintroduces the redirect without either spelling
# appearing anywhere. Nothing about that is exotic - it is the shortest way to
# write it. So the rule is the other way round: every environment read in this
# build script names one of these, spelled out, and a read whose argument is
# not a literal from the list is refused whatever it turns out to say.
readonly BUILD_SCRIPT='crates/ferritecad-sketch-solver/build.rs'
readonly ALLOWED_ENV='CARGO_MANIFEST_DIR|CARGO_CFG_TARGET_OS|CARGO_FEATURE_PLANEGCS|OUT_DIR|FCAD_PLANEGCS_DIR|FERRITECAD_REQUIRE_PLANEGCS'
reads="$(grep -coE 'env::var(_os)?\(' "${BUILD_SCRIPT}" || true)"
allowed="$(grep -coE "env::var(_os)?\\(\"(${ALLOWED_ENV})\"\\)" "${BUILD_SCRIPT}" || true)"
if [ "${reads}" != "${allowed}" ]; then
    grep -nE 'env::var(_os)?\(' "${BUILD_SCRIPT}" >&2
    fail "${BUILD_SCRIPT} reads the environment somewhere this does not recognise: \
${reads} read(s), ${allowed} of them naming one of ${ALLOWED_ENV}. What the shim is compiled \
against comes from ${PIN} and from the delivery, not from a variable"
fi

# What the component artifact has to carry. The upload names paths, and a path
# dropped from that list takes the file out of the artifact without failing
# anything: `if-no-files-found: error` fires on a path that matches nothing,
# never on a path that is no longer written down.
uploaded="$(awk '
    /name: planegcs-\$\{\{ matrix\.name \}\}/ { artifact = 1; next }
    artifact && /path: \|/ { paths = 1; next }
    paths && /^[[:space:]]*[a-z][a-z-]*:/ { paths = 0; artifact = 0 }
    paths { print }
' .github/workflows/planegcs-pin.yml)"
for required in \
    'vendor/planegcs/sources/' \
    'vendor/planegcs/build-inputs/' \
    'vendor/planegcs/LICENSE-Eigen-MPL-2.0.txt' \
    'vendor/planegcs/LICENSE-Boost-BSL-1.0.txt' \
    'vendor/planegcs/LICENSE-FreeCAD-LGPL-2.0-or-later.txt' \
    'vendor/planegcs/PROVENANCE.txt' \
    'vendor/planegcs/REPLACING.md'
do
    printf '%s\n' "${uploaded}" | grep -Fq "${required}" \
        || fail "the planegcs component artifact does not upload ${required}"
done

# The document quotes the pin. A digest in it that the repository does not pin
# is a digest somebody typed.
if [ -f "${PLANEGCS_DOC}" ]; then
    while read -r quoted; do
        printf '%s\n' ${pinned_digests[@]+"${pinned_digests[@]}"} | grep -Fxq "${quoted}" \
            || fail "${PLANEGCS_DOC} quotes ${quoted}, which ${PIN} does not pin"
    done < <(grep -oE '[0-9a-f]{64}' "${PLANEGCS_DOC}" | sort -u)
fi

if [ "${problems}" -gt 0 ]; then
    echo >&2
    echo "planegcs's native build inputs have one owner; see ${PIN} and \
docs/decisions/0002-release-compliance-artifacts.md" >&2
    exit 1
fi

echo "planegcs pins: ${PIN} owns FreeCAD ${freecad_tag}, Eigen ${eigen_version} and Boost \
${boost_version}, nothing that runs holds a second copy, and the component artifact carries \
the source, build-input, licence and provenance files"
