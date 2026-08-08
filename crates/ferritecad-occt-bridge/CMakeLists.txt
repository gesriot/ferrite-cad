# SPDX-License-Identifier: MIT
#
# The C++ side of the Open CASCADE adapter.
#
# Built as a shared library that links OCCT itself, so the Rust crate links one
# name and CMake keeps the toolkit list. Open CASCADE is linked dynamically and
# only dynamically; see docs/build-occt.md for why that is policy rather than
# preference.

cmake_minimum_required(VERSION 3.16)
project(ferritecad_occt_bridge LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

# Only what the adapter calls. Visualization, DataExchange and the OCAF
# document toolkits are not needed here: this slice builds solids, it does not
# read STEP or open a viewer.
find_package(OpenCASCADE REQUIRED COMPONENTS
  FoundationClasses
  ModelingData
  ModelingAlgorithms
)

# Stock OpenCASCADEConfig.cmake sets OpenCASCADE_LIBRARIES to every toolkit in
# the install. Rebuild the link set from the modules actually requested, the
# same way tools/occt-smoke does.
set(_occt_link_libs "")
foreach(_comp IN ITEMS FoundationClasses ModelingData ModelingAlgorithms)
  if(DEFINED OpenCASCADE_${_comp}_LIBRARIES)
    list(APPEND _occt_link_libs ${OpenCASCADE_${_comp}_LIBRARIES})
  endif()
endforeach()
list(REMOVE_DUPLICATES _occt_link_libs)
if(NOT _occt_link_libs)
  set(_occt_link_libs ${OpenCASCADE_LIBRARIES})
endif()
if(NOT _occt_link_libs)
  message(FATAL_ERROR
    "No Open CASCADE libraries resolved. Check OpenCASCADE_DIR.")
endif()

# Refuse a static Open CASCADE. A static build changes the licensing,
# packaging and testing path entirely, and must not enter a release by way of
# a local experiment.
if(DEFINED OpenCASCADE_BUILD_SHARED_LIBS AND NOT OpenCASCADE_BUILD_SHARED_LIBS)
  message(FATAL_ERROR
    "Static Open CASCADE is forbidden: FerriteCAD links it dynamically only.")
endif()

add_library(ferritecad_occt_bridge SHARED src/bridge.cpp)

target_include_directories(ferritecad_occt_bridge
  PUBLIC include
  PRIVATE ${OpenCASCADE_INCLUDE_DIR})
target_link_directories(ferritecad_occt_bridge PRIVATE ${OpenCASCADE_LIBRARY_DIR})
target_link_libraries(ferritecad_occt_bridge PRIVATE ${_occt_link_libs})

if(APPLE)
  # So the Rust test binary can find the bridge through its own RPATH rather
  # than through the environment, which SIP strips.
  set_target_properties(ferritecad_occt_bridge PROPERTIES
    INSTALL_NAME_DIR "@rpath"
    BUILD_WITH_INSTALL_NAME_DIR ON)
endif()
set_target_properties(ferritecad_occt_bridge PROPERTIES
  BUILD_RPATH "${OpenCASCADE_LIBRARY_DIR}"
  INSTALL_RPATH "${OpenCASCADE_LIBRARY_DIR}")

# What the Rust build script needs and cannot discover for itself.
#
# The name carries the configuration because a multi-config generator evaluates
# these expressions once per configuration, and one file cannot hold two
# answers.
file(GENERATE
  OUTPUT "${CMAKE_BINARY_DIR}/ferritecad-bridge-info-$<CONFIG>.txt"
  CONTENT "occt_library_dir=${OpenCASCADE_LIBRARY_DIR}\nocct_version=${OpenCASCADE_VERSION}\nbridge_dir=$<TARGET_FILE_DIR:ferritecad_occt_bridge>\n")
