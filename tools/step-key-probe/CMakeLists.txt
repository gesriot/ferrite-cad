# SPDX-License-Identifier: MIT
#
# Asks whether a definition in a STEP file has an identity that survives being
# read again.
#
# A tool, not part of the product, and deliberately built before anything in
# the product changes. Durable selection into an imported assembly needs a key
# that comes from the file rather than from the reader, and whether Open
# CASCADE can supply one is a measurement, not a design decision. This reports
# the candidates and their properties on the committed corpus; what to do about
# them is decided afterwards, on the evidence.

cmake_minimum_required(VERSION 3.16)
project(ferritecad_step_key_probe LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# The same component set the smoke tool asks for. DataExchange and
# ApplicationFramework are what STEP and XDE actually need; Visualization is
# here because Open CASCADE's imported targets pull it in transitively and
# omitting it breaks the link on some installs.
find_package(OpenCASCADE REQUIRED COMPONENTS
  FoundationClasses
  ModelingData
  ModelingAlgorithms
  Visualization
  ApplicationFramework
  DataExchange
)

set(_occt_link_libs "")
foreach(_comp IN ITEMS FoundationClasses ModelingData ModelingAlgorithms
                       Visualization ApplicationFramework DataExchange)
  if(DEFINED OpenCASCADE_${_comp}_LIBRARIES)
    list(APPEND _occt_link_libs ${OpenCASCADE_${_comp}_LIBRARIES})
  endif()
endforeach()
list(REMOVE_DUPLICATES _occt_link_libs)
if(NOT _occt_link_libs)
  set(_occt_link_libs ${OpenCASCADE_LIBRARIES})
endif()

add_executable(step_key_probe src/main.cpp)
# The bridge's own identity route, included rather than copied: this tool
# exists to measure what the importer does, and a private copy would measure
# itself. See crates/ferritecad-occt-bridge/src/step_identity.hpp.
target_include_directories(step_key_probe PRIVATE
  ${OpenCASCADE_INCLUDE_DIR}
  ${CMAKE_CURRENT_SOURCE_DIR}/../../crates/ferritecad-occt-bridge/src)
target_link_directories(step_key_probe PRIVATE ${OpenCASCADE_LIBRARY_DIR})
target_link_libraries(step_key_probe PRIVATE ${_occt_link_libs})

if(APPLE)
  set_target_properties(step_key_probe PROPERTIES
    BUILD_RPATH "@loader_path;@loader_path/../lib"
    INSTALL_RPATH "@loader_path;@loader_path/../lib")
elseif(UNIX)
  set_target_properties(step_key_probe PROPERTIES
    BUILD_RPATH "$ORIGIN;$ORIGIN/../lib"
    INSTALL_RPATH "$ORIGIN;$ORIGIN/../lib")
endif()
