/* SPDX-License-Identifier: MIT
 *
 * A flat C boundary onto FreeCAD's planegcs, so the bench can ask it the same
 * questions it asks every other candidate.
 *
 * planegcs is LGPL-2.0-or-later and is linked dynamically, exactly as Open
 * CASCADE is: the shared library beside this shim can be replaced by the user
 * with their own build. See THIRD_PARTY_LICENSES.md. This shim is FerriteCAD's
 * own MIT code and holds no planegcs types.
 */
#ifndef FERRITECAD_PLANEGCS_SHIM_H
#define FERRITECAD_PLANEGCS_SHIM_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Constraint kinds, matching the bench's own enum. Stable by number: these
 * cross an ABI. */
enum {
  FC_GCS_COINCIDENT = 0,
  FC_GCS_FIXED = 1,
  FC_GCS_DISTANCE = 2,
  FC_GCS_HORIZONTAL = 3,
  FC_GCS_VERTICAL = 4,
  FC_GCS_EQUAL_LENGTH = 5,
  FC_GCS_PERPENDICULAR = 6,
  FC_GCS_PARALLEL = 7
};

/* One constraint, fixed width so the layout is trivially stable.
 * `points` holds up to four point indices; unused entries are -1.
 * `value` carries a distance, or the x of a Fixed; `value2` its y. */
typedef struct FcGcsConstraint {
  int32_t kind;
  int32_t points[4];
  double value;
  double value2;
} FcGcsConstraint;

/* Solves, and reports what planegcs made of the system.
 *
 * `state` is 2 doubles per point, read as the starting guess and written with
 * the solution. Returns 0 on success and non-zero when planegcs did not
 * converge; the diagnosis out-parameters are filled either way when they can
 * be.
 */
int32_t fc_gcs_solve(double *state, size_t point_count,
                     const FcGcsConstraint *constraints,
                     size_t constraint_count, int32_t *out_dofs,
                     int32_t *out_has_conflicting, int32_t *out_has_redundant,
                     int32_t *out_iterations);

/* The planegcs version this shim was built against, for the record. */
const char *fc_gcs_provenance(void);

#ifdef __cplusplus
}
#endif

#endif
