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

/* What sort of sub-shape an archive restored.
 *
 * Reported rather than assumed. An archive carries faces, edges and vertices
 * alike, and a caller that guessed would hand back a vertex under a face's
 * name. These three are the whole vocabulary: fc_occt_decode_shape_named
 * refuses an archive holding anything else rather than reporting a kind that
 * is not on this list. */
typedef int32_t FcOcctSubShapeKind;
enum {
  FC_OCCT_SUB_SHAPE_FACE = 0,
  FC_OCCT_SUB_SHAPE_EDGE = 1,
  FC_OCCT_SUB_SHAPE_VERTEX = 2
};

/* An opaque kernel session. Shapes belong to the session that made them. */
typedef struct FcOcctSession FcOcctSession;

/*
 * Where fc_occt_tessellate writes which topological edge draws which segments.
 *
 * A struct rather than six more positional parameters. fc_occt_tessellate had
 * already reached the length at which an argument list stops being checkable
 * by eye, and a caller that transposed two same-typed pointers would get a
 * silently wrong wireframe rather than a compiler error.
 *
 * Caller-owned throughout, like every other buffer here, and read with the
 * same two-call protocol: zero capacities fill in the counts and write no
 * data, then a second call with buffers that size fills them.
 *
 * `segments` holds two vertex indices per segment, into the same vertices the
 * triangles use, and `segment_capacity` counts segments rather than indices.
 * `edge_shapes` receives one session-local sub-shape identifier per
 * topological edge, and `edge_first_segment` / `edge_segment_count` say which
 * run of segments that edge owns. The runs are contiguous, in edge order, and
 * cover every segment exactly once.
 */
typedef struct FcOcctEdgeBuffers {
  uint32_t *segments;
  size_t segment_capacity;
  uint64_t *edge_shapes;
  uint32_t *edge_first_segment;
  uint32_t *edge_segment_count;
  size_t edge_capacity;
  /* Always written, on both calls. */
  size_t out_segment_count;
  size_t out_edge_count;
} FcOcctEdgeBuffers;

/*
 * Where fc_occt_tessellate writes which packed positions are which corner.
 *
 * A B-Rep vertex is one point of the model and several points of the mesh:
 * every face meeting there carries its own copy. So this is one identifier per
 * topological vertex and a run of packed positions for each, never a partition
 * of the positions: most nodes of a tessellation lie inside a face and are no
 * B-Rep vertex at all.
 *
 * The association is read from topology, not from coordinates. For each edge
 * lying on a meshed face, BRep_Tool::PolygonOnTriangulation gives that edge's
 * run of nodes across the face, and TopExp::Vertices gives the edge's two ends;
 * the first and last node of the run are those two ends.
 *
 * Two details are measured rather than assumed, on OCCT 7.9.3 over a plate, a
 * cylinder, a half cylinder, a sphere, a torus, a filleted and a shelled plate
 * and five STEP files:
 *
 *   * the location is load-bearing. Asked without it, every association on the
 *     two assembly files is lost, 30 of 30 and 96 of 96.
 *   * the run follows the edge's own sense, not the face's use of it. Of the
 *     edge uses that are REVERSED in their face, reading the ends face-relative
 *     puts the first node at the wrong end in every single case measured.
 *
 * Caller-owned and read with the same two-call protocol as everything else.
 * `occurrences` holds packed position indices; `vertex_shapes` receives one
 * session-local identifier per topological vertex, and `vertex_first` /
 * `vertex_occurrence_count` say which run of occurrences that vertex owns. The
 * runs are contiguous, in vertex order, and cover every occurrence exactly.
 */
typedef struct FcOcctVertexBuffers {
  uint32_t *occurrences;
  size_t occurrence_capacity;
  uint64_t *vertex_shapes;
  uint32_t *vertex_first;
  uint32_t *vertex_occurrence_count;
  size_t vertex_capacity;
  /* Always written, on both calls. */
  size_t out_occurrence_count;
  size_t out_vertex_count;
} FcOcctVertexBuffers;

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

