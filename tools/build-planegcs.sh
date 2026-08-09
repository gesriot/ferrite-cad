#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Builds FreeCAD's planegcs as a shared library for the solver bench.
#
# planegcs is LGPL-2.0-or-later, so it is linked dynamically and the library
# this produces can be replaced by the user with their own build. Nothing of
# it is compiled into a FerriteCAD binary. See THIRD_PARTY_LICENSES.md.
#
# The LGPL sources are used byte-identical. The only files added beside them
# are FerriteCAD's own build glue, marked as such.
#
#   tools/build-planegcs.sh [output-directory]
#
# Then: FCAD_PLANEGCS_DIR=<output> cargo test -p ferritecad-solver-lab --features planegcs

set -euo pipefail

host_os="$(uname -s)"
case "${host_os}" in
  Darwin|Linux) ;;
  *)
    echo "this helper currently supports macOS and Linux only" >&2
    exit 1 ;;
esac

FREECAD_TAG="1.0.1"
FREECAD_SHA256="f62bc07c477544eff62b6ab0fc3bb63fa7f1e6f94763c51b0049507842d444f3"

OUT="${1:-$(pwd)/vendor/planegcs}"
WORK="${OUT}/work"
mkdir -p "${WORK}"

archive="${WORK}/freecad-${FREECAD_TAG}.tar.gz"
if [ ! -f "${archive}" ]; then
  echo "fetching FreeCAD ${FREECAD_TAG}"
  curl -sSL -o "${archive}" \
    "https://github.com/FreeCAD/FreeCAD/archive/refs/tags/${FREECAD_TAG}.tar.gz"
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

# FerriteCAD's own glue. planegcs includes "../../SketcherGlobal.h" for one
# export macro; FreeCAD's own version reaches FCGlobal.h and from there into
# Qt, none of which a solver needs.
cat > "${tree}/SketcherGlobal.h" <<'GLUE'
// SPDX-License-Identifier: MIT
#pragma once
#define SketcherExport
GLUE

# FreeCAD's FCConfig.h is a build-system product full of Qt and platform
# probes. planegcs needs none of it, only for the include to resolve.
cat > "${tree}/FCConfig.h" <<'GLUE'
// SPDX-License-Identifier: MIT
#pragma once
GLUE

mkdir -p "${tree}/Base"
cat > "${tree}/Base/Console.h" <<'GLUE'
// SPDX-License-Identifier: MIT
// Enough of FreeCAD's console for planegcs to build outside FreeCAD. It uses
// two printf-style calls; both are silent here, because writing to a terminal
// inside a timed region would measure the terminal.
#pragma once
namespace Base {
class ConsoleSingleton {
public:
    void Log(const char*, ...) {}
    void Warning(const char*, ...) {}
};
inline ConsoleSingleton& Console() {
    static ConsoleSingleton instance;
    return instance;
}
}  // namespace Base
GLUE

eigen="${FCAD_EIGEN_INCLUDE:-}"
if [ -z "${eigen}" ]; then
  for candidate in /opt/homebrew/include/eigen3 /usr/local/include/eigen3 /usr/include/eigen3; do
    [ -d "${candidate}/Eigen" ] && eigen="${candidate}" && break
  done
fi
boost="${FCAD_BOOST_INCLUDE:-}"
if [ -z "${boost}" ]; then
  for candidate in /opt/homebrew/include /usr/local/include /usr/include; do
    [ -d "${candidate}/boost" ] && boost="${candidate}" && break
  done
fi
if [ ! -d "${eigen}/Eigen" ]; then
  echo "Eigen headers not found; set FCAD_EIGEN_INCLUDE" >&2
  exit 1
fi
if [ -z "${boost}" ]; then
  echo "Boost headers not found; set FCAD_BOOST_INCLUDE" >&2
  exit 1
fi
echo "eigen ${eigen}"
echo "boost ${boost}"

build="${OUT}/build"
rm -rf "${build}"
mkdir -p "${build}"
for source in "${tree}/App/planegcs/"*.cpp; do
  echo "  compiling $(basename "${source}")"
  "${CXX:-c++}" -std=c++17 -O2 -fPIC -w -c "${source}" \
    -I"${tree}/App/planegcs" -I"${tree}" -I"${eigen}" -I"${boost}" \
    -o "${build}/$(basename "${source%.cpp}").o"
done

case "${host_os}" in
  Darwin)
    "${CXX:-c++}" -dynamiclib -o "${OUT}/libplanegcs.dylib" "${build}"/*.o \
      -install_name "@rpath/libplanegcs.dylib"
    library="${OUT}/libplanegcs.dylib" ;;
  Linux)
    "${CXX:-c++}" -shared -o "${OUT}/libplanegcs.so" "${build}"/*.o
    library="${OUT}/libplanegcs.so" ;;
esac

# The licence travels with the library, not as an afterthought at release.
cp "${WORK}/FreeCAD-${FREECAD_TAG}/LICENSE" \
  "${OUT}/LICENSE-FreeCAD-LGPL-2.0-or-later.txt"
cat > "${OUT}/README.txt" <<NOTICE
This directory contains a build of planegcs from FreeCAD ${FREECAD_TAG}
(https://github.com/FreeCAD/FreeCAD), archive SHA-256 ${FREECAD_SHA256}.

planegcs is licensed under the GNU Library General Public License, version 2
or (at your option) any later version. It is built here as a shared library so
it can be replaced: rebuild it from the sources in ./tree and put the result
in its place. The complete licence text is beside this notice in
LICENSE-FreeCAD-LGPL-2.0-or-later.txt.

The sources under ./tree/App/planegcs are byte-identical to the release. The
files ./tree/SketcherGlobal.h, ./tree/FCConfig.h and ./tree/Base/Console.h are
FerriteCAD's own build glue and are MIT.
NOTICE

echo "built ${library}"
echo "run:  FCAD_PLANEGCS_DIR=${OUT} cargo test -p ferritecad-solver-lab --features planegcs -- --nocapture"
