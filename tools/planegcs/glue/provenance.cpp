// SPDX-License-Identifier: MIT
//
// FerriteCAD's build glue, not planegcs.
//
// The one thing the shared library says about itself. It is compiled into
// libplanegcs rather than into the shim so that the answer comes from the
// library that was actually loaded: a shim-side string would keep saying
// "FreeCAD 1.0.1" beside a library built from anything at all, which is
// exactly the substitution the packaging gate has to be able to catch.
//
// The text is injected by tools/planegcs/CMakeLists.txt from
// tools/planegcs/pin.env, so it cannot drift from the digest that was checked
// before the sources were extracted.

#if defined(_WIN32)
#define FCAD_PLANEGCS_API __declspec(dllexport)
#else
#define FCAD_PLANEGCS_API
#endif

#if !defined(FCAD_PLANEGCS_PROVENANCE)
#error "FCAD_PLANEGCS_PROVENANCE must be defined by the build"
#endif

extern "C" FCAD_PLANEGCS_API const char *fc_planegcs_provenance(void) {
  return FCAD_PLANEGCS_PROVENANCE;
}
