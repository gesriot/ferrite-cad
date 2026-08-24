#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Builds FreeCAD's planegcs as a replaceable shared library, on Linux, macOS
# and Windows alike, and packages it with what its licence obliges a sender to
# hand over.
#
# planegcs is LGPL-2.0-or-later, so it is linked dynamically and the library
# this produces can be replaced by the user with their own build. Nothing of it
# is compiled into a FerriteCAD binary. See docs/build-planegcs.md for the
# whole statement and THIRD_PARTY_LICENSES.md for the terms.
#
# The LGPL sources are used byte-identical. The only files added beside them
# are FerriteCAD's own build glue from tools/planegcs/glue, marked as such.
#
# Eigen and Boost are fetched here too, by the digests in tools/planegcs/pin.env
# and by nothing else. They used to be whatever headers the machine happened to
# have, which is fine for an experiment and is not a release input: Eigen's
# MPL-2.0 code is compiled into the library, so the exact source has to travel
# with it, and a version discovered on a runner cannot be named in provenance
# before the build that discovers it. There is deliberately no environment
# variable that redirects either one - for an experimental build against your
# own headers, drive tools/planegcs/CMakeLists.txt directly.
#
#   tools/build-planegcs.sh [output-directory]
#
# On Windows run it from a shell that has already seen vcvars, so that cmake
# finds MSVC's cl.exe. Then:
#
#   FCAD_PLANEGCS_DIR=<output> cargo test -p ferritecad-solver-lab \
#       --features planegcs

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
definition="${here}/planegcs"

# shellcheck source=tools/planegcs/pin.env
. "${definition}/pin.env"

FREECAD_TAG="${FCAD_PLANEGCS_FREECAD_TAG}"
FREECAD_SHA256="${FCAD_PLANEGCS_ARCHIVE_SHA256}"
ARCHIVE_URL="${FCAD_PLANEGCS_FREECAD_URL}"
EIGEN_VERSION="${FCAD_PLANEGCS_EIGEN_VERSION}"
EIGEN_URL="${FCAD_PLANEGCS_EIGEN_URL}"
EIGEN_SHA256="${FCAD_PLANEGCS_EIGEN_SHA256}"
BOOST_VERSION="${FCAD_PLANEGCS_BOOST_VERSION}"
BOOST_URL="${FCAD_PLANEGCS_BOOST_URL}"
BOOST_SHA256="${FCAD_PLANEGCS_BOOST_SHA256}"
# What the Boost archive calls its own top directory.
BOOST_PREFIX="boost_$(printf '%s' "${BOOST_VERSION}" | tr . _)"

host_os="$(uname -s)"
case "${host_os}" in
  Darwin)
    platform="macOS"
    library_name="libplanegcs.dylib"
    import_library_name="" ;;
  Linux)
    platform="Linux"
    library_name="libplanegcs.so"
    import_library_name="" ;;
  MINGW*|MSYS*|CYGWIN*)
    platform="Windows"
    library_name="planegcs.dll"
    # Linker metadata only. It carries no planegcs implementation; what it does
    # is let somebody relink against a library they replaced.
    import_library_name="planegcs.lib" ;;
  *)
    echo "unsupported host ${host_os}" >&2
    exit 1 ;;
esac

# A Windows shell hands out both spellings of a path, and the two halves of
# this script want different ones: the tests below are POSIX, cmake is a native
# program. Neither conversion is guesswork about which one arrived, so both
# directions are explicit.
posix() {
  if [ "${platform}" = "Windows" ]; then cygpath -u "$1"; else printf '%s' "$1"; fi
}
native() {
  if [ "${platform}" = "Windows" ]; then cygpath -m "$1"; else printf '%s' "$1"; fi
}

OUT="$(posix "${1:-$(pwd)/vendor/planegcs}")"
mkdir -p "${OUT}"
OUT="$(cd "${OUT}" && pwd)"
WORK="${OUT}/work"
mkdir -p "${WORK}"

digest_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

# Fetched if it is not here, and verified whether or not it was: a cached
# archive is an archive somebody could have replaced since it was fetched, and
# the check is worth exactly as much as the times it is skipped.
#
# All three are done before anything is extracted, so a wrong digest on any one
# of them stops the build with nobody's bytes unpacked.
fetch_verified() {
  local archive="$1" url="$2" expected="$3" what="$4" actual
  if [ ! -f "${archive}" ]; then
    echo "fetching ${what}"
    curl -sSL -o "${archive}" "${url}"
  fi
  actual="$(digest_of "${archive}")"
  if [ "${actual}" != "${expected}" ]; then
    echo "checksum mismatch for ${archive}" >&2
    echo "  expected ${expected}" >&2
    echo "  actual   ${actual}" >&2
    exit 1
  fi
  echo "checksum ok ${what}"
}

