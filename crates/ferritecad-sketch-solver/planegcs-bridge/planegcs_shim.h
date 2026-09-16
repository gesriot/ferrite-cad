/* SPDX-License-Identifier: MIT
 *
 * A flat C boundary onto FreeCAD's planegcs, so the bench can ask it the same
 * questions it asks every other candidate.
 *
 * planegcs is LGPL-2.0-or-later and is linked dynamically: the shared library
 * beside this shim can be replaced by the user with their own build. Its terms
 * are recorded separately from Open CASCADE in THIRD_PARTY_LICENSES.md. This
 * shim is FerriteCAD's own MIT code and holds no planegcs types.
 */
#ifndef FERRITECAD_PLANEGCS_SHIM_H
#define FERRITECAD_PLANEGCS_SHIM_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
#define FC_GCS_NOEXCEPT noexcept
extern "C" {
#else
#define FC_GCS_NOEXCEPT
#endif

/* Return statuses. Only SUCCESS and CONVERGED contain an applied solution.
 *
 * These are this shim's numbers, not planegcs's, and they do not reach a
 * caller of ferritecad-sketch-solver: the Rust side turns them into named
 * situations, because a status number is a fact about a library version
 * rather than about a sketch. */
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
  FC_GCS_PARALLEL = 7,
  /* A circle has the radius in `value`. It names its circle in `circles[0]`
   * and no point at all: a circle's centre is already one of the sketch's
   * points and is constrained by naming that point, not by naming the circle
   * a second way. */
  FC_GCS_CIRCLE_RADIUS = 8
};

/* One circle of the sketch: a centre that is one of the sketch's points, and a
 * radius of its own.
 *
 * Three unknowns, and the radius is one of them. It is declared here rather
 * than derived from a second point because it is a scalar with no position: a
 * circle described by a centre and a rim point would carry four parameters for
 * three degrees of freedom, and would report a degree of freedom the drawing
 * does not have.
 *
 * `center` indexes the point array the session was built from. `radius` is the
 * starting guess and must be a positive finite length. */
typedef struct FcGcsCircle {
  int32_t center;
  double radius;
} FcGcsCircle;

/* One constraint, fixed width so the layout is trivially stable.
 *
 * `points` holds up to four point indices and `circles` up to two circle
 * indices; unused entries of either are -1. The two arrays are separate and
 * never stand in for one another: an index into one array read as an index
 * into the other would name real geometry of the wrong kind, and the system
 * would build and solve and be wrong. Every kind uses one array or the other
 * and the shim checks that the array it does not use is empty.
 *
 * `value` carries a distance, a radius, or the x of a Fixed; `value2` its y. */
typedef struct FcGcsConstraint {
  int32_t kind;
  int32_t points[4];
  int32_t circles[2];
  double value;
  double value2;
} FcGcsConstraint;

/* A system built once and solved many times.
 *
 * Dragging is not a sequence of unrelated solves: the sketch is set up once
 * and then nudged, which is both how a person uses it and the only fair way to
 * compare candidates. Building the system inside every timed solve measured
 * planegcs's setup — which includes a mandatory diagnosis — against a solve
 * that had none.
 */
typedef struct FcGcsSession FcGcsSession;

/* Builds the system. Returns NULL if it could not be built.
 *
 * `start` holds two doubles per point, x then y. `circles` holds one entry per
 * circle, each naming a point of `start` as its centre; the circles are their
 * own array rather than more entries in `start`, because a radius is not a
 * coordinate and packing it beside one would make the parameter block
 * unreadable without a rule about which slots meant what.
 *
 * The declared unknowns are every point coordinate followed by every radius,
 * so a free circle has three of them. */
FcGcsSession *fc_gcs_session_create(const double *start, size_t point_count,
                                    const FcGcsCircle *circles,
                                    size_t circle_count,
                                    const FcGcsConstraint *constraints,
                                    size_t constraint_count) FC_GCS_NOEXCEPT;

