#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Checks that a planegcs delivery carries its native build inputs.
#
# planegcs is compiled against Eigen and Boost. Eigen is MPL-2.0 code that ends
# up inside the shared library, so a package carrying that library owes its
# recipient the exact Source Code Form and the MPL text. Boost's object-code
# exception means the library alone owes nothing, but which Boost produced the
# build is still part of what the release has to be able to say, and the short
# licence text is carried as a deliberate inventory choice. ADR 0002 records
# both decisions.
#
# The delivery is checked against the archives it was built from rather than
# against wording anyone could write beside it: the helper leaves those
# archives in the output's work directory, this re-verifies their digests, and
# the Eigen source and both licence texts are compared with what comes back out
# of them.
#
#   tools/check-planegcs-delivery.sh <delivery-directory>

set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "usage: tools/check-planegcs-delivery.sh <delivery-directory>" >&2
    exit 2
fi

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=tools/planegcs/pin.env
. "${here}/planegcs/pin.env"

out="$1"
if [ ! -d "${out}" ]; then
    echo "error: ${out} is not a directory" >&2
    exit 1
fi
out="$(cd "${out}" && pwd)"

problems=0
fail() {
    echo "error: $1" >&2
    problems=$((problems + 1))
}

digest_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

scratch="$(mktemp -d)"
trap 'rm -rf "${scratch}"' EXIT

work="${out}/work"
eigen_tree="${out}/sources/eigen-${FCAD_PLANEGCS_EIGEN_VERSION}"
boost_prefix="boost_$(printf '%s' "${FCAD_PLANEGCS_BOOST_VERSION}" | tr . _)"

# The archives the helper checked and kept. Re-verified here rather than taken
# on trust: everything below reads them, and an archive that was replaced after
# the build would otherwise make this whole check agree with the wrong thing.
verified_archive() {
    local archive="$1" expected="$2" what="$3" actual
    if [ ! -f "${archive}" ]; then
        fail "${archive} is missing, so the delivered ${what} cannot be checked against the \
archive it came from"
        return 1
    fi
    actual="$(digest_of "${archive}")"
    if [ "${actual}" != "${expected}" ]; then
        fail "${archive} has digest ${actual} and the pin says ${expected}"
        return 1
    fi
    return 0
}

freecad_archive="${work}/freecad-${FCAD_PLANEGCS_FREECAD_TAG}.tar.gz"
eigen_archive="${work}/eigen-${FCAD_PLANEGCS_EIGEN_VERSION}.tar.gz"
boost_archive="${work}/boost-${FCAD_PLANEGCS_BOOST_VERSION}.tar.gz"

verified_archive "${freecad_archive}" "${FCAD_PLANEGCS_ARCHIVE_SHA256}" "FreeCAD source" || true

# The exact Eigen Source Code Form, and that it is exactly the archive's.
if [ ! -d "${eigen_tree}" ]; then
    fail "${eigen_tree} is missing; the delivery does not carry the Eigen source compiled \
into the library"
elif verified_archive "${eigen_archive}" "${FCAD_PLANEGCS_EIGEN_SHA256}" "Eigen source"; then
    mkdir -p "${scratch}/eigen"
    tar xzf "${eigen_archive}" -C "${scratch}/eigen" --strip-components=1
    if ! diff -r "${scratch}/eigen" "${eigen_tree}" > "${scratch}/eigen.diff" 2>&1; then
        head -20 "${scratch}/eigen.diff" >&2
        fail "the delivered Eigen source is not the checked archive's"
    fi
fi

# The MPL text, and that it is the one from that same source rather than a copy
# of the licence from anywhere else.
mpl="${out}/LICENSE-Eigen-MPL-2.0.txt"
if [ ! -s "${mpl}" ]; then
    fail "${mpl} is missing or empty; a package carrying MPL-2.0 code carries its text"
elif [ ! -f "${eigen_tree}/COPYING.MPL2" ]; then
    fail "${eigen_tree}/COPYING.MPL2 is missing, so the MPL text beside the library answers \
to nothing"
elif ! cmp -s "${mpl}" "${eigen_tree}/COPYING.MPL2"; then
    fail "${mpl} is not the MPL text in the Eigen source it was built from"
fi

# Boost's whole source is not delivered - the object-code exception is why -
# so the text is checked against the archive rather than against a sibling.
bsl="${out}/LICENSE-Boost-BSL-1.0.txt"
if [ ! -s "${bsl}" ]; then
    fail "${bsl} is missing or empty"
elif verified_archive "${boost_archive}" "${FCAD_PLANEGCS_BOOST_SHA256}" "Boost licence"; then
    tar xzf "${boost_archive}" -C "${scratch}" "${boost_prefix}/LICENSE_1_0.txt"
    if ! cmp -s "${bsl}" "${scratch}/${boost_prefix}/LICENSE_1_0.txt"; then
        fail "${bsl} is not the licence text in the checked Boost archive"
    fi
fi

# Provenance and the replacement notice must name all three inputs. Not "say
# something about Eigen": the version, the URL and the digest a recipient would
# need to fetch and check the same source themselves.
names_input() {
    local file="$1" what="$2" version="$3" url="$4" digest="$5"
    [ -f "${file}" ] || { fail "${file} is missing"; return; }
    grep -Fq "${version}" "${file}" \
        || fail "$(basename "${file}") does not name the ${what} version ${version}"
    grep -Fq "${url}" "${file}" \
        || fail "$(basename "${file}") does not name the ${what} archive ${url}"
    grep -Fq "${digest}" "${file}" \
        || fail "$(basename "${file}") does not name the ${what} digest ${digest}"
}

for notice in "${out}/PROVENANCE.txt" "${out}/REPLACING.md"; do
    names_input "${notice}" FreeCAD "${FCAD_PLANEGCS_FREECAD_TAG}" \
        "${FCAD_PLANEGCS_FREECAD_URL}" "${FCAD_PLANEGCS_ARCHIVE_SHA256}"
    names_input "${notice}" Eigen "${FCAD_PLANEGCS_EIGEN_VERSION}" \
        "${FCAD_PLANEGCS_EIGEN_URL}" "${FCAD_PLANEGCS_EIGEN_SHA256}"
    names_input "${notice}" Boost "${FCAD_PLANEGCS_BOOST_VERSION}" \
        "${FCAD_PLANEGCS_BOOST_URL}" "${FCAD_PLANEGCS_BOOST_SHA256}"
done

# A notice generated from the template must have been filled in. The digits
# matter: the first version of this pattern was [A-Z_]* and @SHA256@ went
# through it, which is the one placeholder whose survival is unreadable as a
# mistake rather than as a fact.
if [ -f "${out}/REPLACING.md" ] && grep -q '@[A-Z0-9_]*@' "${out}/REPLACING.md"; then
    grep -n '@[A-Z0-9_]*@' "${out}/REPLACING.md" >&2
    fail "the delivery notice still has unfilled placeholders"
fi

if [ "${problems}" -gt 0 ]; then
    echo >&2
    echo "the planegcs delivery does not carry its native build inputs; see \
docs/decisions/0002-release-compliance-artifacts.md" >&2
    exit 1
fi

echo "planegcs delivery: the Eigen source is the checked archive's, both licence texts come \
from the archives they belong to, and FreeCAD, Eigen and Boost are each named with a version, \
a URL and a digest"
