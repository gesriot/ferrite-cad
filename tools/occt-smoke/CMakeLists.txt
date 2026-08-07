# Standalone OCCT smoke test. Dynamic linkage only (LGPL).
cmake_minimum_required(VERSION 3.16)
project(occt_smoke LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# Modules we exercise: FoundationClasses, ModelingData, ModelingAlgorithms,
# DataExchange (STEP + XDE). The smoke source never calls Visualization APIs.
#
# ApplicationFramework is required for XCAF documents (TKCAF/TKLCAF/…).
# Visualization is listed only because OCCT's own DataExchange/XCAF imported
# targets INTERFACE_LINK TKService/TKV3d — without the component, find_package
# fails on stock installs (Homebrew 7.9, official CMake packages). We still
# do not initialize an interactive viewer.
# Component order matters: OpenCASCADEConfig.cmake includes each
# *Targets.cmake in the order listed, and DataExchange/XCAF IMPORTED
# targets reference ApplicationFramework + Visualization toolkits.
find_package(OpenCASCADE REQUIRED COMPONENTS
  FoundationClasses
  ModelingData
  ModelingAlgorithms
  Visualization
  ApplicationFramework
  DataExchange
)

# Stock OpenCASCADEConfig.cmake sets OpenCASCADE_LIBRARIES to *every* toolkit
# in the install (including Draw). Rebuild the link set from the modules we
# actually requested so we do not pull TKDraw / DETools into the smoke binary.
set(_occt_link_libs "")
foreach(_comp IN ITEMS
    FoundationClasses ModelingData ModelingAlgorithms
    Visualization ApplicationFramework DataExchange)
  if(DEFINED OpenCASCADE_${_comp}_LIBRARIES)
    list(APPEND _occt_link_libs ${OpenCASCADE_${_comp}_LIBRARIES})
  endif()
endforeach()
list(REMOVE_DUPLICATES _occt_link_libs)
if(NOT _occt_link_libs)
  # Older / odd packages: fall back to the global list.
  set(_occt_link_libs ${OpenCASCADE_LIBRARIES})
endif()
if(NOT _occt_link_libs)
  message(FATAL_ERROR
    "No Open CASCADE libraries resolved. Check OpenCASCADE_DIR and that "
    "DataExchange was built as shared libraries.")
endif()

# Refuse static Open CASCADE: FerriteCAD/LGPL policy requires shared libs only.
# Accepting an absent variable would make this check cosmetic on exactly the
# third-party packages we most need to audit.
set(_occt_shared_build FALSE)
if(DEFINED OpenCASCADE_BUILD_SHARED_LIBS AND OpenCASCADE_BUILD_SHARED_LIBS)
  set(_occt_shared_build TRUE)
elseif(DEFINED OpenCASCADE_BUILD_SHARED_LIBS)
  message(FATAL_ERROR
    "Static Open CASCADE is forbidden (OpenCASCADE_BUILD_SHARED_LIBS=OFF). "
    "Rebuild with -DBUILD_LIBRARY_TYPE=Shared.")
endif()
if(DEFINED OpenCASCADE_LIBRARY_TYPE)
  string(TOUPPER "${OpenCASCADE_LIBRARY_TYPE}" _occt_lib_type)
  if(_occt_lib_type STREQUAL "SHARED")
    set(_occt_shared_build TRUE)
  else()
    message(FATAL_ERROR
      "Open CASCADE must declare shared libraries, found "
      "OpenCASCADE_LIBRARY_TYPE=${OpenCASCADE_LIBRARY_TYPE}. Rebuild OCCT with "
      "-DBUILD_LIBRARY_TYPE=Shared and reinstall.")
  endif()
endif()
if(NOT _occt_shared_build)
  message(FATAL_ERROR
    "Cannot verify that Open CASCADE is shared. Its CMake package must set "
    "OpenCASCADE_BUILD_SHARED_LIBS=ON or OpenCASCADE_LIBRARY_TYPE=Shared.")
endif()

# Package component variables are toolkit names in some OCCT distributions.
# On Unix, resolve every name and reject an archive explicitly as a second
# line of defence against a misleading package variable.
if(UNIX)
  foreach(_occt_lib IN LISTS _occt_link_libs)
    if(IS_ABSOLUTE "${_occt_lib}")
      set(_occt_resolved_lib "${_occt_lib}")
    else()
      unset(_occt_resolved_lib CACHE)
      find_library(_occt_resolved_lib NAMES "${_occt_lib}"
        HINTS "${OpenCASCADE_LIBRARY_DIR}" NO_DEFAULT_PATH)
    endif()
    if(NOT _occt_resolved_lib)
      message(FATAL_ERROR "Could not resolve OCCT toolkit ${_occt_lib} to verify linkage.")
    endif()
    if(_occt_resolved_lib MATCHES "\\.a$")
      message(FATAL_ERROR
        "Static OCCT archive resolved for ${_occt_lib}: ${_occt_resolved_lib}. "
        "Rebuild OCCT with -DBUILD_LIBRARY_TYPE=Shared.")
    endif()
  endforeach()
endif()

add_executable(occt_smoke src/main.cpp)

if(OpenCASCADE_INCLUDE_DIR)
  target_include_directories(occt_smoke PRIVATE ${OpenCASCADE_INCLUDE_DIR})
elseif(OpenCASCADE_INCLUDE_DIRS)
  target_include_directories(occt_smoke PRIVATE ${OpenCASCADE_INCLUDE_DIRS})
endif()

if(OpenCASCADE_LIBRARY_DIR)
  target_link_directories(occt_smoke PRIVATE ${OpenCASCADE_LIBRARY_DIR})
endif()

# Toolkit names resolve through OpenCASCADE_LIBRARY_DIR / IMPORTED targets
# created by OpenCASCADE*Targets.cmake (shared .so/.dylib/.dll only).
target_link_libraries(occt_smoke PRIVATE ${_occt_link_libs})

# On Windows, OpenCASCADE_DLL_DIRECTORY may be set by the package config.
if(WIN32 AND DEFINED OpenCASCADE_BINARY_DIR)
  set_target_properties(occt_smoke PROPERTIES
    VS_DEBUGGER_ENVIRONMENT "PATH=${OpenCASCADE_BINARY_DIR};$ENV{PATH}")
endif()

# RPATH so the binary finds shared OCCT next to a vendor install without
# LD_LIBRARY_PATH / DYLD_LIBRARY_PATH.
if(APPLE)
  set_target_properties(occt_smoke PROPERTIES
    BUILD_RPATH "@loader_path;@loader_path/../lib"
    INSTALL_RPATH "@loader_path;@loader_path/../lib")
elseif(UNIX)
  set_target_properties(occt_smoke PROPERTIES
    BUILD_RPATH "$ORIGIN;$ORIGIN/../lib"
    INSTALL_RPATH "$ORIGIN;$ORIGIN/../lib")
endif()

install(TARGETS occt_smoke RUNTIME DESTINATION bin)
install(FILES data/sample_named_colored.step DESTINATION share/occt_smoke)

message(STATUS "OpenCASCADE_DIR         = ${OpenCASCADE_DIR}")
message(STATUS "OpenCASCADE_INCLUDE_DIR = ${OpenCASCADE_INCLUDE_DIR}")
message(STATUS "OpenCASCADE_LIBRARY_DIR = ${OpenCASCADE_LIBRARY_DIR}")
message(STATUS "occt_smoke link libs    = ${_occt_link_libs}")
if(DEFINED OpenCASCADE_BUILD_SHARED_LIBS)
  message(STATUS "OpenCASCADE_BUILD_SHARED_LIBS=${OpenCASCADE_BUILD_SHARED_LIBS}")
endif()
