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

/* Fixed-width rather than a C enum: an enum's underlying type is an
 * implementation choice, which is exactly what a cross-compiler ABI must not
 * contain. */
typedef int32_t FcOcctStatus;
enum {
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
};

/* Caller-owned error detail. Always NUL-terminated after a failed call, and
 * left untouched after a successful one. */
typedef struct FcOcctError {
  char message[FC_OCCT_ERROR_CAPACITY];
} FcOcctError;

typedef int32_t FcOcctSegmentKind;
enum {
  FC_OCCT_SEGMENT_LINE = 0,
  FC_OCCT_SEGMENT_ARC = 1
};

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

#ifdef __cplusplus
#define FC_OCCT_NOEXCEPT noexcept
#else
#define FC_OCCT_NOEXCEPT
#endif

/* The Open CASCADE version this bridge was compiled against, e.g. "7.9.3".
 * Static storage; never freed. Reported rather than assumed, because it is
 * part of every cache key. */
const char *fc_occt_version(void) FC_OCCT_NOEXCEPT;

FcOcctStatus fc_occt_session_create(FcOcctSession **out_session,
                                    FcOcctError *out_error) FC_OCCT_NOEXCEPT;

/* Destroys a session and every shape it still holds. Null is accepted. */
void fc_occt_session_destroy(FcOcctSession *session) FC_OCCT_NOEXCEPT;

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
                             FcOcctError *out_error) FC_OCCT_NOEXCEPT;

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
                                        FcOcctError *out_error) FC_OCCT_NOEXCEPT;

/* The cap faces. `which` is 0 for the start cap and 1 for the end cap. */
FcOcctStatus fc_occt_extrude_cap_faces(FcOcctSession *session, uint64_t shape,
                                       int32_t which, uint64_t *out_ids,
                                       size_t capacity, size_t *out_count,
                                       FcOcctError *out_error) FC_OCCT_NOEXCEPT;

/*
 * Face count and volume of a shape.
 *
 * Present so a caller can check that a solid is the one it asked for while
 * tessellation is still unimplemented. Cheap, and the only assertion available
 * about real geometry in this slice.
 */
FcOcctStatus fc_occt_shape_stats(FcOcctSession *session, uint64_t shape,
                                 uint64_t *out_face_count, double *out_volume,
                                 FcOcctError *out_error) FC_OCCT_NOEXCEPT;

/*
 * Serialises a shape into Open CASCADE's binary B-Rep form.
 *
 * Call with `capacity` 0 to learn the length, then again with a buffer that
 * size, as with the face queries.
 *
 * The bytes are Open CASCADE's own and carry no FerriteCAD framing; the caller
 * adds its version, length and integrity check. Triangulation is deliberately
 * not written: a tessellation is cached under its own key, at its own
 * deflection, and embedding one here would tie two independent results
 * together.
 */
FcOcctStatus fc_occt_encode_shape(FcOcctSession *session, uint64_t shape,
                                  uint8_t *out_bytes, size_t capacity,
                                  size_t *out_length,
                                  FcOcctError *out_error) FC_OCCT_NOEXCEPT;

/*
 * Restores a shape from bytes written by fc_occt_encode_shape.
 *
 * The result carries geometry and nothing else. Open CASCADE's B-Rep format
 * stores shapes, not the history of the operations that made them, so a
 * decoded shape has no side faces and no caps — and this bridge refuses those
 * queries on one rather than answering with an empty list. A caller that needs
 * names must cache the mapping alongside the geometry and restore both.
 */
FcOcctStatus fc_occt_decode_shape(FcOcctSession *session,
                                  const uint8_t *bytes, size_t length,
                                  uint64_t *out_shape,
                                  FcOcctError *out_error) FC_OCCT_NOEXCEPT;

/*
 * Archives a shape together with sub-shapes the caller wants to find again.
 *
 * The archive is a compound this bridge builds: the shape first, then each
 * requested sub-shape, in the order given. A slot is that position — 0 is the
 * shape itself and `k + 1` is the k-th sub-shape — and it means nothing
 * outside this one blob. It is not a traversal index and not a name.
 * This slice accepts faces only; later sub-shape kinds require carrying their
 * actual kind through the ABI rather than labelling everything as a face.
 *
 * The obvious alternative does not work. BinTools_ShapeSet hands out an index
 * per shape and can look one up again, but the lookup strips the location, and
 * the two caps of a prism share a TShape: both resolve to the same index, so a
 * reference to one would silently resolve to the other. Measured on OCCT
 * 7.9.3. Writing the wanted sub-shapes down explicitly avoids the question.
 *
 * `out_slots` receives one slot per requested sub-shape and must have room for
 * `sub_shape_count`. The bytes follow the usual two-call protocol: pass a zero
 * capacity to learn the length.
 */