void fc_gcs_session_destroy(FcGcsSession *session) FC_GCS_NOEXCEPT;

/* What planegcs makes of the system, and which constraints it blames.
 *
 * `out_blamed` receives the indices of conflicting constraints, then of
 * redundant ones, up to `capacity`; `out_blamed_count` is how many there were
 * in total, which may exceed the capacity. Indices are into the array the
 * session was built from, so a caller can name the constraint a person wrote.
 *
 * The two groups are written in that order and counted separately, and the
 * caller splits them on `out_conflicting_count`. They are different findings:
 * a redundant sketch still solves, and telling somebody their drawing is
 * impossible when a constraint is merely repeated is the wrong sentence.
 */
int32_t fc_gcs_session_diagnose(FcGcsSession *session, int32_t *out_dofs,
                                int32_t *out_conflicting_count,
                                int32_t *out_redundant_count,
                                int32_t *out_blamed, size_t capacity,
                                size_t *out_blamed_count) FC_GCS_NOEXCEPT;

/* Partitions the already diagnosed system and captures its gesture-start
 * reference. Kept separate so setup and diagnosis can be measured without
 * charging the same diagnosis twice. */
int32_t fc_gcs_session_prepare(FcGcsSession *session) FC_GCS_NOEXCEPT;

/* Moves the target of a Fixed constraint, which is what dragging is. */
int32_t fc_gcs_session_move(FcGcsSession *session, size_t constraint_index,
                            double x, double y) FC_GCS_NOEXCEPT;

/* Solves with the current targets. planegcs starts each solve from the
 * gesture-start reference captured by prepare, while reusing the constraint
 * graph and its partitioning. */
int32_t fc_gcs_session_solve(FcGcsSession *session) FC_GCS_NOEXCEPT;

/* Copies the current point positions out. */
int32_t fc_gcs_session_state(const FcGcsSession *session, double *out,
                             size_t count) FC_GCS_NOEXCEPT;

/* Copies the current radii out, one double per circle, in the order the
 * session was built from.
 *
 * A separate call rather than more doubles on the end of the state, so that a
 * caller which asks for the state of a sketch with circles in it cannot
 * silently receive a longer array and read a radius as a coordinate. */
int32_t fc_gcs_session_radii(const FcGcsSession *session, double *out,
                             size_t count) FC_GCS_NOEXCEPT;

/* Which planegcs this is, asked of the shared library rather than of this
 * shim, so that the answer identifies the library that was actually loaded. */
const char *fc_gcs_provenance(void) FC_GCS_NOEXCEPT;

/* How much work has actually crossed this boundary on the calling thread.
 *
 * Diagnostic, and the only way two claims the bench makes can be checked
 * rather than believed: that a result attributed to planegcs came from
 * planegcs and not from the reference implementation, and that a gesture used
 * one native system rather than rebuilding it fifty times. Both are invisible
 * in the numbers - a rebuilt system returns the same coordinates - and a bench
 * that cannot tell is a bench that will one day be wrong and look right.
 *
 * Per thread, not per process: the test harness runs each test on its own
 * thread, and a process-wide counter would be a race dressed as a measurement.
 */
uint64_t fc_gcs_native_solves(void) FC_GCS_NOEXCEPT;
uint64_t fc_gcs_native_sessions(void) FC_GCS_NOEXCEPT;

/* Sessions created minus sessions destroyed, on the calling thread.
 *
 * A session that was never released and one that was released twice are both
 * invisible in a result: the coordinates are the same either way, and the
 * second is a double free that may not fault until much later. This is what
 * lets a gate say the lifetime was right rather than assume it. */
uint64_t fc_gcs_native_live_sessions(void) FC_GCS_NOEXCEPT;

#ifdef __cplusplus
}
#endif

#undef FC_GCS_NOEXCEPT

#endif
