# SPDX-License-Identifier: MIT
#
# Generates the synthetic STEP conformance corpus.
#
# A tool, not part of the product. The adapter does not read STEP yet, and
# nothing here is linked into a FerriteCAD binary; this exists so that when
# §18 is built there is a corpus with recorded provenance to build it against,
# rather than one assembled afterwards to fit whatever the reader happened to
# do.

cmake_minimum_required(VERSION 3.16)
project(ferritecad_step_corpus LANGUAGES CXX)

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

add_executable(step_corpus src/main.cpp)
target_include_directories(step_corpus PRIVATE ${OpenCASCADE_INCLUDE_DIR})
target_link_directories(step_corpus PRIVATE ${OpenCASCADE_LIBRARY_DIR})
target_link_libraries(step_corpus PRIVATE ${_occt_link_libs})

if(APPLE)
  set_target_properties(step_corpus PROPERTIES
    BUILD_RPATH "@loader_path;@loader_path/../lib"
    INSTALL_RPATH "@loader_path;@loader_path/../lib")
elseif(UNIX)
  set_target_properties(step_corpus PROPERTIES
    BUILD_RPATH "$ORIGIN;$ORIGIN/../lib"
    INSTALL_RPATH "$ORIGIN;$ORIGIN/../lib")
endif()
