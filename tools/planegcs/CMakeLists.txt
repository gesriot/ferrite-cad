# SPDX-License-Identifier: MIT
#
# Builds FreeCAD's planegcs as a replaceable shared library.
#
# One build definition for Linux, macOS and Windows, because three of them
# would be three chances for a platform to be quietly different. It is driven
# by tools/build-planegcs.sh, which fetches the pinned release, checks its
# digest before extracting anything, and lays the tree out where this expects
# it. See docs/build-planegcs.md for the whole statement: which files are
# planegcs, which are FerriteCAD's, and how to replace the result.
#
# planegcs is LGPL-2.0-or-later. It is built shared and nothing else links it
# statically, so whoever receives a build can replace it with their own.

cmake_minimum_required(VERSION 3.16)
project(ferritecad_planegcs LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

# Both sides of the boundary must agree about the C runtime: planegcs classes
# carry std::vector members across it, and Rust's MSVC target uses the dynamic
# CRT. Left to defaults this happens to be right, which is not the same as
# being decided.
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreadedDLL")

set(FCAD_PLANEGCS_TREE "" CACHE PATH
  "The extracted source tree: App/planegcs beside FerriteCAD's glue headers")
set(FCAD_EIGEN_INCLUDE "" CACHE PATH "Directory containing Eigen/")
set(FCAD_BOOST_INCLUDE "" CACHE PATH "Directory containing boost/")
set(FCAD_PLANEGCS_PROVENANCE "" CACHE STRING
  "What the library answers when asked which planegcs it is")

foreach(_required FCAD_PLANEGCS_TREE FCAD_EIGEN_INCLUDE FCAD_BOOST_INCLUDE
                  FCAD_PLANEGCS_PROVENANCE)
  if(NOT ${_required})
    message(FATAL_ERROR "${_required} must be set")
  endif()
endforeach()

# planegcs uses Eigen's dense and sparse solvers and Boost's graph and math
# headers. Both are header-only for everything used here, which is why neither
# is a link dependency and neither adds an obligation beyond attribution.
if(NOT IS_DIRECTORY "${FCAD_EIGEN_INCLUDE}/Eigen")
  message(FATAL_ERROR "no Eigen/ under ${FCAD_EIGEN_INCLUDE}")
endif()
if(NOT IS_DIRECTORY "${FCAD_BOOST_INCLUDE}/boost")
  message(FATAL_ERROR "no boost/ under ${FCAD_BOOST_INCLUDE}")
endif()

# Named rather than globbed. This list is the answer to "which source files
# are planegcs", and a glob answers "whatever was in the directory", which is
# not a statement anybody can review.
set(_planegcs_sources
  App/planegcs/Constraints.cpp
  App/planegcs/GCS.cpp
  App/planegcs/Geo.cpp
  App/planegcs/qp_eq.cpp
  App/planegcs/SubSystem.cpp
)
set(_planegcs_lgpl "")
foreach(_source IN LISTS _planegcs_sources)
  if(NOT EXISTS "${FCAD_PLANEGCS_TREE}/${_source}")
    message(FATAL_ERROR "${FCAD_PLANEGCS_TREE}/${_source} is missing")
  endif()
  list(APPEND _planegcs_lgpl "${FCAD_PLANEGCS_TREE}/${_source}")
endforeach()

# SHARED, and checked below rather than trusted. A static planegcs would put
# LGPL object code inside whatever linked it and take on obligations this
# project has decided not to take on.
# FerriteCAD's own glue is taken from the tree, not from beside this file, so
# a tree handed to somebody else builds without needing the rest of the
# repository. tools/build-planegcs.sh is what puts it there.
if(NOT EXISTS "${FCAD_PLANEGCS_TREE}/glue/provenance.cpp")
  message(FATAL_ERROR "${FCAD_PLANEGCS_TREE}/glue/provenance.cpp is missing")
endif()

add_library(planegcs SHARED
  ${_planegcs_lgpl}
  "${FCAD_PLANEGCS_TREE}/glue/provenance.cpp"
)

get_target_property(_planegcs_type planegcs TYPE)
if(NOT _planegcs_type STREQUAL "SHARED_LIBRARY")
  message(FATAL_ERROR
    "planegcs must be a shared library, this build declares ${_planegcs_type}. "
    "See THIRD_PARTY_LICENSES.md.")
endif()

target_include_directories(planegcs PRIVATE
  "${FCAD_PLANEGCS_TREE}/App/planegcs"
  "${FCAD_PLANEGCS_TREE}"
  "${FCAD_EIGEN_INCLUDE}"
  "${FCAD_BOOST_INCLUDE}"
)

# Turns FerriteCAD's SketcherGlobal.h from dllimport into dllexport, and is
# what makes the Windows DLL export anything at all. PRIVATE: the shim
# compiles the same headers without it and gets dllimport, which is the point.
target_compile_definitions(planegcs PRIVATE
  FCAD_PLANEGCS_BUILDING
  "FCAD_PLANEGCS_PROVENANCE=\"${FCAD_PLANEGCS_PROVENANCE}\""
)

if(MSVC)
  # GCS.cpp is a quarter of a megabyte of source over Eigen templates and
  # overruns the default section limit. /w rather than a warning selection:
  # these are somebody else's sources, used unmodified, and a warning nobody
  # is allowed to act on is noise.
  target_compile_options(planegcs PRIVATE /bigobj /w /EHsc)
else()
  target_compile_options(planegcs PRIVATE -w)
endif()

set_target_properties(planegcs PROPERTIES
  OUTPUT_NAME planegcs
  # So a build can be moved: the loader is told to look beside whoever links
  # it rather than at the absolute path of this build directory.
  INSTALL_NAME_DIR "@rpath"
  MACOSX_RPATH ON
  # One directory holds the DLL and its import library, so the driver script
  # and the Rust build script have one answer to give the linker.
  RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/out"
  LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/out"
  ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/out"
)

# What the driver script needs and cannot work out for itself: where the files
# landed under this generator, and which compiler actually produced them.
# $<TARGET_LINKER_FILE> is the import library on Windows and the shared object
# itself everywhere else, which is exactly the distinction the Rust link needs.
#
# The name carries the configuration because a multi-config generator, which is
# what Windows gets by default, evaluates these expressions once per
# configuration and one file cannot hold two answers.
file(GENERATE
  OUTPUT "${CMAKE_BINARY_DIR}/planegcs-build-info-$<CONFIG>.txt"
  CONTENT "library=$<TARGET_FILE:planegcs>
linker_file=$<TARGET_LINKER_FILE:planegcs>
target_type=${_planegcs_type}
generator=${CMAKE_GENERATOR}
cxx_compiler_id=${CMAKE_CXX_COMPILER_ID}
cxx_compiler_version=${CMAKE_CXX_COMPILER_VERSION}
provenance=${FCAD_PLANEGCS_PROVENANCE}
")