/*
 * The edge one profile segment left where a cap meets the face swept from it.
 *
 * `which` is 0 for the start cap and 1 for the end cap, as for the cap faces.
 * Zero or one identifier comes back: a segment that produced no such edge is
 * reported as none rather than matched to a neighbour.
 *
 * The association is BRepPrimAPI_MakePrism's own — FirstShape and LastShape of
 * the input edge — and was measured before it was relied on. On 7.9.3, over a
 * plate swept blind, reversed and symmetrically and a profile with an arc
 * swept blind and symmetrically: every segment yielded an EDGE belonging to
 * the solid and bounding the cap it should, start and end never overlapped,
 * and every named edge was one the tessellation walk also reaches, so a name
 * and a drawn line are the same sub-shape.
 */
FcOcctStatus fc_occt_extrude_cap_edges(FcOcctSession *session, uint64_t shape,
                                       size_t segment_index, int32_t which,
                                       uint64_t *out_ids, size_t capacity,
                                       size_t *out_count,
                                       FcOcctError *out_error) FC_OCCT_NOEXCEPT;

/*
 * The edge swept from one corner of the profile.
 *
 * `joint_index` counts corners the way the segments are counted: joint `j` is
 * where segment `j - 1` meets segment `j` round the loop, so joint 0 is where
 * the last segment meets the first. Everything the algorithm generated there
 * comes back, so a count other than one is reported rather than trimmed.
 *
 * The association is BRepPrimAPI_MakePrism's own answer for the corner vertex
 * the two input edges already share, and was measured before it was relied on.
 * On 7.9.3, over a rectangular plate swept blind, reversed, symmetrically and
 * reversed-symmetrically, a three-segment profile containing an arc swept
 * blind and symmetrically, a triangle, and a two-segment profile: every corner
 * yielded exactly one EDGE that belongs to the solid, bounds exactly the two
 * side faces raised from the segments meeting there, is never a cap edge, is
 * never shared with another corner, and is one the tessellation walk reaches.
 * Rebuilding a profile from a different starting segment moved no association.
 */
FcOcctStatus fc_occt_extrude_sweep_edges(FcOcctSession *session, uint64_t shape,
                                         size_t joint_index, uint64_t *out_ids,
                                         size_t capacity, size_t *out_count,
                                         FcOcctError *out_error)
    FC_OCCT_NOEXCEPT;

/*
 * The vertex one corner of the profile reaches on one cap.
 *
 * `joint_index` counts corners the way the segments are counted, exactly as
 * fc_occt_extrude_sweep_edges does: corner `j` is where segment `j - 1` meets
 * segment `j`. `which` is 0 for the start cap and 1 for the end cap.
 *
 * Positional on purpose. Whether the unordered pair of segments meeting at a
 * corner names that corner uniquely is not a question this layer can answer: a
 * loop of two segments has one such pair at both of its corners. Keying these
 * answers by the pair here would merge them before the caller could see the
 * ambiguity, so the pairing is left to the caller and the answers are reported
 * per corner.
 *
 * Everything the algorithm recorded at that corner comes back, so a count
 * other than one is reported rather than trimmed. A vertex outside the
 * finished solid is refused when the shape is built, not passed on.
 *
 * The association is BRepPrimAPI_MakePrism's own FirstShape and LastShape of
 * the shared corner vertex, and was measured before it was relied on. See
 * tools/occt-smoke step 9, which the pin workflow runs on Linux, macOS and
 * Windows: over seven sweeps and forty-six positional answers, every one was a
 * TopoDS_VERTEX in the solid by IsSame, on the cap claimed for it, ending the
 * edge swept from the same corner, and reached by the tessellation
 * association.
 */
