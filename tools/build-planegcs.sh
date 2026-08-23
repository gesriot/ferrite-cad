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
ARCHIVE_URL="https://github.com/FreeCAD/FreeCAD/archive/refs/tags/${FREECAD_TAG}.tar.gz"

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

archive="${WORK}/freecad-${FREECAD_TAG}.tar.gz"
if [ ! -f "${archive}" ]; then
  echo "fetching FreeCAD ${FREECAD_TAG}"
  curl -sSL -o "${archive}" "${ARCHIVE_URL}"
fi

# Verified before anything is extracted, let alone compiled.
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "${archive}" | cut -d' ' -f1)"
else
  actual="$(shasum -a 256 "${archive}" | cut -d' ' -f1)"
fi
if [ "${actual}" != "${FREECAD_SHA256}" ]; then
  echo "checksum mismatch for ${archive}" >&2
  echo "  expected ${FREECAD_SHA256}" >&2
  echo "  actual   ${actual}" >&2
  exit 1
fi
echo "checksum ok"

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

eigen="$([ -n "${FCAD_EIGEN_INCLUDE:-}" ] && posix "${FCAD_EIGEN_INCLUDE}" || true)"
if [ -z "${eigen}" ]; then
  for candidate in /opt/homebrew/include/eigen3 /usr/local/include/eigen3 /usr/include/eigen3; do
    [ -d "${candidate}/Eigen" ] && eigen="${candidate}" && break
  done
fi
boost="$([ -n "${FCAD_BOOST_INCLUDE:-}" ] && posix "${FCAD_BOOST_INCLUDE}" || true)"
if [ -z "${boost}" ]; then
  for candidate in /opt/homebrew/include /usr/local/include /usr/include; do
    [ -d "${candidate}/boost" ] && boost="${candidate}" && break
  done
fi
if [ ! -d "${eigen}/Eigen" ]; then
  echo "Eigen headers not found; set FCAD_EIGEN_INCLUDE" >&2
  exit 1
fi
if [ ! -d "${boost}/boost" ]; then
  echo "Boost headers not found; set FCAD_BOOST_INCLUDE" >&2
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

# The licence travels with the library, not as an afterthought at release.
cp "${WORK}/FreeCAD-${FREECAD_TAG}/LICENSE" \
  "${OUT}/LICENSE-FreeCAD-LGPL-2.0-or-later.txt"

cat > "${OUT}/PROVENANCE.txt" <<NOTICE
planegcs from FreeCAD ${FREECAD_TAG}
archive      ${ARCHIVE_URL}
sha256       ${FREECAD_SHA256}  (checked before extraction)
platform     ${platform}
compiler     ${compiler}
library      ${library_name}
import       ${import_library_name:-none on this platform}
provenance   ${provenance}
NOTICE

sed -e "s|@TAG@|${FREECAD_TAG}|g" \
    -e "s|@SHA256@|${FREECAD_SHA256}|g" \
    -e "s|@ARCHIVE_URL@|${ARCHIVE_URL}|g" \
    -e "s|@PLATFORM@|${platform}|g" \
    -e "s|@COMPILER@|${compiler}|g" \
    -e "s|@LIBRARY@|${library_name}|g" \
    -e "s|@IMPORT_LIBRARY@|${import_library_name:-planegcs.lib}|g" \
    -e "s|@PROVENANCE@|${provenance}|g" \
    "${definition}/DELIVERY.md.in" > "${OUT}/REPLACING.md"

if grep -q '@[A-Z_]*@' "${OUT}/REPLACING.md"; then
  echo "the delivery notice still has unfilled placeholders" >&2
  exit 1
fi

echo "built ${OUT}/${library_name}"
echo "run:  FCAD_PLANEGCS_DIR=$(native "${OUT}") cargo test -p ferritecad-solver-lab --features planegcs -- --nocapture"
