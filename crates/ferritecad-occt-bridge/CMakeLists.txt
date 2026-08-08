# SPDX-License-Identifier: MIT
#
# The C++ side of the Open CASCADE adapter.
#
# Built as a static library that is linked into the Rust binary. The dynamic
# linking policy is about Open CASCADE, not about this shim: the shim is
# FerriteCAD's own MIT code, and OCCT stays dynamic behind it. See
# docs/build-occt.md.
#
# Static was not the first choice. A shared shim needs __declspec(dllexport) on
# Windows or it exports nothing and the Rust link fails with LNK2019, and then
# its DLL has to be findable at run time on a platform with no RPATH. A static
# shim has neither problem on any platform, and the only thing left to ship
# beside the executable is Open CASCADE itself.

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

add_library(ferritecad_occt_bridge STATIC src/bridge.cpp)

target_include_directories(ferritecad_occt_bridge
  PUBLIC include
  PRIVATE ${OpenCASCADE_INCLUDE_DIR})
target_link_directories(ferritecad_occt_bridge PRIVATE ${OpenCASCADE_LIBRARY_DIR})
target_link_libraries(ferritecad_occt_bridge PRIVATE ${_occt_link_libs})

# What the Rust build script needs and cannot discover for itself.
#
# The name carries the configuration because a multi-config generator evaluates
# these expressions once per configuration, and one file cannot hold two
# answers.
#
# `occt_libraries` is the toolkit list resolved above, so the Rust side never
# has to guess which toolkits this build of Open CASCADE actually provides.
string(REPLACE ";" "," _occt_libs_csv "${_occt_link_libs}")
file(GENERATE
  OUTPUT "${CMAKE_BINARY_DIR}/ferritecad-bridge-info-$<CONFIG>.txt"
  CONTENT "occt_library_dir=${OpenCASCADE_LIBRARY_DIR}\nocct_version=${OpenCASCADE_VERSION}\nocct_libraries=${_occt_libs_csv}\nbridge_dir=$<TARGET_FILE_DIR:ferritecad_occt_bridge>\n")
