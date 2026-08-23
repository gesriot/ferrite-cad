/* SPDX-License-Identifier: MIT
 *
 * FerriteCAD's build glue, not planegcs.
 *
 * planegcs includes "../../SketcherGlobal.h" for one export macro. FreeCAD's
 * own version reaches FCGlobal.h and from there into Qt, none of which a
 * solver needs.
 *
 * The macro is not empty on Windows, and that is the whole Windows story: a
 * class with no __declspec exports nothing from a DLL, the shim fails to link
 * with LNK2019, and the only ways out are exporting everything the compiler
 * emitted or linking planegcs statically. Static linking is refused here on
 * licensing grounds, so this glue says dllexport while the library is being
 * built and dllimport to everyone who uses it, which is what FreeCAD's own
 * header does. Every planegcs class the shim touches already carries the
 * macro upstream, so no LGPL source is edited to make this work.
 */
#pragma once

#if defined(_WIN32)
#if defined(FCAD_PLANEGCS_BUILDING)
#define SketcherExport __declspec(dllexport)
#else
#define SketcherExport __declspec(dllimport)
#endif
#else
#define SketcherExport
#endif
