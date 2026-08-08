// SPDX-License-Identifier: MIT
//
// The only place in FerriteCAD where Open CASCADE headers are included.
//
// Two rules govern everything here. No exception may leave a function: each
// entry point is wrapped in a catch-all that converts to a status code,
// because unwinding into Rust is undefined behaviour. And no OCCT type may
// appear in the header: what crosses the boundary is integers, doubles and
// opaque identifiers.

#include "ferritecad_occt.h"

#include <cmath>
#include <cstring>
#include <sstream>
#include <exception>
#include <string>
#include <unordered_map>
#include <vector>

#include <BRep_Tool.hxx>
#include <BRep_Builder.hxx>
#include <BinTools.hxx>
#include <BRepBuilderAPI_MakeEdge.hxx>
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_MakeVertex.hxx>
#include <BRepBuilderAPI_MakeWire.hxx>
#include <BRepGProp.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepPrimAPI_MakePrism.hxx>
#include <BRepTools_History.hxx>
#include <GC_MakeArcOfCircle.hxx>
#include <GProp_GProps.hxx>
#include <Message_ProgressIndicator.hxx>
#include <Message_ProgressScope.hxx>
#include <Standard_Failure.hxx>
#include <Standard_Type.hxx>
#include <Standard_Version.hxx>
#include <TopExp_Explorer.hxx>
#include <NCollection_List.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Edge.hxx>
#include <TopoDS_Face.hxx>
#include <TopoDS_Shape.hxx>
#include <TopoDS_Vertex.hxx>
#include <TopoDS_Compound.hxx>
#include <TopoDS_Iterator.hxx>
#include <TopoDS_Wire.hxx>
#include <Poly_Triangulation.hxx>
#include <TopLoc_Location.hxx>
#include <gp_Ax3.hxx>
#include <gp_Dir.hxx>
#include <gp_Pln.hxx>
#include <gp_Pnt.hxx>
#include <gp_Vec.hxx>

namespace {

/// A shape the session owns, with the sub-shapes the caller may name.
struct ShapeRecord {
  TopoDS_Shape shape;
  /// True when the shape came back from a cache blob rather than an operation.
  ///
  /// Open CASCADE's B-Rep format stores a shape, not the history of what made
  /// it, so a decoded shape has no side faces and no caps. Recording that lets
  /// the queries refuse rather than answer with an empty list, which a naming
  /// layer would read as "this feature produced nothing".
  bool decoded = false;
  /// Face identifiers raised from each profile segment, in segment order.
  std::vector<std::vector<uint64_t>> side_faces;
  std::vector<uint64_t> start_cap;
  std::vector<uint64_t> end_cap;
  /// Identifier to sub-shape. The identifiers mean nothing outside this
  /// session, which is exactly what the Rust side promises about them.
  std::vector<TopoDS_Shape> sub_shapes;

  uint64_t remember(const TopoDS_Shape &sub) {
    // The same OCCT face can be reported through more than one route. It must
    // still have one session-local identity; otherwise two handles compare
    // different even though they name the same topology, hiding precisely the
    // silent retargeting the naming layer is meant to catch.
    for (size_t i = 0; i < sub_shapes.size(); ++i) {
      if (sub_shapes[i].IsSame(sub)) {
        return static_cast<uint64_t>(i);
      }
    }
    sub_shapes.push_back(sub);
    return static_cast<uint64_t>(sub_shapes.size() - 1);
  }
};

/// Consults the caller's cancellation callback.
///
/// Wired into every algorithm that accepts a progress range. Whether an
/// algorithm actually polls it is the algorithm's business; see the note on
/// fc_occt_extrude in the header for what a prism does.
class CancelIndicator : public Message_ProgressIndicator {
public:
  CancelIndicator(FcOcctCancelFn callback, void *context)
      : myCallback(callback), myContext(context) {}

  bool UserBreak() override {
    ++myPolls;
    return myCallback != nullptr && myCallback(myContext) != 0;
  }

  void Show(const Message_ProgressScope &, const bool) override {}