FcOcctStatus fc_occt_extrude_cap_vertices(FcOcctSession *session,
                                          uint64_t shape, size_t joint_index,
                                          int32_t which, uint64_t *out_ids,
                                          size_t capacity, size_t *out_count,
                                          FcOcctError *out_error)
    FC_OCCT_NOEXCEPT;

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
 * Reads a STEP file that is already in memory.
 *
 * Bytes rather than a path: the bridge opens nothing. Where the data came
 * from — a file, a network, a test — is the caller's business, and a bridge
 * that took a path would have to grow its own opinions about encodings,
 * permissions and what happens when the file changes underneath it.
 *
 * The result is one buffer in FerriteCAD's own encoding, read back with the
 * usual two-call protocol: pass a zero capacity to learn the length, then
 * call again. A tree with names in it does not fit a handful of parallel
 * arrays without inventing a second protocol, so it is written down once and
 * parsed on the other side.
 *
 * The encoding, all little-endian, is:
 *
 *   magic "FCSI", format version u16
 *   status u8            0 = imported, 1 = rejected before anything was built
 *   source unit          length-prefixed UTF-8
 *   schema               length-prefixed UTF-8
 *   definition count u32, then per definition:
 *       shape            u64, a handle into this session
 *       name             length-prefixed UTF-8
 *       solids           u32
 *       key              length-prefixed UTF-8, never empty
 *   instance count u32, then per instance:
 *       definition       u32
 *       parent           u32, or 0xFFFFFFFF for a root
 *       name             length-prefixed UTF-8
 *       placement        12 f64, a row-major 3x4 matrix
 *       colour source    u8   0 = none, 1 = from the instance, 2 = inherited
 *       colour           3 f64, linear RGB, meaningless when the source is 0
 *   diagnostic count u32, then per diagnostic:
 *       stage            u8   0 = load, 1 = transfer, 2 = identity
 *       severity         u8   0 = warning, 1 = fail
 *       entity           length-prefixed UTF-8, empty when not attributed
 *       message          length-prefixed UTF-8
 *
 * The key says what identifies a definition in the file rather than in this
 * reading of it, and it is the one field this bridge refuses to hand over a
 * scene without. A definition it cannot name, or two that carry one name, end
 * the import: status becomes rejected, an identity diagnostic says which, and
 * every shape the import registered is released. That is stricter than Open
 * CASCADE, which reads such a file without complaint — and it has to be,
 * because the alternative is a stored scene that can never be re-attached to
 * geometry once the session that read it is gone.
 *
 * A key is local to its source. `step.product_definition#31` identifies
 * something within one file and nothing at all between two, so a durable
 * reference has to carry the identity of the source alongside it.
 *
 * There is no "valid" flag and there will not be one. Measured on 8.0.1: of
 * six deliberately damaged files, Open CASCADE refuses two outright and reads
 * four. Of those four, this bridge refuses one more because a definition has
 * no identity, two are read and reported precisely, and one is read,
 * transferred and reported clean while carrying a malformed coordinate. A file
 * that produced no diagnostics is a file nothing was noticed about, which is
 * not the same as a sound one, and a flag would collapse that distinction
 * exactly where it matters.
 */
FcOcctStatus fc_occt_import_step(
    FcOcctSession *session, const uint8_t *bytes, size_t length,
    uint8_t *out_buffer, size_t capacity, size_t *out_length,
    FcOcctError *out_error) FC_OCCT_NOEXCEPT;

/*
 * Rounds every edge of a shape to one radius.
 *
 * Evaluation surface, not yet part of any feature. It exists to find out where
 * Open CASCADE's filleting stops working, which is a question that has to be
 * answered before a fillet feature is designed around it.
 *
 * The result is checked before it is kept. Measured on 7.9.3 with a 60 x 40 x
 * 10 plate: at r = 5 the builder reports failure, which is correct, but at
 * r = 5.1 and r = 6 it reports success and hands back a shape that fails
 * BRepCheck_Analyzer and encloses MORE volume than the block it was cut from.
 * A fillet on a convex edge removes material, so that shape is not a worse
 * answer — it is not an answer. Anything that fails the check is refused here
 * and no handle is issued for it.
 */
FcOcctStatus fc_occt_fillet_all(FcOcctSession *session, uint64_t shape,
                                double radius, FcOcctCancelFn cancel,
                                void *cancel_context, uint64_t *out_shape,
                                FcOcctError *out_error) FC_OCCT_NOEXCEPT;

/*
 * Hollows a solid, leaving the named faces open.
 *
 * `thickness` is the wall thickness in millimetres, always positive; the wall
 * is grown inwards. The faces are sub-shape identifiers of this session, so a
 * caller opens the face it already has a name for rather than one it found by
 * looking.
 *
 * Checked in the same way and for the same reason as the fillet above.
 */