archive="${WORK}/freecad-${FREECAD_TAG}.tar.gz"
eigen_archive="${WORK}/eigen-${EIGEN_VERSION}.tar.gz"
boost_archive="${WORK}/boost-${BOOST_VERSION}.tar.gz"

fetch_verified "${archive}" "${ARCHIVE_URL}" "${FREECAD_SHA256}" "FreeCAD ${FREECAD_TAG}"
fetch_verified "${eigen_archive}" "${EIGEN_URL}" "${EIGEN_SHA256}" "Eigen ${EIGEN_VERSION}"
fetch_verified "${boost_archive}" "${BOOST_URL}" "${BOOST_SHA256}" "Boost ${BOOST_VERSION}"

tree="${OUT}/tree"
rm -rf "${tree}"
mkdir -p "${tree}/App/planegcs"
tar xzf "${archive}" -C "${WORK}" \
  "FreeCAD-${FREECAD_TAG}/LICENSE" \
  "FreeCAD-${FREECAD_TAG}/src/Mod/Sketcher/App/planegcs" \
  "FreeCAD-${FREECAD_TAG}/src/boost_graph_adjacency_list.hpp"
cp "${WORK}/FreeCAD-${FREECAD_TAG}/src/Mod/Sketcher/App/planegcs/"* "${tree}/App/planegcs/"
cp "${WORK}/FreeCAD-${FREECAD_TAG}/src/boost_graph_adjacency_list.hpp" "${tree}/"

# FerriteCAD's own glue, copied rather than generated. Written out here as
# heredocs it could not be reviewed, licence-checked or diffed like the rest of
# the repository; as files it can be, and the tree that is handed to somebody
# else builds without needing the rest of the checkout.
mkdir -p "${tree}/Base" "${tree}/glue"
cp "${definition}/glue/SketcherGlobal.h" "${tree}/"
cp "${definition}/glue/FCConfig.h" "${tree}/"
cp "${definition}/glue/Base/Console.h" "${tree}/Base/"
cp "${definition}/glue/provenance.cpp" "${tree}/glue/"

# Into the delivery, and compiled from there. Not into a scratch directory
# followed by a copy: two Eigen trees is one Eigen tree too many, and the
# question a recipient of an MPL-2.0 binary asks is about the source that
# produced it, not about a source that resembles it.
eigen="${OUT}/sources/eigen-${EIGEN_VERSION}"
rm -rf "${OUT}/sources"
mkdir -p "${eigen}"
tar xzf "${eigen_archive}" -C "${eigen}" --strip-components=1

# Boost is headers here and stays in the work directory: the object-code
# exception means the shared library carries no source or notice obligation
# from it, and 180 megabytes of headers in a component artifact would be an
# obligation nobody has. The licence text and the checked digest do travel.
boost="${WORK}/boost"
rm -rf "${boost}"
mkdir -p "${boost}"
tar xzf "${boost_archive}" -C "${boost}" --strip-components=1 \
  "${BOOST_PREFIX}/boost" \
  "${BOOST_PREFIX}/LICENSE_1_0.txt"

if [ ! -d "${eigen}/Eigen" ]; then
  echo "the Eigen archive did not unpack an Eigen/ directory into ${eigen}" >&2
  exit 1
fi
if [ ! -d "${boost}/boost" ]; then
  echo "the Boost archive did not unpack a boost/ directory into ${boost}" >&2
  exit 1
fi
echo "eigen ${eigen}"
echo "boost ${boost}"

# What the library will answer when the lab asks it what it is. Built from the
# pin, so it cannot say 1.0.1 beside a library made from something else.
provenance="planegcs from FreeCAD ${FREECAD_TAG}, archive SHA-256 ${FREECAD_SHA256}"

build="${OUT}/build"
rm -rf "${build}"

# No -G. The platform default is whichever toolchain is installed, which is
# what docs/build-occt.md paid to learn: naming a Visual Studio release here
# breaks on the next one.
cmake -S "$(native "${definition}")" -B "$(native "${build}")" \
  -DCMAKE_BUILD_TYPE=Release \
  -DFCAD_PLANEGCS_TREE="$(native "${tree}")" \
  -DFCAD_EIGEN_INCLUDE="$(native "${eigen}")" \
  -DFCAD_BOOST_INCLUDE="$(native "${boost}")" \
  -DFCAD_PLANEGCS_PROVENANCE="${provenance}"
cmake --build "$(native "${build}")" --config Release

info="${build}/planegcs-build-info-Release.txt"
if [ ! -f "${info}" ]; then
  echo "the build reported nothing about itself; expected ${info}" >&2
  exit 1
fi
read_info() { grep "^$1=" "${info}" | head -1 | cut -d= -f2-; }

built_library="$(read_info library)"
built_linker_file="$(read_info linker_file)"
target_type="$(read_info target_type)"
compiler="$(read_info cxx_compiler_id) $(read_info cxx_compiler_version)"