  int Polls() const { return myPolls; }

private:
  FcOcctCancelFn myCallback = nullptr;
  void *myContext = nullptr;
  int myPolls = 0;
};

void write_error(FcOcctError *out_error, const std::string &message) {
  if (out_error == nullptr) {
    return;
  }
  const size_t limit = FC_OCCT_ERROR_CAPACITY - 1;
  const size_t length = message.size() < limit ? message.size() : limit;
  std::memcpy(out_error->message, message.data(), length);
  out_error->message[length] = '\0';
}

bool cancelled(FcOcctCancelFn callback, void *context) {
  return callback != nullptr && callback(context) != 0;
}

/// Describes an Open CASCADE failure, on either side of the 8.0 boundary.
///
/// Open CASCADE 8.0 reparented Standard_Failure from Standard_Transient to
/// std::exception: DynamicType() no longer exists and GetMessageString() is
/// deprecated in favour of what(). Both spellings are kept rather than picking
/// the newer one, because a contributor's installed Open CASCADE is not
/// necessarily the pinned one.
std::string describe(const Standard_Failure &failure) {
#if OCC_VERSION_HEX >= 0x080000
  const char *text = failure.what();
  return text != nullptr && text[0] != '\0'
             ? std::string("Open CASCADE raised ") + text
             : std::string("Open CASCADE raised an unnamed failure");
#else
  std::string message = "Open CASCADE raised ";
  message += failure.DynamicType()->Name();
  const char *detail = failure.GetMessageString();
  if (detail != nullptr && detail[0] != '\0') {
    message += ": ";
    message += detail;
  }
  return message;
#endif
}

/// Runs `body`, converting anything it throws into a status code.
template <typename Body>
FcOcctStatus guarded(FcOcctError *out_error, Body body) noexcept {
  try {
    return body();
  } catch (const Standard_Failure &failure) {
    write_error(out_error, describe(failure));
    return FC_OCCT_KERNEL;
  } catch (const std::exception &error) {
    write_error(out_error, std::string("the bridge failed: ") + error.what());
    return FC_OCCT_KERNEL;
  } catch (...) {
    // Nothing is known about this, so nothing is claimed about it.
    write_error(out_error, "the bridge threw an exception of unknown type");
    return FC_OCCT_INTERNAL;
  }
}

bool finite3(const double v[3]) {
  return std::isfinite(v[0]) && std::isfinite(v[1]) && std::isfinite(v[2]);
}

double dot3(const double a[3], const double b[3]) {
  return a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
}

} // namespace

struct FcOcctSession {
  std::unordered_map<uint64_t, ShapeRecord> shapes;
  uint64_t next_shape = 1;
};

