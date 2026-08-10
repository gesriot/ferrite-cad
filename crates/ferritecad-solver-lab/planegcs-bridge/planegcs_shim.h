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
#define FC_GCS_NOEXCEPT noexcept
#else
#define FC_GCS_NOEXCEPT
#endif

#ifdef __cplusplus
#define FC_GCS_NOEXCEPT noexcept
extern "C" {
#else
#define FC_GCS_NOEXCEPT
#endif

/* Return statuses. Only SUCCESS and CONVERGED contain an applied solution. */
enum {
  FC_GCS_SUCCESS = 0,
  FC_GCS_NOT_CONVERGED = 1,
  FC_GCS_CONVERGED = 2,
  FC_GCS_INVALID_INPUT = -1,
  FC_GCS_UNKNOWN_CONSTRAINT = -2,
  FC_GCS_STD_EXCEPTION = -3,
  FC_GCS_UNKNOWN_EXCEPTION = -4
};

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
 * the solution. Returns FC_GCS_SUCCESS or FC_GCS_CONVERGED only when an
 * acceptable native solution was applied; all other statuses are failures.
 * The diagnosis out-parameters are filled when they can be.
 */
int32_t fc_gcs_solve(double *state, size_t point_count,
                     const FcGcsConstraint *constraints,
                     size_t constraint_count, int32_t *out_dofs,
                     int32_t *out_has_conflicting, int32_t *out_has_redundant,
                     int32_t *out_iterations) FC_GCS_NOEXCEPT;

/* A system built once and solved many times.
 *
 * Dragging is not a sequence of unrelated solves: the sketch is set up once
 * and then nudged, which is both how a person uses it and the only fair way to
 * compare candidates. Building the system inside every timed solve measured
 * planegcs's setup — which includes a mandatory diagnosis — against a solve
 * that had none.
 */
typedef struct FcGcsSession FcGcsSession;

/* Builds the system. Returns NULL if it could not be built. */
FcGcsSession *fc_gcs_session_create(const double *start, size_t point_count,
                                    const FcGcsConstraint *constraints,
                                    size_t constraint_count) FC_GCS_NOEXCEPT;

void fc_gcs_session_destroy(FcGcsSession *session) FC_GCS_NOEXCEPT;

/* What planegcs makes of the system, and which constraints it blames.
 *
 * `out_blamed` receives the indices of conflicting constraints, then of
 * redundant ones, up to `capacity`; `out_blamed_count` is how many there were
 * in total, which may exceed the capacity. Indices are into the array the
 * session was built from, so a caller can name the constraint a person wrote.
 */
int32_t fc_gcs_session_diagnose(FcGcsSession *session, int32_t *out_dofs,
                                int32_t *out_conflicting_count,
                                int32_t *out_redundant_count,
                                int32_t *out_blamed, size_t capacity,
                                size_t *out_blamed_count) FC_GCS_NOEXCEPT;

/* Moves the target of a Fixed constraint, which is what dragging is. */
int32_t fc_gcs_session_move(FcGcsSession *session, size_t constraint_index,
                            double x, double y) FC_GCS_NOEXCEPT;

/* Solves from wherever the system currently is. */
int32_t fc_gcs_session_solve(FcGcsSession *session) FC_GCS_NOEXCEPT;

/* Copies the current point positions out. */
int32_t fc_gcs_session_state(const FcGcsSession *session, double *out,
                             size_t count) FC_GCS_NOEXCEPT;

/* The planegcs version this shim was built against, for the record. */
const char *fc_gcs_provenance(void) FC_GCS_NOEXCEPT;

#ifdef __cplusplus
}
#endif

#undef FC_GCS_NOEXCEPT

#endif
