# SPDX-License-Identifier: MIT
#
# The C++ side of the planegcs boundary.
#
# Built as a static library that is linked into the Rust test binary. The
# dynamic linking policy is about planegcs, not about this shim: the shim is
# FerriteCAD's own MIT code and holds no planegcs types, and planegcs stays a
# replaceable shared library behind it.
#
# Static was not the first choice, and the reason is the same one recorded in
# crates/ferritecad-occt-bridge/CMakeLists.txt: a shared shim needs
# __declspec(dllexport) on Windows or the Rust link fails with LNK2019, and
# then its own DLL has to be findable at run time as well. A static shim has
# neither problem, and the only thing left to find beside the executable is
# planegcs itself, which is the file the licence is about.

cmake_minimum_required(VERSION 3.16)
project(ferritecad_planegcs_bridge LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

# The same runtime the library was built against, and the same one Rust's MSVC
# target uses. planegcs classes carry std::vector members across this boundary.
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreadedDLL")

set(FCAD_PLANEGCS_TREE "" CACHE PATH "The planegcs source tree to compile against")
set(FCAD_EIGEN_INCLUDE "" CACHE PATH "Directory containing Eigen/")
set(FCAD_BOOST_INCLUDE "" CACHE PATH "Directory containing boost/")

if(NOT FCAD_PLANEGCS_TREE)
  message(FATAL_ERROR "FCAD_PLANEGCS_TREE must be set")
endif()
if(NOT EXISTS "${FCAD_PLANEGCS_TREE}/App/planegcs/GCS.h")
  message(FATAL_ERROR "${FCAD_PLANEGCS_TREE} does not look like a planegcs tree")
endif()

add_library(ferritecad_planegcs_bridge STATIC planegcs_shim.cpp)

# Note what is not defined here: FCAD_PLANEGCS_BUILDING. Without it the glue's
# SketcherGlobal.h resolves to __declspec(dllimport) on Windows, which is what
# makes this shim call into the DLL rather than expect a copy of it.
target_include_directories(ferritecad_planegcs_bridge PRIVATE
  "${CMAKE_CURRENT_LIST_DIR}"
  "${FCAD_PLANEGCS_TREE}/App/planegcs"
  "${FCAD_PLANEGCS_TREE}"
)
foreach(_headers FCAD_EIGEN_INCLUDE FCAD_BOOST_INCLUDE)
  if(${_headers})
    target_include_directories(ferritecad_planegcs_bridge PRIVATE "${${_headers}}")
  endif()
endforeach()

if(MSVC)
  target_compile_options(ferritecad_planegcs_bridge PRIVATE /bigobj /w /EHsc)
else()
  target_compile_options(ferritecad_planegcs_bridge PRIVATE -w)
endif()

# What the Rust build script needs and cannot discover for itself. The name
# carries the configuration for the same reason the library's does: a
# multi-config generator writes one of these per configuration.
file(GENERATE
  OUTPUT "${CMAKE_BINARY_DIR}/planegcs-bridge-info-$<CONFIG>.txt"
  CONTENT "bridge_dir=$<TARGET_FILE_DIR:ferritecad_planegcs_bridge>
configuration=$<CONFIG>
generator=${CMAKE_GENERATOR}
cxx_compiler_id=${CMAKE_CXX_COMPILER_ID}
cxx_compiler_version=${CMAKE_CXX_COMPILER_VERSION}
")