extern "C" {

const char *fc_occt_version(void) noexcept { return OCC_VERSION_COMPLETE; }

FcOcctStatus fc_occt_session_create(FcOcctSession **out_session,
                                    FcOcctError *out_error) noexcept {
  return guarded(out_error, [&]() -> FcOcctStatus {
    if (out_session == nullptr) {
      write_error(out_error, "fc_occt_session_create needs somewhere to put "
                             "the session");
      return FC_OCCT_INVALID_INPUT;
    }
    *out_session = new FcOcctSession();
    return FC_OCCT_OK;
  });
}

void fc_occt_session_destroy(FcOcctSession *session) noexcept { delete session; }

FcOcctStatus fc_occt_extrude(FcOcctSession *session, const FcOcctPlane *plane,
                             const FcOcctSegment *segments,
                             size_t segment_count, double base_offset,
                             double top_offset, FcOcctCancelFn cancel,
                             void *cancel_context, uint64_t *out_shape,
                             FcOcctError *out_error) noexcept {
  return guarded(out_error, [&]() -> FcOcctStatus {
    if (session == nullptr || plane == nullptr || segments == nullptr ||
        out_shape == nullptr) {
      write_error(out_error, "fc_occt_extrude was given a null argument");
      return FC_OCCT_INVALID_INPUT;
    }
    if (segment_count < 2) {
      write_error(out_error,
                  "a closed profile needs at least two segments, got " +
                      std::to_string(segment_count));
      return FC_OCCT_INVALID_INPUT;
    }
    if (!finite3(plane->origin) || !finite3(plane->x_axis) ||
        !finite3(plane->normal) || !std::isfinite(base_offset) ||
        !std::isfinite(top_offset)) {
      write_error(out_error, "the extrusion request contains a non-finite "
                             "number");
      return FC_OCCT_INVALID_INPUT;
    }
    if (std::abs(top_offset - base_offset) < 1.0e-12) {
      write_error(out_error, "the extrusion has no length");
      return FC_OCCT_INVALID_INPUT;
    }
    // Checked rather than assumed: a frame a fraction of a degree off square
    // builds a solid that is subtly wrong everywhere.
    if (std::abs(dot3(plane->x_axis, plane->normal)) > 1.0e-9) {
      write_error(out_error, "the plane's X axis is not perpendicular to its "
                             "normal");
      return FC_OCCT_INVALID_INPUT;
    }

    if (cancelled(cancel, cancel_context)) {
      return FC_OCCT_CANCELLED;
    }

    const gp_Pnt origin(plane->origin[0], plane->origin[1], plane->origin[2]);
    const gp_Dir normal(plane->normal[0], plane->normal[1], plane->normal[2]);
    const gp_Dir x_axis(plane->x_axis[0], plane->x_axis[1], plane->x_axis[2]);
    const gp_Ax3 frame(origin, normal, x_axis);
    const gp_Pln sketch_plane(frame);

    const auto to_model = [&](double x, double y, double lift) {
      const gp_Vec along_x = gp_Vec(frame.XDirection()) * x;
      const gp_Vec along_y = gp_Vec(frame.YDirection()) * y;
      const gp_Vec along_n = gp_Vec(frame.Direction()) * lift;
      return origin.Translated(along_x + along_y + along_n);
    };

    // Corner vertices are built once and shared between adjacent edges.
    //
    // This is not a micro-optimisation. BRepBuilderAPI_MakeWire welds the
    // vertices of the edges it is given, and welding *replaces* the edges: on
    // OCCT 7.9.3 only the first edge survived, so history queried with the
    // edges we built returned faces for one segment and nothing for the rest.
    // Sharing the vertices leaves MakeWire nothing to weld and every edge
    // keeps its identity, which is what makes the history complete.
    std::vector<TopoDS_Vertex> corners;
    corners.reserve(segment_count);
    for (size_t i = 0; i < segment_count; ++i) {
      const FcOcctSegment &segment = segments[i];
      double x = 0.0;
      double y = 0.0;
      if (segment.kind == FC_OCCT_SEGMENT_LINE) {
        x = segment.start_x;
        y = segment.start_y;
      } else if (segment.kind == FC_OCCT_SEGMENT_ARC) {
        x = segment.center_x + segment.radius * std::cos(segment.start_angle);
        y = segment.center_y + segment.radius * std::sin(segment.start_angle);
      } else {
        write_error(out_error, "segment " + std::to_string(i) +
                                   " has an unknown kind " +
                                   std::to_string(segment.kind));
        return FC_OCCT_UNSUPPORTED;
      }
      if (!std::isfinite(x) || !std::isfinite(y)) {
        write_error(out_error,
                    "segment " + std::to_string(i) + " starts at a non-finite "
                                                     "point");
        return FC_OCCT_INVALID_INPUT;
      }
      corners.push_back(BRepBuilderAPI_MakeVertex(to_model(x, y, base_offset)));
    }

    std::vector<TopoDS_Edge> edges;
    edges.reserve(segment_count);
    BRepBuilderAPI_MakeWire wire;
    for (size_t i = 0; i < segment_count; ++i) {
      const FcOcctSegment &segment = segments[i];
      const TopoDS_Vertex &from = corners[i];
      const TopoDS_Vertex &to = corners[(i + 1) % segment_count];

      TopoDS_Edge edge;
      if (segment.kind == FC_OCCT_SEGMENT_LINE) {
        edge = BRepBuilderAPI_MakeEdge(from, to);
      } else {
        // Three points define the arc unambiguously, which avoids having to
        // agree with OCCT about parameterisation and sweep direction.
        const double mid_angle =
            segment.start_angle +
            0.5 * (segment.end_angle - segment.start_angle);
        const gp_Pnt through =
            to_model(segment.center_x + segment.radius * std::cos(mid_angle),
                     segment.center_y + segment.radius * std::sin(mid_angle),
                     base_offset);
        GC_MakeArcOfCircle arc(BRep_Tool::Pnt(from), through,
                               BRep_Tool::Pnt(to));
        if (!arc.IsDone()) {
          write_error(out_error, "segment " + std::to_string(i) +
                                     " does not describe an arc");
          return FC_OCCT_INVALID_INPUT;
        }
        edge = BRepBuilderAPI_MakeEdge(arc.Value(), from, to);
      }

      edges.push_back(edge);
      wire.Add(edge);
    }

    if (!wire.IsDone()) {
      write_error(out_error, "the profile segments do not form a closed wire");
      return FC_OCCT_INVALID_INPUT;
    }

    BRepBuilderAPI_MakeFace face_builder(sketch_plane, wire.Wire());
    if (!face_builder.IsDone()) {
      write_error(out_error, "the profile does not bound a face on its plane");
      return FC_OCCT_INVALID_INPUT;
    }
    const TopoDS_Face face = face_builder.Face();

    if (cancelled(cancel, cancel_context)) {
      return FC_OCCT_CANCELLED;
    }

    const gp_Vec sweep = gp_Vec(frame.Direction()) * (top_offset - base_offset);
    BRepPrimAPI_MakePrism prism(face, sweep);

    Handle(CancelIndicator) indicator = new CancelIndicator(cancel,
                                                            cancel_context);
    prism.Build(indicator->Start());

    if (cancelled(cancel, cancel_context)) {
      return FC_OCCT_CANCELLED;
    }
    if (!prism.IsDone()) {
      write_error(out_error, "the sweep did not produce a solid");
      return FC_OCCT_KERNEL;
    }

    ShapeRecord record;
    record.shape = prism.Shape();

    NCollection_List<TopoDS_Shape> arguments;
    arguments.Append(face);
    const BRepTools_History history(arguments, prism);

    record.side_faces.resize(segment_count);
    for (size_t i = 0; i < segment_count; ++i) {
      const NCollection_List<TopoDS_Shape> &generated = history.Generated(edges[i]);
      for (NCollection_List<TopoDS_Shape>::Iterator it(generated); it.More();
           it.Next()) {
        record.side_faces[i].push_back(record.remember(it.Value()));
      }
    }

    // The caps are generated from no input at all — the sweep creates them —
    // so history cannot name them and the algorithm reports them apart.
    // Measured on 7.9.3: both are a TopoDS_Face.
    if (!prism.FirstShape().IsNull()) {
      record.start_cap.push_back(record.remember(prism.FirstShape()));
    }
    if (!prism.LastShape().IsNull()) {
      record.end_cap.push_back(record.remember(prism.LastShape()));
    }

    const uint64_t id = session->next_shape++;
    session->shapes.emplace(id, std::move(record));
    *out_shape = id;
    return FC_OCCT_OK;
  });
}

namespace {

FcOcctStatus copy_ids(const std::vector<uint64_t> &ids, uint64_t *out_ids,
                      size_t capacity, size_t *out_count,
                      FcOcctError *out_error) {
  if (out_count == nullptr) {
    write_error(out_error, "a count is required");
    return FC_OCCT_INVALID_INPUT;
  }
  *out_count = ids.size();
  if (capacity == 0) {
    return FC_OCCT_OK;
  }
  if (out_ids == nullptr || capacity < ids.size()) {
    write_error(out_error, "the buffer holds " + std::to_string(capacity) +
                               " but " + std::to_string(ids.size()) +
                               " were produced");
    return FC_OCCT_INVALID_INPUT;
  }
  for (size_t i = 0; i < ids.size(); ++i) {
    out_ids[i] = ids[i];
  }
  return FC_OCCT_OK;
}

} // namespace

FcOcctStatus fc_occt_extrude_side_faces(FcOcctSession *session, uint64_t shape,
                                        size_t segment_index,
                                        uint64_t *out_ids, size_t capacity,
                                        size_t *out_count,
                                        FcOcctError *out_error) noexcept {
  return guarded(out_error, [&]() -> FcOcctStatus {
    if (session == nullptr) {
      write_error(out_error, "no session");
      return FC_OCCT_INVALID_INPUT;
    }
    const auto found = session->shapes.find(shape);
    if (found == session->shapes.end()) {
      write_error(out_error, "shape " + std::to_string(shape) +
                                 " was released or never existed");
      return FC_OCCT_UNKNOWN_HANDLE;
    }
    if (found->second.decoded) {
      write_error(out_error,
                  "shape " + std::to_string(shape) +
                      " was decoded from a cache blob, which carries geometry "
                      "but no history; rebuild it to name its faces");
      return FC_OCCT_UNSUPPORTED;
    }
    if (segment_index >= found->second.side_faces.size()) {
      write_error(out_error, "segment " + std::to_string(segment_index) +
                                 " is outside the profile");
      return FC_OCCT_INVALID_INPUT;
    }
    return copy_ids(found->second.side_faces[segment_index], out_ids, capacity,
                    out_count, out_error);
  });
}

FcOcctStatus fc_occt_extrude_cap_faces(FcOcctSession *session, uint64_t shape,
                                       int32_t which, uint64_t *out_ids,
                                       size_t capacity, size_t *out_count,
                                       FcOcctError *out_error) noexcept {
  return guarded(out_error, [&]() -> FcOcctStatus {
    if (session == nullptr) {
      write_error(out_error, "no session");
      return FC_OCCT_INVALID_INPUT;
    }
    const auto found = session->shapes.find(shape);
    if (found == session->shapes.end()) {
      write_error(out_error, "shape " + std::to_string(shape) +
                                 " was released or never existed");
      return FC_OCCT_UNKNOWN_HANDLE;
    }
    if (found->second.decoded) {
      write_error(out_error,
                  "shape " + std::to_string(shape) +
                      " was decoded from a cache blob, which carries geometry "
                      "but no history; rebuild it to name its caps");
      return FC_OCCT_UNSUPPORTED;
    }
    if (which != 0 && which != 1) {
      write_error(out_error, "cap selector must be 0 or 1");
      return FC_OCCT_INVALID_INPUT;
    }
    const std::vector<uint64_t> &ids =
        which == 0 ? found->second.start_cap : found->second.end_cap;
    return copy_ids(ids, out_ids, capacity, out_count, out_error);
  });
}

FcOcctStatus fc_occt_shape_stats(FcOcctSession *session, uint64_t shape,
                                 uint64_t *out_face_count, double *out_volume,
                                 FcOcctError *out_error) noexcept {
  return guarded(out_error, [&]() -> FcOcctStatus {
    if (session == nullptr || out_face_count == nullptr ||
        out_volume == nullptr) {
      write_error(out_error, "fc_occt_shape_stats was given a null argument");
      return FC_OCCT_INVALID_INPUT;
    }
    const auto found = session->shapes.find(shape);
    if (found == session->shapes.end()) {
      write_error(out_error, "shape " + std::to_string(shape) +
                                 " was released or never existed");
      return FC_OCCT_UNKNOWN_HANDLE;
    }

    uint64_t faces = 0;
    for (TopExp_Explorer it(found->second.shape, TopAbs_FACE); it.More();
         it.Next()) {
      ++faces;
    }
    GProp_GProps properties;
    BRepGProp::VolumeProperties(found->second.shape, properties);

    *out_face_count = faces;
    *out_volume = properties.Mass();
    return FC_OCCT_OK;
  });
}

FcOcctStatus fc_occt_encode_shape(FcOcctSession *session, uint64_t shape,
                                  uint8_t *out_bytes, size_t capacity,
                                  size_t *out_length,
                                  FcOcctError *out_error) noexcept {
  return guarded(out_error, [&]() -> FcOcctStatus {
    if (session == nullptr || out_length == nullptr) {
      write_error(out_error, "fc_occt_encode_shape was given a null argument");
      return FC_OCCT_INVALID_INPUT;
    }
    const auto found = session->shapes.find(shape);
    if (found == session->shapes.end()) {
      write_error(out_error, "shape " + std::to_string(shape) +
                                 " was released or never existed");
      return FC_OCCT_UNKNOWN_HANDLE;
    }

    // No triangulation and no normals: a tessellation is cached separately,
    // under its own deflection, and bundling one here would tie two
    // independent results to a single key.
    std::ostringstream buffer(std::ios::out | std::ios::binary);
    BinTools::Write(found->second.shape, buffer, false, false,
                    BinTools_FormatVersion_CURRENT);
    if (!buffer.good()) {
      write_error(out_error, "Open CASCADE failed to serialise the shape");
      return FC_OCCT_KERNEL;
    }
    const std::string encoded = buffer.str();

    *out_length = encoded.size();
    if (capacity == 0) {
      return FC_OCCT_OK;
    }
    if (out_bytes == nullptr || capacity < encoded.size()) {
      write_error(out_error, "the buffer holds " + std::to_string(capacity) +
                                 " bytes but the shape needs " +
                                 std::to_string(encoded.size()));
      return FC_OCCT_INVALID_INPUT;
    }
    std::memcpy(out_bytes, encoded.data(), encoded.size());
    return FC_OCCT_OK;
  });
}

FcOcctStatus fc_occt_decode_shape(FcOcctSession *session, const uint8_t *bytes,
                                  size_t length, uint64_t *out_shape,
                                  FcOcctError *out_error) noexcept {
  return guarded(out_error, [&]() -> FcOcctStatus {
    if (session == nullptr || bytes == nullptr || out_shape == nullptr) {
      write_error(out_error, "fc_occt_decode_shape was given a null argument");
      return FC_OCCT_INVALID_INPUT;
    }
    if (length == 0) {
      write_error(out_error, "a cached shape cannot be empty");
      return FC_OCCT_INVALID_INPUT;
    }

    std::istringstream buffer(
        std::string(reinterpret_cast<const char *>(bytes), length),
        std::ios::in | std::ios::binary);

    TopoDS_Shape restored;
    BinTools::Read(restored, buffer);
    if (restored.IsNull()) {
      write_error(out_error, "the cached bytes did not describe a shape");
      return FC_OCCT_KERNEL;
    }
    if (buffer.peek() != std::char_traits<char>::eof()) {
      write_error(out_error,
                  "the cached shape has trailing bytes after its B-Rep");
      return FC_OCCT_INVALID_INPUT;
    }

    ShapeRecord record;
    record.shape = restored;
    record.decoded = true;

    const uint64_t id = session->next_shape++;
    session->shapes.emplace(id, std::move(record));
    *out_shape = id;
    return FC_OCCT_OK;
  });
}

FcOcctStatus fc_occt_tessellate(
    FcOcctSession *session, uint64_t shape, double linear_deflection,
    double angular_deflection, uint8_t relative, FcOcctCancelFn cancel,
    void *cancel_context, float *out_positions, float *out_normals,
    size_t vertex_capacity, uint32_t *out_indices, size_t index_capacity,
    uint64_t *out_face_shapes, uint32_t *out_face_first,
    uint32_t *out_face_index_count, size_t face_capacity,
    size_t *out_vertex_count, size_t *out_index_count, size_t *out_face_count,
    FcOcctError *out_error) noexcept {
  return guarded(out_error, [&]() -> FcOcctStatus {
    if (session == nullptr || out_vertex_count == nullptr ||
        out_index_count == nullptr || out_face_count == nullptr) {
      write_error(out_error, "fc_occt_tessellate was given a null argument");
      return FC_OCCT_INVALID_INPUT;
    }
    if (!std::isfinite(linear_deflection) || linear_deflection <= 0.0 ||
        !std::isfinite(angular_deflection) || angular_deflection <= 0.0) {
      write_error(out_error,
                  "tessellation needs positive, finite deflections");
      return FC_OCCT_INVALID_INPUT;
    }

    const auto found = session->shapes.find(shape);
    if (found == session->shapes.end()) {
      write_error(out_error, "shape " + std::to_string(shape) +
                                 " was released or never existed");
      return FC_OCCT_UNKNOWN_HANDLE;
    }
    if (cancelled(cancel, cancel_context)) {
      return FC_OCCT_CANCELLED;
    }

    ShapeRecord &record = found->second;
    Handle(CancelIndicator) indicator = new CancelIndicator(cancel, cancel_context);

    // Explicit on every axis. The defaults differ between Open CASCADE
    // releases, and a mesh whose fineness quietly changed with the kernel
    // would invalidate every cached tessellation without saying so.
    BRepMesh_IncrementalMesh mesher;
    mesher.SetShape(record.shape);
    IMeshTools_Parameters parameters;
    parameters.Deflection = linear_deflection;
    parameters.Angle = angular_deflection;
    parameters.Relative = relative != 0 ? Standard_True : Standard_False;
    parameters.InParallel = Standard_False;
    mesher.ChangeParameters() = parameters;
    mesher.Perform(indicator->Start());

    if (cancelled(cancel, cancel_context)) {
      return FC_OCCT_CANCELLED;
    }

    std::vector<float> positions;
    std::vector<float> normals;
    std::vector<uint32_t> indices;
    std::vector<uint64_t> face_shapes;
    std::vector<uint32_t> face_first;
    std::vector<uint32_t> face_index_count;

    for (TopExp_Explorer it(record.shape, TopAbs_FACE); it.More(); it.Next()) {
      const TopoDS_Face face = TopoDS::Face(it.Current());
      TopLoc_Location location;
      const Handle(Poly_Triangulation) triangulation =
          BRep_Tool::Triangulation(face, location);
      if (triangulation.IsNull() || triangulation->NbTriangles() == 0) {
        // A face the mesher could not triangulate is reported as absent
        // rather than as an empty range: a range of no triangles would look
        // like a face that is genuinely invisible.
        continue;
      }

      const gp_Trsf transform = location.Transformation();
      const bool reversed = face.Orientation() == TopAbs_REVERSED;
      const int node_count = triangulation->NbNodes();
      const uint32_t base = static_cast<uint32_t>(positions.size() / 3);

      std::vector<gp_Pnt> nodes;
      nodes.reserve(static_cast<size_t>(node_count));
      for (int i = 1; i <= node_count; ++i) {
        nodes.push_back(triangulation->Node(i).Transformed(transform));
      }

      // Accumulated from the triangles meeting at each node, which is exact
      // for a plane and smooth across a cylinder. The triangulation itself
      // carries no normals on the versions this bridge is built against.
      std::vector<gp_Vec> accumulated(static_cast<size_t>(node_count),
                                      gp_Vec(0.0, 0.0, 0.0));
      const uint32_t first_index = static_cast<uint32_t>(indices.size());

      for (int t = 1; t <= triangulation->NbTriangles(); ++t) {
        int a = 0;
        int b = 0;
        int c = 0;
        triangulation->Triangle(t).Get(a, b, c);
        if (reversed) {
          std::swap(b, c);
        }

        const gp_Vec edge1(nodes[a - 1], nodes[b - 1]);
        const gp_Vec edge2(nodes[a - 1], nodes[c - 1]);
        const gp_Vec cross = edge1.Crossed(edge2);
        if (cross.SquareMagnitude() > 1.0e-24) {
          accumulated[a - 1] += cross;
          accumulated[b - 1] += cross;
          accumulated[c - 1] += cross;
        }

        indices.push_back(base + static_cast<uint32_t>(a - 1));
        indices.push_back(base + static_cast<uint32_t>(b - 1));
        indices.push_back(base + static_cast<uint32_t>(c - 1));
      }

      for (int i = 0; i < node_count; ++i) {
        const gp_Pnt &point = nodes[static_cast<size_t>(i)];
        positions.push_back(static_cast<float>(point.X()));
        positions.push_back(static_cast<float>(point.Y()));
        positions.push_back(static_cast<float>(point.Z()));

        gp_Vec normal = accumulated[static_cast<size_t>(i)];
        if (normal.SquareMagnitude() > 1.0e-24) {
          normal.Normalize();
        } else {
          // Every triangle at this node was degenerate. Say so with a zero
          // rather than inventing a direction that would light it wrongly.
          normal = gp_Vec(0.0, 0.0, 0.0);
        }
        normals.push_back(static_cast<float>(normal.X()));
        normals.push_back(static_cast<float>(normal.Y()));
        normals.push_back(static_cast<float>(normal.Z()));
      }

      face_shapes.push_back(record.remember(face));
      face_first.push_back(first_index);
      face_index_count.push_back(static_cast<uint32_t>(indices.size()) -
                                 first_index);
    }

    *out_vertex_count = positions.size() / 3;
    *out_index_count = indices.size();
    *out_face_count = face_shapes.size();

    if (vertex_capacity == 0 && index_capacity == 0 && face_capacity == 0) {
      return FC_OCCT_OK;
    }
    if (vertex_capacity < positions.size() / 3 ||
        index_capacity < indices.size() || face_capacity < face_shapes.size()) {
      write_error(out_error, "the mesh buffers are too small: " +
                                 std::to_string(positions.size() / 3) +
                                 " vertices, " + std::to_string(indices.size()) +
                                 " indices, " + std::to_string(face_shapes.size()) +
                                 " faces are needed");
      return FC_OCCT_INVALID_INPUT;
    }
    if (out_positions == nullptr || out_normals == nullptr ||
        out_indices == nullptr || out_face_shapes == nullptr ||
        out_face_first == nullptr || out_face_index_count == nullptr) {
      write_error(out_error,
                  "fc_occt_tessellate was given capacity but no buffer");
      return FC_OCCT_INVALID_INPUT;
    }

    std::memcpy(out_positions, positions.data(), positions.size() * sizeof(float));
    std::memcpy(out_normals, normals.data(), normals.size() * sizeof(float));
    std::memcpy(out_indices, indices.data(), indices.size() * sizeof(uint32_t));
    std::memcpy(out_face_shapes, face_shapes.data(),
                face_shapes.size() * sizeof(uint64_t));
    std::memcpy(out_face_first, face_first.data(),
                face_first.size() * sizeof(uint32_t));
    std::memcpy(out_face_index_count, face_index_count.data(),
                face_index_count.size() * sizeof(uint32_t));
    return FC_OCCT_OK;
  });
}

FcOcctStatus fc_occt_encode_shape_named(FcOcctSession *session, uint64_t shape,
                                        const uint64_t *sub_shapes,
                                        size_t sub_shape_count,
                                        uint32_t *out_slots, uint8_t *out_bytes,
                                        size_t capacity, size_t *out_length,
                                        FcOcctError *out_error) noexcept {
  return guarded(out_error, [&]() -> FcOcctStatus {
    if (session == nullptr || out_length == nullptr ||
        (sub_shape_count > 0 && (sub_shapes == nullptr || out_slots == nullptr))) {
      write_error(out_error,
                  "fc_occt_encode_shape_named was given a null argument");
      return FC_OCCT_INVALID_INPUT;
    }
    const auto found = session->shapes.find(shape);
    if (found == session->shapes.end()) {
      write_error(out_error, "shape " + std::to_string(shape) +
                                 " was released or never existed");
      return FC_OCCT_UNKNOWN_HANDLE;
    }

    // The archive is written down deliberately rather than discovered: the
    // shape, then each requested sub-shape, in the order asked for.
    BRep_Builder builder;
    TopoDS_Compound archive;
    builder.MakeCompound(archive);
    builder.Add(archive, found->second.shape);

    for (size_t i = 0; i < sub_shape_count; ++i) {
      const uint64_t sub_id = sub_shapes[i];
      if (sub_id >= found->second.sub_shapes.size()) {
        write_error(out_error, "sub-shape " + std::to_string(sub_id) +
                                   " does not belong to shape " +
                                   std::to_string(shape));
        return FC_OCCT_UNKNOWN_HANDLE;
      }
      const TopoDS_Shape &sub_shape = found->second.sub_shapes[sub_id];

      // A stale identifier could name a sub-shape of some earlier result. An
      // archive whose entries are not part of the shape it archives would
      // hand back faces of the wrong solid.
      bool contained = false;
      for (TopExp_Explorer it(found->second.shape, sub_shape.ShapeType());
           it.More(); it.Next()) {
        if (it.Current().IsSame(sub_shape)) {
          contained = true;
          break;
        }
      }
      if (!contained) {
        write_error(out_error, "sub-shape " + std::to_string(sub_id) +
                                   " is not part of the shape being archived");
        return FC_OCCT_INVALID_INPUT;
      }

      builder.Add(archive, sub_shape);
      out_slots[i] = static_cast<uint32_t>(i + 1);
    }

    std::ostringstream buffer(std::ios::out | std::ios::binary);
    BinTools::Write(archive, buffer, false, false,
                    BinTools_FormatVersion_CURRENT);
    if (!buffer.good()) {
      write_error(out_error, "Open CASCADE failed to serialise the archive");
      return FC_OCCT_KERNEL;
    }
    const std::string encoded = buffer.str();

    *out_length = encoded.size();
    if (capacity == 0) {
      return FC_OCCT_OK;
    }
    if (out_bytes == nullptr || capacity < encoded.size()) {
      write_error(out_error, "the buffer holds " + std::to_string(capacity) +
                                 " bytes but the archive needs " +
                                 std::to_string(encoded.size()));
      return FC_OCCT_INVALID_INPUT;
    }
    std::memcpy(out_bytes, encoded.data(), encoded.size());
    return FC_OCCT_OK;
  });
}

FcOcctStatus fc_occt_decode_shape_named(FcOcctSession *session,
                                        const uint8_t *bytes, size_t length,
                                        const uint32_t *slots,
                                        size_t slot_count, uint64_t *out_shape,
                                        uint64_t *out_sub_shapes,
                                        FcOcctError *out_error) noexcept {
  return guarded(out_error, [&]() -> FcOcctStatus {
    if (session == nullptr || bytes == nullptr || out_shape == nullptr ||
        (slot_count > 0 && (slots == nullptr || out_sub_shapes == nullptr))) {
      write_error(out_error,
                  "fc_occt_decode_shape_named was given a null argument");
      return FC_OCCT_INVALID_INPUT;
    }
    if (length == 0) {
      write_error(out_error, "a cached archive cannot be empty");
      return FC_OCCT_INVALID_INPUT;
    }

    std::istringstream buffer(
        std::string(reinterpret_cast<const char *>(bytes), length),
        std::ios::in | std::ios::binary);

    TopoDS_Shape restored;
    BinTools::Read(restored, buffer);
    if (restored.IsNull() || restored.ShapeType() != TopAbs_COMPOUND) {
      write_error(out_error,
                  "the cached bytes are not an archive written by this bridge");
      return FC_OCCT_KERNEL;
    }
    if (buffer.peek() != std::char_traits<char>::eof()) {
      write_error(out_error,
                  "the cached archive has trailing bytes after its B-Rep");
      return FC_OCCT_INVALID_INPUT;
    }

    std::vector<TopoDS_Shape> entries;
    for (TopoDS_Iterator it(restored); it.More(); it.Next()) {
      entries.push_back(it.Value());
    }
    if (entries.empty()) {
      write_error(out_error, "the archive holds no shape");
      return FC_OCCT_KERNEL;
    }

    ShapeRecord record;
    record.shape = entries[0];
    // Still decoded: an archive carries the sub-shapes that were named, not
    // the history of the operation that made them.
    record.decoded = true;

    // Every entry after the root must be a face of that root, even if the
    // caller's table does not request it. A valid archive written above has no
    // unrelated padding entries, so accepting one would make the decoder a
    // parser for a looser and undocumented format. Keep the root's own face
    // occurrence rather than the separately archived copy so orientation also
    // comes from the restored shape.
    std::vector<TopoDS_Shape> canonical(entries.size());
    canonical[0] = record.shape;
    for (size_t entry = 1; entry < entries.size(); ++entry) {
      const TopoDS_Shape &sub_shape = entries[entry];
      if (sub_shape.ShapeType() != TopAbs_FACE) {
        write_error(out_error, "archive entry " + std::to_string(entry) +
                                   " is not a face");
        return FC_OCCT_INVALID_INPUT;
      }

      bool contained = false;
      for (TopExp_Explorer it(record.shape, TopAbs_FACE); it.More(); it.Next()) {
        if (it.Current().IsSame(sub_shape)) {
          canonical[entry] = it.Current();
          contained = true;
          break;
        }
      }
      if (!contained) {
        write_error(out_error, "archive entry " + std::to_string(entry) +
                                   " is not part of the archived shape");
        return FC_OCCT_INVALID_INPUT;
      }
    }

    std::vector<uint64_t> resolved(slot_count, 0);
    for (size_t i = 0; i < slot_count; ++i) {
      const uint32_t slot = slots[i];
      if (slot == 0) {
        write_error(out_error,
                    "slot 0 is the archived shape itself, not a sub-shape");
        return FC_OCCT_INVALID_INPUT;
      }
      if (static_cast<size_t>(slot) >= entries.size()) {
        write_error(out_error, "slot " + std::to_string(slot) +
                                   " is outside an archive of " +
                                   std::to_string(entries.size() - 1) +
                                   " sub-shapes");
        return FC_OCCT_INVALID_INPUT;
      }
      resolved[i] = record.remember(canonical[slot]);
    }

    const uint64_t id = session->next_shape++;
    session->shapes.emplace(id, std::move(record));
    *out_shape = id;
    for (size_t i = 0; i < slot_count; ++i) {
      out_sub_shapes[i] = resolved[i];
    }
    return FC_OCCT_OK;
  });
}

void fc_occt_release_shape(FcOcctSession *session, uint64_t shape) noexcept {
  if (session != nullptr) {
    session->shapes.erase(shape);
  }
}

size_t fc_occt_live_shape_count(const FcOcctSession *session) noexcept {
  return session == nullptr ? 0 : session->shapes.size();
}

} // extern "C"
