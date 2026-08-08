/* SPDX-License-Identifier: MIT */
/*
 * The flat C ABI between FerriteCAD and Open CASCADE.
 *
 * Everything Rust knows about OCCT passes through this header. No OCCT type,
 * template, handle or header appears in it, and nothing here allocates on
 * behalf of the caller: buffers are supplied by the caller and errors are
 * written into a caller-owned struct. That removes ownership questions from
 * the boundary entirely — there is nothing to free and no allocator to agree
 * on.
 *
 * Every function is noexcept. A C++ exception unwinding into Rust is undefined
 * behaviour, not a rough edge, so each entry point catches Standard_Failure,
 * std::exception and everything else, and converts it to a status code.
 */

#ifndef FERRITECAD_OCCT_H
#define FERRITECAD_OCCT_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Longest error message carried across the boundary. Messages are truncated
 * rather than allocated; an OCCT failure that needs more than this to identify
 * has bigger problems than its wording. */
#define FC_OCCT_ERROR_CAPACITY 512

typedef enum FcOcctStatus {
  FC_OCCT_OK = 0,
  /* The request is malformed: a degenerate profile, a bad plane. */
  FC_OCCT_INVALID_INPUT = 1,
  /* The kernel refused or failed to build the geometry. */
  FC_OCCT_KERNEL = 2,
  /* The caller's cancellation callback asked to stop. */
  FC_OCCT_CANCELLED = 3,
  /* Well-formed, but this bridge does not implement it. */
  FC_OCCT_UNSUPPORTED = 4,
  /* A shape or sub-shape identifier this session did not issue. */
  FC_OCCT_UNKNOWN_HANDLE = 5,
  /* An exception that is not Standard_Failure or std::exception. */
  FC_OCCT_INTERNAL = 6
} FcOcctStatus;

/* Caller-owned error detail. Always NUL-terminated after a failed call, and
 * left untouched after a successful one. */
typedef struct FcOcctError {
  char message[FC_OCCT_ERROR_CAPACITY];
} FcOcctError;

typedef enum FcOcctSegmentKind {
  FC_OCCT_SEGMENT_LINE = 0,
  FC_OCCT_SEGMENT_ARC = 1
} FcOcctSegmentKind;

/* One profile segment, in the sketch plane's own 2D coordinates.
 *
 * A single struct rather than a union so the layout is trivially stable across
 * compilers; the unused fields cost a few bytes per segment and remove a whole
 * class of ABI question. */
typedef struct FcOcctSegment {
  int32_t kind; /* FcOcctSegmentKind */
  double start_x, start_y;
  double end_x, end_y;
  double center_x, center_y;
  double radius;
  double start_angle, end_angle;
} FcOcctSegment;

/* The sketch plane in model space. Axes must be unit and orthogonal; the
 * bridge checks rather than assumes. */
typedef struct FcOcctPlane {
  double origin[3];
  double x_axis[3];
  double normal[3];
} FcOcctPlane;

/* Returns non-zero to ask the running operation to stop.
 *
 * See fc_occt_extrude for what Open CASCADE actually does with this. */
typedef int32_t (*FcOcctCancelFn)(void *context);

/* An opaque kernel session. Shapes belong to the session that made them. */
typedef struct FcOcctSession FcOcctSession;

/* The Open CASCADE version this bridge was compiled against, e.g. "7.9.3".
 * Static storage; never freed. Reported rather than assumed, because it is
 * part of every cache key. */
const char *fc_occt_version(void);

FcOcctStatus fc_occt_session_create(FcOcctSession **out_session,
                                    FcOcctError *out_error);

/* Destroys a session and every shape it still holds. Null is accepted. */
void fc_occt_session_destroy(FcOcctSession *session);

/*
 * Sweeps a closed planar profile into a solid.
 *
 * `base_offset` and `top_offset` are distances along the plane normal, so a
 * blind extrusion is (0, d) and a symmetric one is (-d, +d).
 *
 * Cancellation: `cancel` is consulted before the profile is built and again
 * before the sweep, and is also installed as an Open CASCADE progress
 * indicator. Be aware that BRepPrimAPI_MakePrism does not poll that indicator
 * — measured on OCCT 7.9.3, UserBreak was called zero times during a prism —
 * so for this operation cancellation is effectively checked between steps, not
 * inside them. The indicator is wired anyway because the algorithms that come
 * later do poll it.
 *
 * On success `*out_shape` receives a session-local shape identifier.
 */
FcOcctStatus fc_occt_extrude(FcOcctSession *session, const FcOcctPlane *plane,
                             const FcOcctSegment *segments,
                             size_t segment_count, double base_offset,
                             double top_offset, FcOcctCancelFn cancel,
                             void *cancel_context, uint64_t *out_shape,
                             FcOcctError *out_error);

/*
 * The faces the sweep raised from one profile segment.
 *
 * Call with `capacity` 0 to learn the count, then again with a buffer. The
 * two-call shape keeps allocation on the caller's side.
 */
FcOcctStatus fc_occt_extrude_side_faces(FcOcctSession *session, uint64_t shape,
                                        size_t segment_index,
                                        uint64_t *out_ids, size_t capacity,
                                        size_t *out_count,
                                        FcOcctError *out_error);

/* The cap faces. `which` is 0 for the start cap and 1 for the end cap. */
FcOcctStatus fc_occt_extrude_cap_faces(FcOcctSession *session, uint64_t shape,
                                       int32_t which, uint64_t *out_ids,
                                       size_t capacity, size_t *out_count,
                                       FcOcctError *out_error);

/*
 * Face count and volume of a shape.
 *
 * Present so a caller can check that a solid is the one it asked for while
 * tessellation is still unimplemented. Cheap, and the only assertion available
 * about real geometry in this slice.
 */
FcOcctStatus fc_occt_shape_stats(FcOcctSession *session, uint64_t shape,
                                 uint64_t *out_face_count, double *out_volume,
                                 FcOcctError *out_error);

/* Drops a shape. Releasing an unknown or already-released identifier is not an
 * error: an unwinding caller must be able to release everything it might hold
 * without first working out what it actually holds. */
void fc_occt_release_shape(FcOcctSession *session, uint64_t shape);

/* How many shapes the session still holds. For tests: handles are opaque, so
 * "did anything leak" cannot be answered from outside without asking. */
size_t fc_occt_live_shape_count(const FcOcctSession *session);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* FERRITECAD_OCCT_H */