FcOcctStatus fc_occt_shell(FcOcctSession *session, uint64_t shape,
                           double thickness, const uint64_t *open_faces,
                           size_t open_face_count, FcOcctCancelFn cancel,
                           void *cancel_context, uint64_t *out_shape,
                           FcOcctError *out_error) FC_OCCT_NOEXCEPT;

/*
 * Whether Open CASCADE considers a shape well formed.
 *
 * Offered so a caller can assert about shapes this bridge did not just build,
 * and so a corpus can say which inputs were sound before blaming an operation
 * for what came out.
 */
FcOcctStatus fc_occt_shape_is_valid(FcOcctSession *session, uint64_t shape,
                                    uint8_t *out_valid,
                                    FcOcctError *out_error) FC_OCCT_NOEXCEPT;

/*
 * Triangulates a shape, reporting which face each triangle belongs to.
 *
 * Two calls. Pass zero capacities to learn the three counts, then call again
 * with buffers that large. Both calls mesh from a clean shape. Open CASCADE
 * otherwise reuses a prior finer triangulation for a later coarse request,
 * making the result depend on call order rather than these parameters. The
 * bridge removes transient triangulation before and after each call.
 * `relative` is the fixed-width boolean 0 or 1; every other value is refused.
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
 *
 * `edges` reports which topological edge draws which segments, and must not be
 * null: an adapter that could not be told would have to fall back on joining
 * vertices that happen to be close, which is how a wireframe comes to claim
 * edges the model does not have.
 *
 * That association comes from BRep_Tool::PolygonOnTriangulation, which stores
 * a polyline of triangulation nodes for each edge of each face. Measured on
 * 7.9.3 across a prism, a half cylinder, a cylinder, a sphere, a torus, a
 * filleted plate, a shelled plate and five STEP fixtures: of 205 topological
 * edges and 403 edge-face sides, every side had a polygon, and no polygon
 * named a node outside its triangulation. So the association is read, never
 * inferred, and a missing polygon is reported as a failure rather than
 * patched over.
 *
 * TopLoc_Location is part of the query and not an optimisation. On the two
 * STEP assemblies, every one of the 125 sides loses its polygon if the face's
 * location is dropped and the identity location is passed instead; even the
 * plate loses 4 of 24.
 *
 * One topological edge is one identity. The two faces that meet at an edge
 * each carry their own polyline for it, and a seam edge carries two polylines
 * on the single face it lies on; all of them are reported under one
 * identifier, consolidated by TopoDS_Shape::IsSame, so orientation never
 * splits an edge into two. Each edge's segments are ordered as the polylines
 * are, face by face in the order the faces were packed.
 */
FcOcctStatus fc_occt_tessellate(
    FcOcctSession *session, uint64_t shape, double linear_deflection,
    double angular_deflection, uint8_t relative, FcOcctCancelFn cancel,
    void *cancel_context, float *out_positions, float *out_normals,
    size_t vertex_capacity, uint32_t *out_indices, size_t index_capacity,
    uint64_t *out_face_shapes, uint32_t *out_face_first,
    uint32_t *out_face_index_count, size_t face_capacity,
    FcOcctEdgeBuffers *edges, FcOcctVertexBuffers *corners,
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
 * and must have room for `slot_count`, and `out_sub_kinds` receives what each
 * of them actually is, as an FcOcctSubShapeKind read off the restored shape.
 * Slot 0 is the shape itself and is refused here: it is not a sub-shape.
 *
 * Every entry of the archive is checked, including entries no slot asks for.
 * An archive holding anything other than faces, edges and vertices of its own
 * root is refused whole rather than read past.
 *
 * The restored shape still carries no history. This call returns the
 * sub-shapes the caller archived, and nothing about how they were made; a
 * semantic name is reattached by the layer that stored the slot table, not by
 * the kernel.
 */
FcOcctStatus fc_occt_decode_shape_named(
    FcOcctSession *session, const uint8_t *bytes, size_t length,
    const uint32_t *slots, size_t slot_count, uint64_t *out_shape,
    uint64_t *out_sub_shapes, int32_t *out_sub_kinds,
    FcOcctError *out_error) FC_OCCT_NOEXCEPT;

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