/*
 * Triangulates a shape, reporting which face each triangle belongs to.
 *
 * Two calls. Pass zero capacities to learn the three counts, then call again
 * with buffers that large. The second call is cheap: Open CASCADE stores the
 * triangulation on the shape itself, and re-meshing with the same deflection
 * finds the work already done.
 *
 * `vertex_capacity` counts vertices, not floats; `out_positions` and
 * `out_normals` each need three floats per vertex. Vertices are never shared
 * between faces, so every triangle belongs to exactly one range and a caller
 * can draw one face without touching another's data.
 *
 * The face of each range is reported as a sub-shape identifier of this
 * session, the same kind `fc_occt_extrude` hands out, so a caller that already
 * knows which face its name refers to can find that face's triangles without
 * comparing geometry.
 *
 * Normals are computed here rather than read back: on OCCT 7.9.3 the
 * triangulation a mesher produces carries none. A node's normal is the
 * average of the triangles meeting at it within its own face, which is exact
 * for a plane and smooth across a cylinder.
 *
 * Orientation is applied, not assumed. Five of a prism's six faces come back
 * REVERSED, so a caller that trusted the stored winding would light most of
 * the solid inside out. Reversed faces have their winding swapped and their
 * normals negated here, and each face's location is applied to its nodes.
 *
 * Cancellation: unlike a prism, the mesher does poll the progress indicator —
 * 32 times for a six-faced box on 7.9.3 — so `cancel` can stop this operation
 * partway rather than only between operations.
 */
FcOcctStatus fc_occt_tessellate(
    FcOcctSession *session, uint64_t shape, double linear_deflection,
    double angular_deflection, uint8_t relative, FcOcctCancelFn cancel,
    void *cancel_context, float *out_positions, float *out_normals,
    size_t vertex_capacity, uint32_t *out_indices, size_t index_capacity,
    uint64_t *out_face_shapes, uint32_t *out_face_first,
    uint32_t *out_face_index_count, size_t face_capacity,
    size_t *out_vertex_count, size_t *out_index_count, size_t *out_face_count,
    FcOcctError *out_error) FC_OCCT_NOEXCEPT;

FcOcctStatus fc_occt_encode_shape_named(
    FcOcctSession *session, uint64_t shape, const uint64_t *sub_shapes,
    size_t sub_shape_count, uint32_t *out_slots, uint8_t *out_bytes,
    size_t capacity, size_t *out_length,
    FcOcctError *out_error) FC_OCCT_NOEXCEPT;

/*
 * Restores a shape and the sub-shapes named by their slots.
 *
 * `out_sub_shapes` receives one session-local identifier per requested slot
 * and must have room for `slot_count`. Slot 0 is the shape itself and is
 * refused here: it is not a sub-shape.
 *
 * The restored shape still carries no history. This call returns the
 * sub-shapes the caller archived, and nothing about how they were made; a
 * semantic name is reattached by the layer that stored the slot table, not by
 * the kernel.
 */
FcOcctStatus fc_occt_decode_shape_named(
    FcOcctSession *session, const uint8_t *bytes, size_t length,
    const uint32_t *slots, size_t slot_count, uint64_t *out_shape,
    uint64_t *out_sub_shapes, FcOcctError *out_error) FC_OCCT_NOEXCEPT;

/* Drops a shape. Releasing an unknown or already-released identifier is not an
 * error: an unwinding caller must be able to release everything it might hold
 * without first working out what it actually holds. */
void fc_occt_release_shape(FcOcctSession *session,
                           uint64_t shape) FC_OCCT_NOEXCEPT;

/* How many shapes the session still holds. For tests: handles are opaque, so
 * "did anything leak" cannot be answered from outside without asking. */
size_t fc_occt_live_shape_count(const FcOcctSession *session) FC_OCCT_NOEXCEPT;

#ifdef __cplusplus
} /* extern "C" */
#endif

#undef FC_OCCT_NOEXCEPT

#endif /* FERRITECAD_OCCT_H */