# Asked of the configured build rather than of the lines above, because those
# lines are what somebody edits to make a platform compile and the include
# directory is where that edit would land. Compared as directories and not as
# text: a Windows shell and cmake spell the same directory differently, and a
# gate that failed on the spelling would be turned off within the week.
same_directory() {
  local a b
  a="$(posix "$1")"
  b="$(posix "$2")"
  [ -d "${a}" ] && [ -d "${b}" ] || return 1
  [ "$(cd "${a}" && pwd -P)" = "$(cd "${b}" && pwd -P)" ]
}
if ! same_directory "$(read_info eigen_include)" "${eigen}"; then
  echo "the library was compiled against Eigen in $(read_info eigen_include), and the source \
delivered beside it is ${eigen}" >&2
  exit 1
fi
if ! same_directory "$(read_info boost_include)" "${boost}"; then
  echo "the library was compiled against Boost in $(read_info boost_include), and the checked \
Boost is ${boost}" >&2
  exit 1
fi

# Refused here as well as in the CMake, because this is the file somebody edits
# when a platform will not link and the licence position is the thing that
# quietly gives way.
if [ "${target_type}" != "SHARED_LIBRARY" ]; then
  echo "planegcs was built as ${target_type}, which the licence position forbids" >&2
  exit 1
fi

cp "${built_library}" "${OUT}/${library_name}"
if [ -n "${import_library_name}" ]; then
  if [ "${built_linker_file}" = "${built_library}" ]; then
    echo "Windows produced no import library, so nothing could relink" >&2
    exit 1
  fi
  cp "${built_linker_file}" "${OUT}/${import_library_name}"
fi

# The licences travel with the library, not as an afterthought at release, and
# each comes out of the archive it belongs to rather than from a copy committed
# here that could disagree with the source that was actually built.
cp "${WORK}/FreeCAD-${FREECAD_TAG}/LICENSE" \
  "${OUT}/LICENSE-FreeCAD-LGPL-2.0-or-later.txt"
cp "${eigen}/COPYING.MPL2" "${OUT}/LICENSE-Eigen-MPL-2.0.txt"
cp "${boost}/LICENSE_1_0.txt" "${OUT}/LICENSE-Boost-BSL-1.0.txt"

cat > "${OUT}/PROVENANCE.txt" <<NOTICE
planegcs from FreeCAD ${FREECAD_TAG}

freecad version   ${FREECAD_TAG}
freecad archive   ${ARCHIVE_URL}
freecad sha256    ${FREECAD_SHA256}  (checked before extraction)
eigen version     ${EIGEN_VERSION}
eigen archive     ${EIGEN_URL}
eigen sha256      ${EIGEN_SHA256}  (checked before extraction)
boost version     ${BOOST_VERSION}
boost archive     ${BOOST_URL}
boost sha256      ${BOOST_SHA256}  (checked before extraction)

platform     ${platform}
compiler     ${compiler}
library      ${library_name}
import       ${import_library_name:-none on this platform}
provenance   ${provenance}
NOTICE

sed -e "s|@TAG@|${FREECAD_TAG}|g" \
    -e "s|@SHA256@|${FREECAD_SHA256}|g" \
    -e "s|@ARCHIVE_URL@|${ARCHIVE_URL}|g" \
    -e "s|@EIGEN_VERSION@|${EIGEN_VERSION}|g" \
    -e "s|@EIGEN_URL@|${EIGEN_URL}|g" \
    -e "s|@EIGEN_SHA256@|${EIGEN_SHA256}|g" \
    -e "s|@BOOST_VERSION@|${BOOST_VERSION}|g" \
    -e "s|@BOOST_URL@|${BOOST_URL}|g" \
    -e "s|@BOOST_SHA256@|${BOOST_SHA256}|g" \
    -e "s|@PLATFORM@|${platform}|g" \
    -e "s|@COMPILER@|${compiler}|g" \
    -e "s|@LIBRARY@|${library_name}|g" \
    -e "s|@IMPORT_LIBRARY@|${import_library_name:-planegcs.lib}|g" \
    -e "s|@PROVENANCE@|${provenance}|g" \
    "${definition}/DELIVERY.md.in" > "${OUT}/REPLACING.md"

# The digits are in the class deliberately. Without them @SHA256@ goes through
# this check untouched, which is the one placeholder whose survival reads as a
# digest rather than as a mistake.
if grep -q '@[A-Z0-9_]*@' "${OUT}/REPLACING.md"; then
  echo "the delivery notice still has unfilled placeholders" >&2
  exit 1
fi

echo "built ${OUT}/${library_name}"
echo "run:  FCAD_PLANEGCS_DIR=$(native "${OUT}") cargo test -p ferritecad-solver-lab --features planegcs -- --nocapture"
